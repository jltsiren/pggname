# Pggname releases

## Pggname 0.4.0 (unreleased)

* New `Topology` trait for structural queries, with implementations for GFA and GBZ graphs.
* Determines whether two graphs are isomorphic at the level of maximal non-branching paths, which
  sees past chopped nodes. A node may map to the reverse complement of another node.
* A positive answer is a translation that pairs each path in the first graph with the path in the
  second graph that spells the same sequence.
* `pggname --compare` compares two graphs, with `--translation` for writing the translation.
* An identifier-independent graph name based on the same machinery is under consideration.
  It is not exposed yet, because color refinement cannot tell all graphs apart.
* Removed `pggname --benchmark` and the options for choosing between integer and string node
  identifiers. GFA graphs still use integer identifiers when possible and string identifiers
  otherwise.

## Pggname 0.3.0 (2026-08-24)

* Supports GBZ version 3 with Zstandard compressed BWT.
* Uses the `gbz` implementation of `GraphName`.
* `pggname --store-name` now writes the same GBZ version it read.

## Pggname 0.2.2 (2026-05-05)

* Sets `target-cpu=native` by default.

## Pggname 0.2.1 (2026-04-17)

* Uses `simple_sds` version 0.4.1 and `gbz` version 0.6.1.

## Pggname 0.2.0 (2026-02-12)

* Supports GBZ version 2 with Zstandard compressed sequences.

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
