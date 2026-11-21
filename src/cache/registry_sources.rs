// Copyright 2017-2020 Matthias Krüger. See the COPYRIGHT
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use std::fs;
use std::path::{Path, PathBuf};

use crate::cache::caches::{get_cache_name, registry_dirs, RegistrySubCache, RegistrySuperCache};

use rayon::prelude::*;
use walkdir::WalkDir;

/// walks one directory tree and returns (all files, total size of those files).
/// the size comes from the metadata walkdir already has, so no second stat() pass is needed.
fn scan_dir(root: &Path) -> (Vec<PathBuf>, u64) {
    let mut files = Vec::new();
    let mut size = 0_u64;

    for entry in WalkDir::new(root).into_iter().filter_map(Result::ok) {
        let file_type = entry.file_type();
        if file_type.is_file() {
            if let Ok(md) = entry.metadata() {
                size += md.len();
            }
            files.push(entry.into_path());
        } else if file_type.is_symlink() {
            // keep the old behaviour: symlinks to files count as files
            if let Ok(md) = fs::metadata(entry.path()) {
                if md.is_file() {
                    size += md.len();
                    files.push(entry.into_path());
                }
            }
        }
    }
    (files, size)
}

#[derive(Debug, Clone)]
/// describes one registry source cache (extracted .crates)
pub(crate) struct RegistrySourceCache {
    /// the name of the index
    name: String,
    /// the path of the root dir of the index, this is unique
    path: PathBuf,
    /// total size of the cache, computed on-demand
    size: Option<u64>,
    /// number of files of the cache
    number_of_files: Option<usize>,
    /// flag to check if we have already calculated the files
    files_calculated: bool, // TODO: make this Option<Vec<PathBuf>>
    /// is `files` already sorted? avoids re-sorting on every call
    files_sorted: bool,
    /// list of files contained in the index
    files: Vec<PathBuf>,
    /// have we calculated the checkout folders
    items_calculated: bool,
    /// the source checkout folders
    items: Vec<PathBuf>,
}

impl RegistrySourceCache {
    /// single pass over the disk: fills files, number of files AND total size.
    /// each crate directory (foo-1.2.3) is walked in parallel.
    fn scan(&mut self) {
        let roots: Vec<PathBuf> = fs::read_dir(&self.path)
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .map(|entry| entry.path())
                    .collect()
            })
            .unwrap_or_default();

        // collect() on an indexed parallel iterator keeps the original order
        let results: Vec<(Vec<PathBuf>, u64)> =
            roots.par_iter().map(|root| scan_dir(root)).collect();

        let total_files: usize = results.iter().map(|(f, _)| f.len()).sum();
        let mut files = Vec::with_capacity(total_files);
        let mut size = 0_u64;
        for (f, s) in results {
            files.extend(f);
            size += s;
        }

        self.number_of_files = Some(files.len());
        self.size = Some(size);
        self.files = files;
        self.files_calculated = true;
        self.files_sorted = false;
    }
}

impl RegistrySubCache for RegistrySourceCache {
    fn new(path: PathBuf) -> Self {
        Self {
            name: get_cache_name(&path),
            path,
            size: None,
            number_of_files: None,
            files_calculated: false,
            files_sorted: false,
            files: vec![],
            items_calculated: false,
            items: vec![],
        }
    }

    fn path(&self) -> &PathBuf {
        &self.path
    }

    // returns the name of the registry
    fn name(&self) -> &str {
        &self.name
    }

    /// invalidate the cache
    #[inline]
    fn invalidate(&mut self) {
        self.size = None;
        self.files_calculated = false;
        self.files_sorted = false;
        self.number_of_files = None;
        self.files = vec![];
        self.items_calculated = false;
        self.items = vec![];
    }

    fn known_to_be_empty(&mut self) {
        self.size = Some(0);
        self.files_calculated = true;
        self.files_sorted = true;
        self.number_of_files = Some(0);
        self.files = Vec::new();
        self.items_calculated = true;
        self.items = Vec::new();
    }

