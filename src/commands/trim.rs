// Copyright 2020 Matthias Krüger. See the COPYRIGHT
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

// "cargo cache trim" command
// trim the size of the cargo cache down to a certain limit.
// note that this does not take account the registry indices and the installed binaries in calculations

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::cache::caches::*;
use crate::cache::*;
use crate::library::*;
use crate::remove::*;

use humansize::{FormatSize, DECIMAL};
use rayon::prelude::*;
use walkdir::WalkDir;

/// walks an item (file or directory) ONCE and returns
/// (most recent access time of everything inside it, total size of its files).
/// the metadata is taken from the walkdir entries, so there is no second stat() per file,
/// and it never panics on unreadable entries or filesystems without atime support.
fn scan_item(path: &Path) -> (SystemTime, u64) {
    let mut latest = SystemTime::UNIX_EPOCH;
    let mut size = 0_u64;

    // WalkDir on a plain file yields just that file, so files and directories share this path
    for entry in WalkDir::new(path).into_iter().filter_map(Result::ok) {
        if let Ok(md) = entry.metadata() {
            // fall back to the modification time if atime is not available
            if let Ok(time) = md.accessed().or_else(|_| md.modified()) {
                latest = latest.max(time);
            }
            if md.is_file() {
                size += md.len();
            }
        }
    }
    (latest, size)
}

/// all cache items with their size, sorted by last access (youngest first).
/// the (expensive) disk scan of every item runs in parallel.
fn gather_all_cache_items_with_size<'a>(
    git_checkouts_cache: &'a mut git_checkouts::GitCheckoutCache,
    bare_repos_cache: &'a mut git_bare_repos::GitRepoCache,
    registry_pkg_cache: &'a mut registry_pkg_cache::RegistryPkgCaches,
    registry_sources_cache: &'a mut registry_sources::RegistrySourceCaches,
) -> Vec<(&'a PathBuf, u64)> {
    let mut all_items: Vec<&PathBuf> = Vec::new();
    all_items.extend(git_checkouts_cache.items());
    all_items.extend(bare_repos_cache.items());
    all_items.extend(registry_pkg_cache.items());
    all_items.extend(registry_sources_cache.items());

    // access time and size are computed together, once per item, in parallel
    let mut scanned: Vec<(&PathBuf, SystemTime, u64)> = all_items
        .par_iter()
        .map(|path| {
            let (accessed, size) = scan_item(path);
            (*path, accessed, size)
        })
        .collect();

    // youngest access first: [2020, 2019, 2018, ....]
    scanned.sort_by(|a, b| b.1.cmp(&a.1));

    scanned
        .into_iter()
        .map(|(path, _accessed, size)| (path, size))
        .collect()
}

// get a list of all cache items, sorted by file access time (young to old)
#[allow(dead_code)]
pub(crate) fn gather_all_cache_items<'a>(
    git_checkouts_cache: &'a mut git_checkouts::GitCheckoutCache,
    bare_repos_cache: &'a mut git_bare_repos::GitRepoCache,
    registry_pkg_cache: &'a mut registry_pkg_cache::RegistryPkgCaches,
    registry_sources_cache: &'a mut registry_sources::RegistrySourceCaches,
) -> Vec<&'a PathBuf> {
    gather_all_cache_items_with_size(
        git_checkouts_cache,
        bare_repos_cache,
        registry_pkg_cache,
        registry_sources_cache,
    )
    .into_iter()
    .map(|(path, _size)| path)
    .collect()
}

/// figure out how big the cache should remain after trimming
fn parse_size_limit_to_bytes(limit: Option<&str>) -> Result<u64, Error> {
    match limit {
        None => unreachable!("No trim --limit was supplied although clap should enforce that!"),
        Some(limit) => {
            let err = || Error::TrimLimitUnitParseFailure(limit.to_string());

            // the last character is the unit: B, K, M, G, T (case insensitive)
            let unit_char = limit.chars().last().ok_or_else(err)?;
            let unit_multiplicator: u64 = match unit_char.to_ascii_lowercase() {
                'b' => 1,
                'k' => 1024,
                'm' => 1024 * 1024,
                'g' => 1024 * 1024 * 1024,
                't' => 1024 * 1024 * 1024 * 1024,
                _ => return Err(err()),
            };

            // the unit is ASCII here, so this slice can never cut a multibyte character
            // (the old code could panic on e.g. "1é" or underflow on "")
            let number = &limit[..limit.len() - unit_char.len_utf8()];
            let value: f64 = number.parse().map_err(|_| err())?;

            // SAFETY NET: "-5M" or "NaN" used to be cast to 0, meaning "delete EVERYTHING"
            if !value.is_finite() || value < 0.0 {
                return Err(err());
            }

            // we may truncate the value here but that's ok
            #[allow(clippy::cast_sign_loss)]
            #[allow(clippy::cast_possible_truncation)]
            #[allow(clippy::cast_precision_loss)]
            Ok((value * unit_multiplicator as f64) as u64)
        }
    }
}

