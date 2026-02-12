# Pggname releases

## Pggname 0.2.0 (2026-02-12)

* Support GBZ version 2 with Zstandard compressed sequences.

## Pggname 0.1.0 (2025-12-26)

Initial release of the stable graph name scheme. The reference implementation supports GFA and GBZ graphs.

## Release process

* Run `cargo clippy`.
* Run tests with `cargo test`.
* Update version in `Cargo.toml`.
* Update `RELEASES.md`.
* Publish in crates.io with `cargo publish`.
* Push to GitHub.
* Draft a new release in GitHub.