    fn files(&mut self) -> &[PathBuf] {
        // BUGFIX: the old code had an empty `if self.files_calculated {}` block and
        // then walked the whole directory again on EVERY call.
        if !self.files_calculated {
            if self.path_exists() {
                self.scan();
            } else {
                self.known_to_be_empty();
            }
        }
        &self.files
    }

    fn total_size(&mut self) -> u64 {
        if self.size.is_none() {
            // scan() computes the size in the same pass as the file list
            let _ = self.files();
        }
        self.size.unwrap_or(0)
    }

    fn files_sorted(&mut self) -> &[PathBuf] {
        let _ = self.files(); // prime cache
        if !self.files_sorted {
            self.files.sort_unstable(); // paths are unique, stability is useless
            self.files_sorted = true;
        }
        &self.files
    }

    fn number_of_files(&mut self) -> usize {
        if let Some(number_of_files) = self.number_of_files {
            number_of_files
        } else {
            // prime the cache; this sets number_of_files
            self.files().len()
        }
    }

    fn items(&mut self) -> &[PathBuf] {
        if !self.items_calculated {
            // registry_dirs() returns an empty list if the path does not exist
            self.items = registry_dirs(&self.path);
            self.items_calculated = true;
        }
        &self.items
    }

    fn number_of_items(&mut self) -> usize {
        self.items().len()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct RegistrySourceCaches {
    /// root path of the cache
    #[allow(unused)]
    path: PathBuf,
    /// list of pkg caches (from alternative registries or so)
    caches: Vec<RegistrySourceCache>,
    /// number of pkg caches found
    #[allow(unused)]
    number_of_caches: usize,
    /// total size of all indices combined
    total_size: Option<u64>,
    /// number of files of all indices combined
    total_number_of_files: Option<usize>,
    /// all source checkout folders
    items_calculated: bool,
    // items
    items: Vec<PathBuf>,
}

impl RegistrySuperCache for RegistrySourceCaches {
    type SubCache = RegistrySourceCache;

    fn caches(&mut self) -> &mut Vec<Self::SubCache> {
        &mut self.caches
    }

    fn new(path: PathBuf) -> Self {
        // empty if the directory does not exist
        let registry_folders = registry_dirs(&path)
            .into_iter()
            .map(RegistrySourceCache::new)
            .collect::<Vec<RegistrySourceCache>>();

        Self {
            path,
            number_of_caches: registry_folders.len(),
            caches: registry_folders,
            total_number_of_files: None,
            total_size: None,
            items_calculated: false,
            items: Vec::new(),
        }
    }

    fn invalidate(&mut self) {
        self.total_number_of_files = None;
        self.total_size = None;
        self.items = vec![];
        self.items_calculated = false;
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
            // registries are independent: scan them in parallel
            let total_size: u64 = self
                .caches
                .par_iter_mut()
                .map(|cache| cache.total_size())
                .sum();
            self.total_size = Some(total_size);
            total_size
        }
    }

    /// number of caches this supercache holds, should be equal to the number of registries
    fn number_of_subcaches(&mut self) -> usize {
        self.caches.len()
    }

    fn total_number_of_files(&mut self) -> usize {
        if let Some(number) = self.total_number_of_files {
            number
        } else {
            let total: usize = self
                .caches
                .par_iter_mut()
                .map(|cache| cache.number_of_files())
                .sum();

            self.total_number_of_files = Some(total);
            total
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

impl RegistrySourceCaches {
    pub(crate) fn total_checkout_folders_sorted(&mut self) -> &[PathBuf] {
        // BUGFIX: the old code called items() again after sorting, which rebuilt
        // the list and returned it UNSORTED.
        let _ = self.items(); // prime cache
        self.items.sort_unstable();
        &self.items
    }
}