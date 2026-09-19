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
//!
//! The identifier-dependent question is [`topology::is_subgraph`]: are all nodes and edges of one
//! graph present in the other, with the same identifiers and sequences?
//! The [`comparison`] module combines the two into a single verdict on a pair of graphs.
//!
//! Two graphs may also represent the same pangenome without being isomorphic, because one of them
//! has chopped long nodes into shorter fragments.
//! The [`unitigs`] module collapses each maximal non-branching path into a single node, which
//! removes the difference.

pub mod algorithms;
pub mod comparison;
pub mod graph;
pub mod isomorphism;
pub mod topology;
pub mod unitigs;

#[cfg(test)]
mod test_utils;

pub use algorithms::stable_name;
pub use graph::Graph;
pub use topology::Topology;
