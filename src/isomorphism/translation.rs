//! A mapping between the unitigs of two isomorphic graphs.

use crate::topology::Topology;
use crate::unitigs::{self, Unitigs};

use super::{Mismatch, NodeMapping};

use gbz::Orientation;
use gbz::support;

//-----------------------------------------------------------------------------

/// A translation between the unitigs of two isomorphic graphs.
///
/// Each unitig is represented as a sequence of oriented nodes.
/// The orientations of the unitigs are chosen so that the unitigs spell the same sequence in both graphs.
/// If possible, each unitig starts from a forward node in the first graph.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Translation {
    // Concatenated walks for the unitigs in the first graph, using GBWT handles for oriented nodes.
    first_unitigs: Vec<u32>,
    // The walk corresponding to unitig `i` is `first_unitigs[first_starts[i]..first_starts[i + 1]]`.
    first_starts: Vec<u32>,
    // Concatenated walks for the unitigs in the second graph, using GBWT handles for oriented nodes.
    second_unitigs: Vec<u32>,
    // The walk corresponding to unitig `i` is `second_unitigs[second_starts[i]..second_starts[i + 1]]`.
    second_starts: Vec<u32>,
}

impl Translation {
    /// Creates a translation between the unitigs of two isomorphic graphs.
    ///
    /// # Arguments
    ///
    /// * `first`: Unitigs of the first graph.
    /// * `second`: Unitigs of the second graph.
    /// * `mapping`: Mapping from unitigs in the first graph to oriented unitigs in the second graph.
    ///
    /// # Panics
    ///
    /// Assumes that there are `n` unitigs.
    /// Panics if `first.len() != n`, `second.len() != n`, or `mapping.len() != n`.
    pub fn new(first: &Unitigs, second: &Unitigs, mapping: &NodeMapping) -> Translation {
        assert_eq!(first.len(), second.len(), "The number of unitigs must match.");
        assert_eq!(mapping.len(), first.len(), "The mapping must cover all unitigs.");

        let mut result = Translation {
            first_unitigs: Vec::new(),
            first_starts: vec![0],
            second_unitigs: Vec::new(),
            second_starts: vec![0],
        };

        for first_id in 0..first.len() {
            let (second_id, orientation) = mapping.get(first_id);
            let old_first_len = result.first_unitigs.len();
            let old_second_len = result.second_unitigs.len();
            result.first_unitigs.extend(
                first.pieces(first_id).map(|piece| Self::encode(piece.node, piece.orientation))
            );
            result.second_unitigs.extend(
                second.pieces(second_id).map(|piece| Self::encode(piece.node, piece.orientation))
            );
            if orientation == Orientation::Reverse {
                Self::reverse(&mut result.second_unitigs[old_second_len..]);
            }

            // Try to ensure that we start from a forward node in the first graph.
            let walk = &result.first_unitigs[old_first_len..];
            let ends_in_reverse = Self::decode(walk[walk.len() - 1]).1 == Orientation::Reverse;
            if Self::decode(walk[0]).1 == Orientation::Reverse && ends_in_reverse {
                Self::reverse(&mut result.first_unitigs[old_first_len..]);
                Self::reverse(&mut result.second_unitigs[old_second_len..]);
            }

            // Store the start of the next unitig / a sentinel value for the end of the last unitig.
            result.first_starts.push(result.first_unitigs.len() as u32);
            result.second_starts.push(result.second_unitigs.len() as u32);
        }

        result
    }

    /// Returns the number of unitigs.
    pub fn len(&self) -> usize {
        self.first_starts.len() - 1
    }

    /// Returns `true` if there are no unitigs.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the given unitig in the first graph as `(node, orientation)` pairs.
    pub fn first_walk(&self, index: usize) -> impl ExactSizeIterator<Item = (usize, Orientation)> {
        Self::walk(&self.first_unitigs, &self.first_starts, index)
    }

