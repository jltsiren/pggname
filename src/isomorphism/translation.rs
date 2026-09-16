//! Expanding a unitig mapping into a correspondence between nodes.
//!
//! An isomorphism between the unitig graphs of two graphs matches the sequences of the unitigs
//! position by position. Each graph cuts its unitigs into nodes in its own way, so overlaying the
//! two sets of cuts gives a correspondence between intervals of nodes.

use crate::topology::Topology;
use crate::unitigs::Unitigs;

use super::{Mismatch, NodeMapping, Options, hashing};

use gbz::Orientation;

//-----------------------------------------------------------------------------

/// An interval of a node of the first graph and the interval of the second graph it corresponds to.
///
/// The intervals have the same length. If the orientation is [`Orientation::Forward`], the two
/// sequences are the same. Otherwise the second is the reverse complement of the first.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Part {
    /// Start of the interval in the node of the first graph.
    pub from: usize,
    /// Node in the second graph.
    pub node: usize,
    /// Start of the interval in the node of the second graph.
    pub to: usize,
    /// Length of the interval.
    pub len: usize,
    /// Orientation of the second interval relative to the first.
    pub orientation: Orientation,
}

//-----------------------------------------------------------------------------

/// A correspondence between the nodes of two graphs.
///
/// Every node of the first graph is covered by one or more [`Part`]s, in increasing order of
/// [`Part::from`]. Together they tile the sequence of the node, and the intervals they point to
/// tile the sequences of the second graph.
///
/// Unlike a [`NodeMapping`], this is not a bijection between nodes. A node of one graph may
/// correspond to a part of a node of the other, or to several nodes in a row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Translation {
    // The parts of node `i` are `offsets[i]..offsets[i + 1]`.
    offsets: Vec<u32>,
    parts: Vec<Part>,
}

impl Translation {
    /// Returns the number of nodes in the first graph.
    pub fn len(&self) -> usize {
        self.offsets.len() - 1
    }

    /// Returns `true` if the first graph has no nodes.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the parts covering the given node of the first graph.
    pub fn parts(&self, node: usize) -> &[Part] {
        &self.parts[self.offsets[node] as usize..self.offsets[node + 1] as usize]
    }

    /// Returns the total number of parts.
    pub fn part_count(&self) -> usize {
        self.parts.len()
    }

    /// Returns `true` if no part reverses the orientation.
    pub fn is_forward(&self) -> bool {
        self.parts.iter().all(|part| part.orientation == Orientation::Forward)
    }

    /// Returns `true` if the correspondence is a bijection between nodes.
    ///
    /// This holds when neither graph splits a node of the other, which is the case when the two
    /// graphs are already isomorphic at the level of nodes.
    pub fn is_node_mapping(&self) -> bool {
        self.parts.iter().enumerate().all(|(index, part)| {
            part.from == 0 && part.to == 0 && self.offsets[index + 1] - self.offsets[index] == 1
        })
    }
}

//-----------------------------------------------------------------------------