/// trim the cache to a certain limit and invalidate caches
pub(crate) fn trim_cache(
    unparsed_size_limit: Option<&str>,
    git_checkouts_cache: &mut git_checkouts::GitCheckoutCache,
    bare_repos_cache: &mut git_bare_repos::GitRepoCache,
    registry_pkg_cache: &mut registry_pkg_cache::RegistryPkgCaches,
    registry_sources_cache: &mut registry_sources::RegistrySourceCaches,
    dry_run: bool,
    size_changed: &mut bool,
) -> Result<(), Error> {
    // the cache should not exceed this limit
    let size_limit = parse_size_limit_to_bytes(unparsed_size_limit)?;

    // fast path:
    // if the cache already fits in the limit, we know we won't have to delete anything
    let total_cache_size: u64 = git_checkouts_cache.total_size()
        + bare_repos_cache.total_size()
        + registry_pkg_cache.total_size()
        + registry_sources_cache.total_size();

    if size_limit >= total_cache_size {
        return Ok(());
    }

    // get all the items of the cache with their sizes, youngest access first
    let all_cache_items = gather_all_cache_items_with_size(
        git_checkouts_cache,
        bare_repos_cache,
        registry_pkg_cache,
        registry_sources_cache,
    );

    let mut cache_size: u64 = 0;
    let mut removed_size: u64 = 0;
    let mut removed_item_count: usize = 0;

    // walk from youngest to oldest: keep items until the size limit is reached,
    // everything after that point (the older items) gets removed
    for &(path, item_size) in &all_cache_items {
        cache_size += item_size;
        if cache_size > size_limit {
            removed_size += item_size;
            removed_item_count += 1;
            remove_file(
                path,
                dry_run,
                size_changed,
                None,
                &DryRunMessage::Default,
                None,
            );
        }
    }

    // invalidate caches that we might have touched
    git_checkouts_cache.invalidate();
    bare_repos_cache.invalidate();
    registry_pkg_cache.invalidate();
    registry_sources_cache.invalidate();

    println!(
        "Removed {} items totalling {}",
        removed_item_count,
        removed_size.format_size(DECIMAL)
    );
    Ok(())
}

#[cfg(test)]
mod parse_size_limit {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn size_limit() {
        // shorter function name
        fn p(limit: Option<&str>) -> Result<u64, Error> {
            parse_size_limit_to_bytes(limit)
        }

        assert_eq!(p(Some("1b")).unwrap(), 1);
        assert_eq!(p(Some("1B")).unwrap(), 1);

        assert_eq!(p(Some("1k")).unwrap(), 1_024);
        assert_eq!(p(Some("1K")).unwrap(), 1_024);

        assert_eq!(p(Some("1m")).unwrap(), 1_048_576);
        assert_eq!(p(Some("1M")).unwrap(), 1_048_576);

        assert_eq!(p(Some("1g")).unwrap(), 1_073_741_824);
        assert_eq!(p(Some("1G")).unwrap(), 1_073_741_824);

        assert_eq!(p(Some("1t")).unwrap(), 1_099_511_627_776);
        assert_eq!(p(Some("1T")).unwrap(), 1_099_511_627_776);

        assert_eq!(p(Some("4M")).unwrap(), 4_194_304);
        assert_eq!(p(Some("42M")).unwrap(), 44_040_192);
        assert_eq!(p(Some("1337M")).unwrap(), 1_401_946_112);

        assert_eq!(p(Some("1.5k")).unwrap(), 1_536);
        assert_eq!(p(Some("0M")).unwrap(), 0);

        match p(Some("1_")) {
            Ok(_) => panic!("expected error"),
            Err(Error::TrimLimitUnitParseFailure(string)) => assert_eq!(string, "1_"),
            Err(..) => panic!("did not get enum variant TrimParseLimitUnitParseFailure"),
        }
    }

    // inputs that used to panic or silently mean "delete everything"
    #[test]
    fn size_limit_invalid_inputs_are_errors() {
        for bad in ["", "M", "-5M", "NaNM", "infM", "1é", "abcG"] {
            assert!(
                matches!(
                    parse_size_limit_to_bytes(Some(bad)),
                    Err(Error::TrimLimitUnitParseFailure(_))
                ),
                "expected an error for {bad:?}"
            );
        }
    }

    // make sure Size limit None panicss
    #[test]
    #[should_panic(expected = "No trim --limit was supplied although clap should enforce that!")]
    fn size_limit_none_panics() {
        let _ignore = parse_size_limit_to_bytes(None);
    }
}