    /// Returns the given unitig in the second graph as `(node, orientation)` pairs.
    pub fn second_walk(&self, index: usize) -> impl ExactSizeIterator<Item = (usize, Orientation)> {
        Self::walk(&self.second_unitigs, &self.second_starts, index)
    }

    // Returns the given walk from the encoded nodes and the offsets.
    fn walk(
        nodes: &[u32], offsets: &[u32], index: usize
    ) -> impl ExactSizeIterator<Item = (usize, Orientation)> {
        let range = offsets[index] as usize..offsets[index + 1] as usize;
        nodes[range].iter().map(|&encoded| Self::decode(encoded))
    }

    // A wrapper around `support::encode_node`.
    fn encode(node: usize, orientation: Orientation) -> u32 {
        support::encode_node(node, orientation) as u32
    }

    // A wrapper around `support::decode_node`.
    fn decode(encoded: u32) -> (usize, Orientation) {
        support::decode_node(encoded as usize)
    }

    // Reverses the walk.
    fn reverse(nodes: &mut [u32]) {
        nodes.reverse();
        for node in nodes.iter_mut() {
            *node = support::flip_node(*node as usize) as u32;
        }
    }
}

//-----------------------------------------------------------------------------

/// Verifies that the translation is a correspondence between the two graphs.
///
/// This checks that:
///
/// * Each walk is a valid non-branching path in its respective graph.
/// * The two walks of a pair spell the same sequence.
/// * Every node of both graphs is visited exactly once.
pub fn verify_translation<A: Topology, B: Topology>(
    first: &A, second: &B, translation: &Translation
) -> Result<(), Mismatch> {
    // Number of visits for each node in the first and second graphs.
    let mut first_visits = vec![0u32; first.nodes()];
    let mut second_visits = vec![0u32; second.nodes()];
    // Buffers for the sequences.
    let mut first_sequence: Vec<u8> = Vec::new();
    let mut second_sequence: Vec<u8> = Vec::new();

    for index in 0..translation.len() {
        check_unitig(first, translation.first_walk(index), &mut first_visits, &mut first_sequence)?;
        check_unitig(second, translation.second_walk(index), &mut second_visits, &mut second_sequence)?;
        if first_sequence != second_sequence {
            return Err(Mismatch::Structure);
        }
    }

    // Every node must be visited exactly once, which also covers the number of nodes.
    if first_visits.iter().any(|&count| count != 1) || second_visits.iter().any(|&count| count != 1) {
        return Err(Mismatch::NodeCount);
    }

    Ok(())
}

// Checks that the walk is a non-branching path in the graph.
// Updates the number of visits to each node and writes the sequence to the buffer.
fn check_unitig<T: Topology>(
    graph: &T, walk: impl Iterator<Item = (usize, Orientation)>,
    visits: &mut [u32], sequence: &mut Vec<u8>
) -> Result<(), Mismatch> {
    sequence.clear();
    let mut previous: Option<(usize, Orientation)> = None;
    for (node, orientation) in walk {
        if node >= visits.len() {
            return Err(Mismatch::NodeCount);
        }
        visits[node] += 1;
        if let Some((from, from_orientation)) = previous {
            let mut prev_successors_iter = graph.neighbors(from, support::exit_side(from_orientation));
            if prev_successors_iter.next() != Some((node, support::entry_side(orientation))) {
                return Err(Mismatch::Structure);
            }
            if prev_successors_iter.next().is_some() {
                return Err(Mismatch::Structure);
            }
            let mut predecessors_iter = graph.neighbors(node, support::entry_side(orientation));
            if predecessors_iter.next() != Some((from, support::exit_side(from_orientation))) {
                return Err(Mismatch::Structure);
            }
            if predecessors_iter.next().is_some() {
                return Err(Mismatch::Structure);
            }
        }
        unitigs::append_piece(graph, node, orientation, sequence);
        previous = Some((node, orientation));
    }

    Ok(())
}

//-----------------------------------------------------------------------------
