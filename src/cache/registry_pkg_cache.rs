// Copyright 2017-2020 Matthias Krüger. See the COPYRIGHT
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use std::fs;
use std::path::PathBuf;

use crate::cache::caches::{get_cache_name, registry_dirs, RegistrySubCache, RegistrySuperCache};

use rayon::prelude::*;

/// holds information on directory with .crates for one registry (subcache)
pub(crate) struct RegistryPkgCache {
    /// the name of the index
    name: String,
    /// the path of the root dir of the index, this is unique
    path: PathBuf,
    /// total size of the index, computed on-demand
    size: Option<u64>,
    /// number of files of the cache
    number_of_files: Option<usize>,
    /// flag to check if we have already calculated the files
    files_calculated: bool, // TODO: make this Option<Vec<PathBuf>>
    /// is `files` already sorted? avoids re-sorting on every call
    files_sorted: bool,
    /// list of files contained in the index
    files: Vec<PathBuf>,
}

impl RegistrySubCache for RegistryPkgCache {
    /// create a new empty `RegistryPkgCache`
    fn new(path: PathBuf) -> Self {
        Self {
            name: get_cache_name(&path),
            path,
            size: None,
            number_of_files: None,
            files_calculated: false,
            files_sorted: false,
            files: vec![],
        }
    }

    // returns the name of the registry
    fn name(&self) -> &str {
        &self.name
    }

    fn path(&self) -> &PathBuf {
        &self.path
    }

    /// invalidate the cache
    #[inline]
    fn invalidate(&mut self) {
        self.size = None;
        self.files_calculated = false;
        self.files_sorted = false;
        self.number_of_files = None;
        self.files = vec![];
    }

    fn known_to_be_empty(&mut self) {
        self.size = Some(0);
        self.files_calculated = true;
        self.files_sorted = true;
        self.number_of_files = Some(0);
        self.files = Vec::new();
    }

    fn total_size(&mut self) -> u64 {
        if let Some(size) = self.size {
            return size;
        }
        // one stat() per file (instead of is_file() + metadata()), in parallel.
        // a file that vanished in the meantime counts as 0 instead of panicking.
        let size: u64 = self
            .files()
            .par_iter()
            .map(|f| {
                fs::metadata(f)
                    .ok()
                    .filter(fs::Metadata::is_file)
                    .map_or(0, |m| m.len())
            })
            .sum();
        self.size = Some(size);
        size
    }

    // return a slice of files belonging to this cache
    fn files(&mut self) -> &[PathBuf] {
        if !self.files_calculated {
            // read_dir on a missing directory just yields an error -> empty list,
            // no need for a separate exists() syscall
            let collection = fs::read_dir(&self.path)
                .map(|entries| {
                    entries
                        .filter_map(Result::ok)
                        .map(|entry| entry.path())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();

            self.number_of_files = Some(collection.len());
            self.files = collection;
            self.files_calculated = true;
            self.files_sorted = false;
        }
        &self.files
    }

    // number of files of the cache
    fn number_of_files(&mut self) -> usize {
        if let Some(number) = self.number_of_files {
            number
        } else {
            // prime the cache; this sets number_of_files
            self.files().len()
        }
    }

    // sort the saved files (only once) and return them
    fn files_sorted(&mut self) -> &[PathBuf] {
        let _ = self.files(); // prime cache
        if !self.files_sorted {
            self.files.sort_unstable(); // paths are unique, stability is useless
            self.files_sorted = true;
        }
        &self.files
    }

    fn items(&mut self) -> &[PathBuf] {
        // we can use files() here
        self.files()
    }

    fn number_of_items(&mut self) -> usize {
        // we can use number_of_files() here
        self.number_of_files()
    }
}

/// holds several `RegistryPkgCaches` (supercache)
pub(crate) struct RegistryPkgCaches {
    /// root path of the cache
    #[allow(unused)]
    path: PathBuf,
    /// list of pkg caches (from alternative registries or so)
    caches: Vec<RegistryPkgCache>,
    /// number of pkg caches found
    #[allow(unused)]
    number_of_caches: usize,
    /// total size of all indices combined
    total_size: Option<u64>,
    /// number of files of all indices combined
    total_number_of_files: Option<usize>,
    // items @TODO
    items: Vec<PathBuf>,
}

impl RegistrySuperCache for RegistryPkgCaches {
    type SubCache = RegistryPkgCache;

    /// create a new empty `RegistryPkgCaches`
    fn new(path: PathBuf) -> Self {
        // empty if the directory does not exist
        let caches = registry_dirs(&path)
            .into_iter()
            .map(RegistryPkgCache::new)
            .collect::<Vec<RegistryPkgCache>>();

        Self {
            path,
            number_of_caches: caches.len(),
            caches,
            total_number_of_files: None,
            total_size: None,
            items: Vec::new(),
        }
    }

    fn caches(&mut self) -> &mut Vec<Self::SubCache> {
        &mut self.caches
    }

    fn invalidate(&mut self) {
        self.number_of_caches = 0;
        self.total_size = None;
        self.total_number_of_files = None;
        self.items = Vec::new();
        self.caches
            .iter_mut()
            .for_each(RegistrySubCache::invalidate);
    }

    fn files(&mut self) -> Vec<PathBuf> {
        let mut all_files = Vec::new();
        for cache in &mut self.caches {
            // copy straight from the slice, no intermediate Vec
            all_files.extend_from_slice(cache.files());
        }
        all_files
    }

    fn files_sorted(&mut self) -> Vec<PathBuf> {
        let mut files_sorted = self.files();
        files_sorted.sort_unstable();
        files_sorted
    }

    // total size of all caches combined
    fn total_size(&mut self) -> u64 {
        if let Some(size) = self.total_size {
            size
        } else {
            // registries are independent: compute them in parallel
            let total_size: u64 = self
                .caches
                .par_iter_mut()
                .map(|cache| cache.total_size())
                .sum();
            self.total_size = Some(total_size);
            total_size
        }
    }

    fn number_of_subcaches(&mut self) -> usize {
        self.caches.len()
    }

    fn total_number_of_files(&mut self) -> usize {
        if let Some(number) = self.total_number_of_files {
            number
        } else {
            let number: usize = self
                .caches
                .iter_mut()
                .map(RegistrySubCache::number_of_files)
                .sum();

            self.total_number_of_files = Some(number);
            number
        }
    }

    fn items(&mut self) -> &[PathBuf] {
        // not memoized on purpose: callers can mutate subcaches through caches(),
        // and the subcaches are already cached so this is cheap.
        self.items.clear();
        for cache in &mut self.caches {
            self.items.extend_from_slice(cache.items());
        }
        &self.items
    }

    fn number_of_items(&mut self) -> usize {
        // used to clone every PathBuf just to call len()
        self.caches
            .iter_mut()
            .map(RegistrySubCache::number_of_items)
            .sum()
    }
}