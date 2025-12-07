//! Pangenome graph naming based on hashing in a canonical order.
//!
//! The stable graph name (pggname) of a pangenome graph is the SHA-256 hash of its canonical GFA representation.
//! In this representation, the nodes are listed in order.
//! Each node is followed by the edges adjacent to it, also in order.
//! Only edges where the canonical orientation starts from the node are included.
//!
//! The purpose of pggname is to identify only the graph itself.
//! Hence the canonical GFA representation does not include other information, such as headers, haplotype paths, or metadata.

pub mod algorithms;
pub mod graph;
pub mod name;

pub use algorithms::stable_name;
pub use graph::Graph;
pub use name::GraphName;
