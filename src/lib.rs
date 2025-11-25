//! Pangenome graph naming based on hashing in a canonical order.
//!
//! The stable graph name (pggname) of a pangenome graph is the SHA-256 hash of its canonical GFA representation.
//! In this representation, the nodes are listed in order.
//! Each node is followed by the edges adjacent to it, also in order.
//! Only edges where the canonical orientation goes from the current node to another node are included.
//!
//! The purpose of pggname is to identify only the graph itself.
//! Hence the canonical GFA representation does not include other information, such as headers, haplotype paths, or metadata.

pub mod graph;

pub use graph::Graph;
pub use graph::parse_gfa;

use sha2::Digest;
use sha2::digest;

//-----------------------------------------------------------------------------

// TODO: Should this be here? Maybe pub mod algorithms?
/// Computes the given hash of the canonical GFA representation of the given graph.
pub fn hash<D: Digest, G: Graph>(graph: &G) -> String
    where digest::Output<D>: core::fmt::LowerHex {
    let mut hasher = D::new();
    for bytes in graph.node_iter() {
        hasher.update(&bytes);
    }
    let hash = hasher.finalize();
    format!("{:x}", hash)
}

//-----------------------------------------------------------------------------
