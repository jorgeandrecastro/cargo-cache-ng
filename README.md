# cargo-cache-ng

`cargo-cache-ng` is a community fork of [`cargo-cache`](https://github.com/matthiaskrgr/cargo-cache), currently undergoing modernization and maintenance.

The program inspects the Cargo directory (`~/.cargo/` or `$CARGO_HOME`) and can display its size, inspect its components, and selectively remove cached items.

> [!NOTE]
> This is an independent fork published on crates.io as [`cargo-cache-ng`](https://crates.io/crates/cargo-cache-ng). It keeps the `cargo-cache` executable name to preserve compatibility with existing commands and scripts.

## Fork goals

- maintain and modernize the project;
- update Rust dependencies;
- improve code modularity and simplify internal paths;
- improve performance, especially for large caches;
- preserve command-line compatibility wherever possible;
- document changes and publish reproducible releases.

Updates are being applied progressively. Some parts inherited from the original project may still use older APIs or dependencies.

## What's new in 0.2.0

- Much faster scanning of large caches: registry sources, directory sizes and `trim` now walk the disk in parallel with a single `stat()` per file.
- The registry source cache no longer re-walks its directory on every access.
- `trim` rejects invalid `--limit` values (negative, `NaN`, empty, non-ASCII) instead of treating them as "delete everything".
- No more panics when a file disappears while sizes are being computed.

See [CHANGELOG.md](CHANGELOG.md) for the full list.

## Project structure

```text
cargo-cache-ng/
├── src/
│   ├── cache/
│   ├── commands/
│   ├── top_items/
│   ├── build.rs
│   ├── clean_unref.rs
│   ├── cli.rs
│   ├── dirsizes.rs
│   ├── library.rs
│   ├── main.rs
│   ├── remove.rs
│   └── test_helpers.rs
├── data/
├── utils/
├── Cargo.toml
├── Cargo.lock
├── CHANGELOG.md
├── README.md
├── COPYRIGHT
├── LICENSE-MIT
└── LICENSE-APACHE
```

## Features

- show the total Cargo cache size and its components;
- remove source and Git repository checkouts;
- recompress Git repositories, verify them, or perform cleanup;
- preview every modifying operation with `--dry-run`;
- remove items based on their age;
- keep only a limited number of crate versions;
- list the largest cached items;
- search cache entries with regular expressions;
- support alternative registries;
- inspect local project caches and `sccache`;
- remove crates not referenced by a `Cargo.toml`;
- verify extracted crate sources;
- trim the cache to a maximum size;
- support `$CARGO_HOME` and custom locations.

## Installation from this fork

The minimum supported Rust version (MSRV) is **1.85**.

With a working Rust toolchain:

```bash
cargo install cargo-cache-ng
```

To install the latest development version from Git:

```bash
cargo install --git https://github.com/jorgeandrecastro/cargo-cache-ng cargo-cache-ng
```

To install from a local copy:

```bash
git clone https://github.com/jorgeandrecastro/cargo-cache-ng
cd cargo-cache-ng
cargo install --path .
```

Installing the package provides the `cargo-cache` executable. The package can be removed with Cargo:

```bash
cargo uninstall cargo-cache-ng
```

## Usage

Display the cache summary:

```bash
cargo cache
```

Show the available options:

```bash
cargo cache --help
```

Common examples:

```bash
# Preview a cleanup
cargo cache --dry-run --autoclean

# Remove extracted sources and Git checkouts
cargo cache --autoclean

# Remove items of a directory that are older than a date
# (--remove-if-older-than / --remove-if-younger-than require --remove-dir)
cargo cache --remove-dir git-db --remove-if-older-than 2024.01.01

# Keep only the latest 3 archived versions
cargo cache --keep-duplicate-crates 3

# Show the 20 largest items
cargo cache --top-cache-items 20

# Search the cache
cargo cache query "crate.*name"

# Inspect a local project cache
cargo cache local

# Verify extracted sources
cargo cache verify

# Trim the cache to a size limit (unit is one letter: B, K, M, G or T)
cargo cache trim --limit 10G
```

Operations that modify disk contents generally support `--dry-run`. Check the exact options of the installed version with `cargo cache --help`, then use a dry run before an important cleanup.

## Example output

```text
cargo cache

Cargo cache '/home/user/.cargo':

Total: 4.22 GB
  102 installed binaries:           920.95 MB
  Registry:                           2.25 GB
    Registry index:                 227.07 MB
    4412 crate archives:            684.29 MB
    2411 crate source checkouts:      1.34 GB
  Git db:                             1.05 GB
    113 bare git repos:             993.72 MB
    9 git repo checkouts:            55.48 MB
```

This output is illustrative: paths and sizes depend on the local system.

## Cache usage in CI

Files stored in `$CARGO_HOME` can be large when saved as CI cache artifacts. Preview the cleanup and run a lightweight cleanup with:

```bash
cargo cache --dry-run --autoclean
cargo cache --autoclean
```

The project also retains a `ci-autoclean` mode for continuous integration environments:

```bash
cargo install --git https://github.com/jorgeandrecastro/cargo-cache-ng \
  --no-default-features --features ci-autoclean cargo-cache-ng
cargo-cache
```

This feature should be reviewed and validated before use in a production CI pipeline.

## Development

Contributions are welcome for bug fixes, performance reviews, module modernization, dependency updates, and documentation.

Before a substantial contribution, open an issue to discuss its scope and avoid incompatible changes. Changes should remain focused and preserve existing behavior.

Before opening a pull request, please run:

```bash
cargo fmt --check
cargo clippy --all-targets
cargo test
```

## Credits

This project builds on the original work by **Matthias Krüger** and all contributors to [`cargo-cache`](https://github.com/matthiaskrgr/cargo-cache).

We preserve the original attribution and respect the MIT and Apache-2.0 licenses under which the upstream code was distributed. Derivative work and modifications must retain the applicable copyright and license notices.

## License

Copyright 2017-2022 Matthias Krüger, and the `cargo-cache-ng` contributors for the modifications made in this fork.

This project is distributed under either the [MIT License](LICENSE-MIT) or the [Apache-2.0 License](LICENSE-APACHE), at the user's option.

Files carrying notices from the original project remain subject to the same terms. Modified files must preserve existing copyright and license notices in accordance with the MIT and Apache-2.0 licenses.