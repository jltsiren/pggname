//! Expanding a unitig mapping into a correspondence between paths of nodes.
//!
//! An isomorphism between the unitig graphs of two graphs matches the sequences of the unitigs
//! position by position. Each graph cuts its unitigs into nodes in its own way, so a unitig becomes
//! a pair of walks: the nodes the first graph reads along the path, and the nodes the second graph
//! reads along the same sequence.

use crate::topology::Topology;
use crate::unitigs::{self, Unitigs};

use super::{Mismatch, NodeMapping};

use gbz::Orientation;
use gbz::support;

//-----------------------------------------------------------------------------

/// A correspondence between the maximal non-branching paths of two graphs.
///
/// Pair `i` consists of the walk formed by unitig `i` of the first graph and the walk in the second
/// graph that spells the same sequence. A walk is a sequence of nodes, each read in a given
/// orientation, and consecutive nodes in it are joined by an edge.
///
/// A pair can be read from either end, and both walks are then reversed together. The direction is
/// chosen so that the first node of the first walk is in forward orientation. A walk that begins in
/// reverse and ends in forward orientation begins in reverse from either end; it is left in the
/// canonical direction of the path, where the sequence is lexicographically smaller.
///
/// Because the two graphs cut the paths in different places, this is not a bijection between nodes:
/// a node of one graph may correspond to a part of a node of the other, or to several nodes in a
/// row.
///
/// The correspondence does not borrow the graphs. Use [`Topology::node_name`] to convert the
/// indexes back to node names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Translation {
    // The nodes of walk `i` are `first_offsets[i]..first_offsets[i + 1]`, encoded as
    // `2 * node + orientation`. The second graph is stored the same way.
    first: Vec<u32>,
    first_offsets: Vec<u32>,
    second: Vec<u32>,
    second_offsets: Vec<u32>,
}

impl Translation {
    /// Returns the number of walk pairs.
    pub fn len(&self) -> usize {
        self.first_offsets.len() - 1
    }

    /// Returns `true` if there are no walk pairs.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the walk in the first graph as `(node, orientation)` pairs.
    pub fn first_walk(&self, index: usize) -> impl ExactSizeIterator<Item = (usize, Orientation)> {
        walk(&self.first, &self.first_offsets, index)
    }

    /// Returns the walk in the second graph as `(node, orientation)` pairs.
    pub fn second_walk(&self, index: usize) -> impl ExactSizeIterator<Item = (usize, Orientation)> {
        walk(&self.second, &self.second_offsets, index)
    }
}

// Returns the given walk from the encoded nodes and the offsets.
fn walk(
    nodes: &[u32], offsets: &[u32], index: usize
) -> impl ExactSizeIterator<Item = (usize, Orientation)> {
    let range = offsets[index] as usize..offsets[index + 1] as usize;
    nodes[range].iter().map(|&encoded| decode(encoded))
}

// Encodes an oriented node as `2 * node + orientation`.
fn encode(node: usize, orientation: Orientation) -> u32 {
    (2 * node + (orientation as usize)) as u32
}

// Decodes an oriented node.
fn decode(encoded: u32) -> (usize, Orientation) {
    let encoded = encoded as usize;
    let orientation = if encoded & 1 == 0 { Orientation::Forward } else { Orientation::Reverse };
    (encoded / 2, orientation)
}

// Reverses the walk: the nodes come in the opposite order, each in the opposite orientation.
fn reverse(nodes: &mut [u32]) {
    nodes.reverse();
    for node in nodes.iter_mut() {
        *node ^= 1;
    }
}

//-----------------------------------------------------------------------------

// Expands a mapping between unitigs into a correspondence between paths of nodes.
pub(crate) fn expand(first: &Unitigs, second: &Unitigs, mapping: &NodeMapping) -> Translation {
    let mut result = Translation {
        first: Vec::new(),
        first_offsets: vec![0],
        second: Vec::new(),
        second_offsets: vec![0],
    };

    for unitig in 0..first.len() {
        let (image, orientation) = mapping.get(unitig);
        let here = result.first.len();
        let there = result.second.len();
        result.first.extend(first.pieces(unitig).map(|piece| encode(piece.node, piece.orientation)));
        result.second.extend(
            second.pieces(image).map(|piece| encode(piece.node, piece.orientation))
        );

        // The image unitig is stored in the opposite direction, so its walk must be reversed to
        // spell the same sequence as the walk in the first graph.
        if orientation == Orientation::Reverse {
            reverse(&mut result.second[there..]);
        }
        // Both walks spell the same sequence, so the pair can be flipped as a whole. Do that when
        // it puts the first node of the first walk in forward orientation.
        let walk = &result.first[here..];
        let ends_in_reverse = decode(walk[walk.len() - 1]).1 == Orientation::Reverse;
        if decode(walk[0]).1 == Orientation::Reverse && ends_in_reverse {
            reverse(&mut result.first[here..]);
            reverse(&mut result.second[there..]);
        }

        result.first_offsets.push(result.first.len() as u32);
        result.second_offsets.push(result.second.len() as u32);
    }

    result
}

//-----------------------------------------------------------------------------

/// Verifies that the translation is a correspondence between the two graphs.
///
/// This checks that every walk is a path in the right graph, that the two walks of a pair spell the
/// same sequence, and that the walks visit every node of both graphs exactly once. It runs in
/// linear time and does not use any hash values.
pub fn verify_translation<A: Topology, B: Topology>(
    first: &A, second: &B, translation: &Translation
) -> Result<(), Mismatch> {
    let mut first_visits = vec![0u32; first.nodes()];
    let mut second_visits = vec![0u32; second.nodes()];
    let mut here: Vec<u8> = Vec::new();
    let mut there: Vec<u8> = Vec::new();

    for index in 0..translation.len() {
        here.clear();
        there.clear();
        check_walk(first, translation.first_walk(index), &mut first_visits, &mut here)?;
        check_walk(second, translation.second_walk(index), &mut second_visits, &mut there)?;
        if here != there {
            return Err(Mismatch::Structure);
        }
    }

    // Every node must be visited exactly once, which also covers the number of nodes.
    if first_visits.iter().any(|&count| count != 1) || second_visits.iter().any(|&count| count != 1) {
        return Err(Mismatch::NodeCount);
    }

    Ok(())
}

// Checks that the walk is a path in the graph, counting the visits and building the sequence.
fn check_walk<T: Topology>(
    graph: &T, walk: impl Iterator<Item = (usize, Orientation)>,
    visits: &mut [u32], sequence: &mut Vec<u8>
) -> Result<(), Mismatch> {
    let mut previous: Option<(usize, Orientation)> = None;
    for (node, orientation) in walk {
        if node >= visits.len() {
            return Err(Mismatch::NodeCount);
        }
        visits[node] += 1;
        // Consecutive nodes must be joined by an edge.
        if let Some((from, from_orientation)) = previous {
            let exists = graph.neighbors(from, support::exit_side(from_orientation))
                .any(|(next, side)| next == node && side == support::entry_side(orientation));
            if !exists {
                return Err(Mismatch::Structure);
            }
        }
        unitigs::append_piece(graph, node, orientation, sequence);
        previous = Some((node, orientation));
    }

    Ok(())
}

//-----------------------------------------------------------------------------
