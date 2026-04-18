# Pggname releases

## Pggname 0.2.1 (2026-04-17)

* Uses `simple_sds` version 0.4.1 and `gbz` version 0.6.1.

## Pggname 0.2.0 (2026-02-12)

* Support GBZ version 2 with Zstandard compressed sequences.

## Pggname 0.1.0 (2025-12-26)

Initial release of the stable graph name scheme. The reference implementation supports GFA and GBZ graphs.

## Release process

* Clean up with `cargo clean`.
* Update version in `Cargo.toml`.
* Switch to crates.io versions of dependencies, if necessary.
* Update `RELEASES.md`.
* Run `cargo clippy`.
* Run tests with `cargo test`.
* Build documentation with `cargo doc`.
* Build the optimized version with `cargo build --release`.
* Commit the final changes for the release.
* Publish in crates.io with `cargo publish`.
* Push to GitHub.
* Draft a new release in GitHub.