// Expands a mapping between unitigs into a correspondence between nodes.
pub(crate) fn expand(first: &Unitigs, second: &Unitigs, mapping: &NodeMapping) -> Translation {
    let nodes = first.node_count();
    let mut offsets: Vec<u32> = Vec::with_capacity(nodes + 1);
    let mut parts: Vec<Part> = Vec::new();
    offsets.push(0);

    let mut buffer: Vec<Part> = Vec::new();
    for node in 0..nodes {
        buffer.clear();
        let piece = first.piece_of(node);
        let unitig = first.unitig_of(node);
        let (image, unitig_orientation) = mapping.get(unitig);
        let len = first.graph().sequence_len(unitig);
        let flipped = unitig_orientation == Orientation::Reverse;

        // The interval of the piece, in the coordinates of the image unitig.
        let (start, end) = if flipped {
            (len - piece.end, len - piece.start)
        } else {
            (piece.start, piece.end)
        };

        for other in second.pieces_in(image, start, end) {
            let overlap_start = start.max(other.start);
            let overlap_end = end.min(other.end);
            if overlap_start >= overlap_end {
                continue;
            }

            // Whether each node is read in the same direction as the first unitig.
            let forward_here = piece.orientation == Orientation::Forward;
            let forward_there = (other.orientation == Orientation::Forward) != flipped;

            // The overlap in the coordinates of the first unitig.
            let (here_start, here_end) = if flipped {
                (len - overlap_end, len - overlap_start)
            } else {
                (overlap_start, overlap_end)
            };

            buffer.push(Part {
                from: if forward_here { here_start - piece.start } else { piece.end - here_end },
                node: other.node,
                to: if forward_there { overlap_start - other.start } else { other.end - overlap_end },
                len: overlap_end - overlap_start,
                orientation: if forward_here == forward_there {
                    Orientation::Forward
                } else {
                    Orientation::Reverse
                },
            });
        }

        buffer.sort_unstable_by_key(|part| part.from);
        parts.extend_from_slice(&buffer);
        offsets.push(parts.len() as u32);
    }

    Translation { offsets, parts }
}

//-----------------------------------------------------------------------------

/// Verifies that the translation is a correspondence between the two graphs.
///
/// This checks that the parts tile the sequences of both graphs and that the corresponding
/// sequences match. It runs in linear time and does not use any hash values.
pub fn verify_translation<A: Topology, B: Topology>(
    first: &A, second: &B, translation: &Translation, options: &Options
) -> Result<(), Mismatch> {
    if translation.len() != first.nodes() {
        return Err(Mismatch::NodeCount);
    }

    // Intervals of the second graph, to be checked for tiling afterwards.
    let mut covered: Vec<(usize, usize, usize)> = Vec::with_capacity(translation.part_count());

    for node in 0..first.nodes() {
        let sequence = first.sequence(node);
        let mut offset = 0;
        for part in translation.parts(node).iter() {
            if part.orientation == Orientation::Reverse && !options.allow_flips {
                return Err(Mismatch::Structure);
            }
            // The parts must tile the node from start to end, without gaps or overlaps.
            if part.from != offset {
                return Err(Mismatch::Structure);
            }
            offset += part.len;
            if offset > sequence.len() {
                return Err(Mismatch::Structure);
            }

            let image = second.sequence(part.node);
            if part.to + part.len > image.len() {
                return Err(Mismatch::Structure);
            }
            let here = &sequence[part.from..part.from + part.len];
            let there = &image[part.to..part.to + part.len];
            let matches = match part.orientation {
                Orientation::Forward => here == there,
                Orientation::Reverse => hashing::equals_reverse_complement(here, there),
            };
            if !matches {
                return Err(Mismatch::Structure);
            }

            covered.push((part.node, part.to, part.len));
        }
        if offset != sequence.len() {
            return Err(Mismatch::Structure);
        }
    }

    // Every position of the second graph must be covered exactly once.
    covered.sort_unstable();
    let mut expected = 0;
    let mut previous = usize::MAX;
    for (node, start, len) in covered {
        if node != previous {
            // Finish the previous node before moving on.
            if previous != usize::MAX && expected != second.sequence_len(previous) {
                return Err(Mismatch::Structure);
            }
            previous = node;
            expected = 0;
        }
        if start != expected {
            return Err(Mismatch::Structure);
        }
        expected += len;
    }
    if previous != usize::MAX && expected != second.sequence_len(previous) {
        return Err(Mismatch::Structure);
    }

    // Nodes of the second graph that were never covered.
    let total: usize = (0..second.nodes()).map(|node| second.sequence_len(node)).sum();
    let mapped: usize = (0..first.nodes()).map(|node| first.sequence_len(node)).sum();
    if total != mapped {
        return Err(Mismatch::SequenceLength);
    }

    Ok(())
}

//-----------------------------------------------------------------------------
