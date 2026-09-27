//! Pangenome graph naming based on hashing in a canonical order.
//!
//! The stable graph name (pggname) of a pangenome graph is the SHA-256 hash of its canonical GFA representation.
//! In this representation, the nodes are listed in order.
//! Each node is followed by the edges adjacent to it, also in order.
//! Only edges where the canonical orientation starts from the node are included.
//!
//! The purpose of pggname is to identify only the graph itself.
//! Hence the canonical GFA representation does not include other information, such as headers, haplotype paths, or metadata.
//! There are also algorithms for determining whether two graphs are isomorphic or one of them is a subgraph of the other.

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
