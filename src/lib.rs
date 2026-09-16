//! Pangenome graph naming based on hashing in a canonical order.
//!
//! The stable graph name (pggname) of a pangenome graph is the SHA-256 hash of its canonical GFA representation.
//! In this representation, the nodes are listed in order.
//! Each node is followed by the edges adjacent to it, also in order.
//! Only edges where the canonical orientation starts from the node are included.
//!
//! The purpose of pggname is to identify only the graph itself.
//! Hence the canonical GFA representation does not include other information, such as headers, haplotype paths, or metadata.
//!
//! Because the name depends on the node identifiers, graphs that differ only in the identifiers get
//! different names.
//! The [`isomorphism`] module answers the identifier-independent question: are two graphs the same
//! graph, up to renaming the nodes?
//! It builds on the [`topology`] module, which provides the structural view of a graph that the
//! [`Graph`] trait, being oriented towards canonical serialization, cannot.

pub mod algorithms;
pub mod graph;
pub mod isomorphism;
pub mod topology;

#[cfg(test)]
mod test_utils;

pub use algorithms::stable_name;
pub use graph::Graph;
pub use topology::Topology;
