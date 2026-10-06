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

//-----------------------------------------------------------------------------

use gbz::support::{self, Orientation, NodeSide};

/// Maximum number of nodes in the graph for subgraph and isomorphism algorithms.
pub const MAX_NODES: usize = 1 << 31;

/// A wrapper around [`support::encode_node`].
///
/// Some algorithms use the GBWT encoding for oriented nodes but store them as [`u32`] to save space.
pub fn encode_oriented_node(node: usize, orientation: Orientation) -> u32 {
    support::encode_node(node, orientation) as u32
}

/// A wrapper around [`support::decode_node`].
///
/// Some algorithms use the GBWT encoding for oriented nodes but store them as [`u32`] to save space.
pub fn decode_oriented_node(encoded: u32) -> (usize, Orientation) {
    support::decode_node(encoded as usize)
}

// FIXME: walk -> path everywhere, except when it refers to a GFA W-line
/// Returns `true` if the encoded walk is canonical.
///
/// Some algorithms use the GBWT encoding for oriented nodes but store them as [`u32`] to save space.
/// See [`support::path_is_canonical`].
pub fn encoded_walk_is_canonical(walk: &[u32]) -> bool {
    if walk.is_empty() {
        return true;
    }
    let (from, to) = (walk[0], walk[walk.len() - 1]);
    if decode_oriented_node(from).1 == Orientation::Forward && decode_oriented_node(to).1 == Orientation::Forward {
        return true;
    }
    let (from, to) = (from as usize, to as usize);
    support::encoded_edge_is_canonical(from, to)
}

/// Reverses the given encoded walk in place.
///
/// Some algorithms use the GBWT encoding for oriented nodes but store them as [`u32`] to save space.
/// See [`support::reverse_path_in_place`].
pub fn reverse_encoded_walk(walk: &mut [u32]) {
    walk.reverse();
    for encoded_node in walk.iter_mut() {
        *encoded_node = support::flip_node(*encoded_node as usize) as u32;
    }
}

/// A wrapper around [`support::encode_node_side`].
///
/// Some algorithms use the GBWT encoding for node sides but store them as [`u32`] to save space.
pub fn encode_node_side(node: usize, side: NodeSide) -> u32 {
    support::encode_node_side(node, side) as u32
}

/// A wrapper around [`support::decode_node_side`].
///
/// Some algorithms use the GBWT encoding for node sides but store them as [`u32`] to save space.
pub fn decode_node_side(encoded: u32) -> (usize, NodeSide) {
    support::decode_node_side(encoded as usize)
}

//-----------------------------------------------------------------------------
