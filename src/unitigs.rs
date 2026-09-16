//! Maximal non-branching paths.
//!
//! Some tools chop long nodes into shorter fragments. The result is a different graph at the level
//! of nodes, but it represents the same pangenome. Collapsing each maximal non-branching path into
//! a single node removes the difference, because chopping a node only adds boundaries inside such a
//! path.
//!
//! In the node side model, a side is *linked* if it has exactly one neighbor, that neighbor has
//! exactly one neighbor in turn, and the two belong to different nodes. A unitig is then a maximal
//! path of linked sides. Entering a node through side `s` means reading it in orientation
//! [`support::entry_orientation(s)`](gbz::support::entry_orientation) and leaving through the other
//! side.
//!
//! # Canonical direction
//!
//! A unitig can be traversed from either end, giving sequences `S` and `revcomp(S)`. The direction
//! has to be chosen the same way in both graphs, and the only information that survives chopping is
//! the sequence itself. The unitig is therefore stored in the direction where the sequence is
//! lexicographically smaller.
//!
//! This is exact unless `S` equals its own reverse complement, in which case both directions give
//! the same sequence and the choice is arbitrary. The two graphs may then store such a unitig in
//! opposite directions. Because the unitig-level comparison allows flips, the mapping can still be
//! found; see [`crate::isomorphism::are_isomorphic_unitigs`] for what this means for
//! orientation-preserving comparisons.
//!
//! # Limitations
//!
//! A connected component where every node side is linked is a cycle with no branches. It has no
//! unitig end to start from, and a canonical starting point would have to be defined by the minimal
//! rotation of a circular sequence, which may fall in the middle of a node. Such components are
//! detected and reported as an error rather than handled incorrectly.

use crate::isomorphism::hashing;
use crate::topology::{IndexedGraph, Topology};

use gbz::{NodeSide, Orientation};
use gbz::support;

#[cfg(test)]
mod tests;

//-----------------------------------------------------------------------------

// Sentinel for a node that has not been assigned to a unitig.
const NONE: u32 = u32::MAX;

/// A decomposition of a graph into maximal non-branching paths.
///
/// # Examples
///
/// ```
/// use pggname::Topology;
/// use pggname::topology::IndexedGraph;
/// use pggname::unitigs::Unitigs;
/// use gbz::Orientation;
///
/// // A path of three nodes collapses into a single unitig.
/// let mut graph = IndexedGraph::new();
/// let a = graph.add_node(b"1", b"GAT");
/// let b = graph.add_node(b"2", b"TA");
/// let c = graph.add_node(b"3", b"CA");
/// graph.add_edge(a, Orientation::Forward, b, Orientation::Forward);
/// graph.add_edge(b, Orientation::Forward, c, Orientation::Forward);
/// graph.finalize();
///
/// let unitigs = Unitigs::new(&graph).unwrap();
/// assert_eq!(unitigs.len(), 1);
/// assert_eq!(unitigs.graph().sequence(0).as_ref(), b"GATTACA");
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unitigs {
    // One node per unitig.
    graph: IndexedGraph,
    // The pieces of unitig `i` are `offsets[i]..offsets[i + 1]`.
    offsets: Vec<u32>,
    // Each piece as `2 * node + orientation` in the original graph.
    pieces: Vec<u32>,
    // Interval covered by each piece within the sequence of its unitig.
    starts: Vec<u32>,
    ends: Vec<u32>,
    // Unitig containing each node of the original graph.
    unitig_of: Vec<u32>,
    // Piece containing each node of the original graph.
    piece_of: Vec<u32>,
}

impl Unitigs {
    /// Decomposes the graph into maximal non-branching paths.
    ///
    /// Returns an error if the graph has a connected component that is a cycle with no branches.
    pub fn new<T: Topology>(graph: &T) -> Result<Self, String> {
        let nodes = graph.nodes();
        let mut unitig_of = vec![NONE; nodes];
        let mut pieces: Vec<u32> = Vec::new();
        let mut offsets: Vec<u32> = vec![0];

        // Every unitig has two ends, and an end is a side that is not linked. Starting from one
        // end reaches the other, so each unitig is found exactly once.
        for node in 0..nodes {
            for side in [NodeSide::Left, NodeSide::Right] {
                if unitig_of[node] != NONE || linked(graph, node, side).is_some() {
                    continue;
                }
                let unitig = (offsets.len() - 1) as u32;
                let (mut current, mut entry) = (node, side);
                loop {
                    unitig_of[current] = unitig;
                    pieces.push(encode_piece(current, support::entry_orientation(entry)));
                    match linked(graph, current, entry.flip()) {
                        Some((next, next_entry)) => { current = next; entry = next_entry; },
                        None => break,
                    }
                }
                offsets.push(pieces.len() as u32);
            }
        }

        if let Some(node) = unitig_of.iter().position(|&unitig| unitig == NONE) {
            return Err(format!(
                "Node {} is in a connected component that is a cycle with no branches, which is not supported",
                String::from_utf8_lossy(&graph.node_name(node))
            ));
        }

        let mut result = Unitigs {
            graph: IndexedGraph::new(),
            offsets, pieces,
            starts: Vec::new(),
            ends: Vec::new(),
            unitig_of,
            piece_of: vec![NONE; nodes],
        };
        result.canonicalize(graph);
        result.build_graph(graph);

        Ok(result)
    }

    /// Returns the number of unitigs.
    pub fn len(&self) -> usize {
        self.offsets.len() - 1
    }

    /// Returns `true` if there are no unitigs.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the number of nodes in the original graph.
    pub fn node_count(&self) -> usize {
        self.unitig_of.len()
    }

    /// Returns the graph with one node per unitig.
    pub fn graph(&self) -> &IndexedGraph {
        &self.graph
    }

    /// Returns the unitig containing the given node of the original graph.
    pub fn unitig_of(&self, node: usize) -> usize {
        self.unitig_of[node] as usize
    }

    /// Returns the pieces of the given unitig.
    ///
    /// Each piece is a node of the original graph, the orientation it is read in, and the interval
    /// it covers in the sequence of the unitig.
    pub fn pieces(&self, unitig: usize) -> impl ExactSizeIterator<Item = Piece> {
        let range = self.offsets[unitig] as usize..self.offsets[unitig + 1] as usize;
        range.map(move |piece| self.piece(piece))
    }

    /// Returns the pieces of the unitig that overlap the given interval of its sequence.
    pub fn pieces_in(&self, unitig: usize, start: usize, end: usize) -> impl Iterator<Item = Piece> {
        let unitig_end = self.offsets[unitig + 1] as usize;
        let first = if start >= end { unitig_end } else { self.piece_index_at(unitig, start) };
        (first..unitig_end)
            .map(move |piece| self.piece(piece))
            .take_while(move |piece| piece.start < end)
    }

    // Returns the index of the last piece of the unitig that starts at or before the offset.
    fn piece_index_at(&self, unitig: usize, offset: usize) -> usize {
        let range = self.offsets[unitig] as usize..self.offsets[unitig + 1] as usize;
        let found = self.starts[range.clone()].partition_point(|&start| (start as usize) <= offset);
        range.start + found.saturating_sub(1)
    }

    /// Returns the piece containing the given node of the original graph.
    pub fn piece_of(&self, node: usize) -> Piece {
        self.piece(self.piece_of[node] as usize)
    }

    // Returns the given piece.
    fn piece(&self, piece: usize) -> Piece {
        let (node, orientation) = decode_piece(self.pieces[piece]);
        Piece {
            node, orientation,
            start: self.starts[piece] as usize,
            end: self.ends[piece] as usize,
        }
    }

    // Stores each unitig in the direction where its sequence is lexicographically smaller.
    fn canonicalize<T: Topology>(&mut self, graph: &T) {
        let mut sequence: Vec<u8> = Vec::new();
        for unitig in 0..self.len() {
            let range = self.offsets[unitig] as usize..self.offsets[unitig + 1] as usize;
            sequence.clear();
            for &piece in self.pieces[range.clone()].iter() {
                let (node, orientation) = decode_piece(piece);
                append_piece(graph, node, orientation, &mut sequence);
            }
            if hashing::compare_to_reverse_complement(&sequence) == std::cmp::Ordering::Greater {
                // Reversing the walk reverses the order of the pieces and flips each orientation.
                self.pieces[range.clone()].reverse();
                for piece in self.pieces[range].iter_mut() {
                    *piece ^= 1;
                }
            }
        }
    }

    // Builds the compacted graph and the offsets of the pieces.
    fn build_graph<T: Topology>(&mut self, graph: &T) {
        let mut sequence: Vec<u8> = Vec::new();
        self.starts = vec![0; self.pieces.len()];
        self.ends = vec![0; self.pieces.len()];

        for unitig in 0..self.len() {
            sequence.clear();
            for piece in self.offsets[unitig] as usize..self.offsets[unitig + 1] as usize {
                let (node, orientation) = decode_piece(self.pieces[piece]);
                self.starts[piece] = sequence.len() as u32;
                self.piece_of[node] = piece as u32;
                append_piece(graph, node, orientation, &mut sequence);
                self.ends[piece] = sequence.len() as u32;
            }
            // The unitig is named after the first node in it, which identifies it uniquely.
            let name = graph.node_name(decode_piece(self.pieces[self.offsets[unitig] as usize]).0);
            self.graph.add_node(&name, &sequence);
        }

        // An end of a unitig is a side that is not linked, and the neighbors of such a side are
        // ends as well.
        for unitig in 0..self.len() {
            for side in [NodeSide::Left, NodeSide::Right] {
                let (node, node_side) = self.end(unitig, side);
                for (neighbor, neighbor_side) in graph.neighbors(node, node_side) {
                    let other = self.unitig_of(neighbor);
                    let other_side = self.end_side(other, neighbor, neighbor_side);
                    self.graph.add_edge(
                        unitig, support::exit_orientation(side),
                        other, support::entry_orientation(other_side)
                    );
                }
            }
        }
        self.graph.finalize();
    }

    // Returns the node side of the original graph at the given end of the unitig.
    fn end(&self, unitig: usize, side: NodeSide) -> (usize, NodeSide) {
        let piece = match side {
            NodeSide::Left => self.offsets[unitig] as usize,
            NodeSide::Right => self.offsets[unitig + 1] as usize - 1,
        };
        let (node, orientation) = decode_piece(self.pieces[piece]);
        let node_side = match side {
            NodeSide::Left => support::entry_side(orientation),
            NodeSide::Right => support::exit_side(orientation),
        };
        (node, node_side)
    }

    // Returns which end of the unitig the given node side is.
    fn end_side(&self, unitig: usize, node: usize, side: NodeSide) -> NodeSide {
        if self.end(unitig, NodeSide::Left) == (node, side) { NodeSide::Left } else { NodeSide::Right }
    }
}

//-----------------------------------------------------------------------------

/// A node of the original graph within a unitig.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Piece {
    /// Node in the original graph.
    pub node: usize,
    /// Orientation the node is read in.
    pub orientation: Orientation,
    /// Start of the piece in the sequence of the unitig.
    pub start: usize,
    /// End of the piece in the sequence of the unitig.
    pub end: usize,
}

impl Piece {
    /// Returns the length of the piece.
    pub fn len(&self) -> usize {
        self.end - self.start
    }

    /// Returns `true` if the piece is empty.
    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }
}

//-----------------------------------------------------------------------------

// Returns the neighbor of the given side, if the side is part of a non-branching path.
//
// Self-loops are excluded, so that a node cannot be linked to itself.
fn linked<T: Topology>(graph: &T, node: usize, side: NodeSide) -> Option<(usize, NodeSide)> {
    if graph.degree(node, side) != 1 {
        return None;
    }
    let (neighbor, neighbor_side) = graph.neighbors(node, side).next()?;
    if neighbor == node || graph.degree(neighbor, neighbor_side) != 1 {
        return None;
    }
    Some((neighbor, neighbor_side))
}

// Appends the sequence of the node, read in the given orientation.
fn append_piece<T: Topology>(graph: &T, node: usize, orientation: Orientation, target: &mut Vec<u8>) {
    let sequence = graph.sequence(node);
    match orientation {
        Orientation::Forward => target.extend_from_slice(&sequence),
        Orientation::Reverse => target.extend(
            sequence.iter().rev().map(|&c| hashing::COMPLEMENT[c as usize])
        ),
    }
}

// Encodes a piece as `2 * node + orientation`.
fn encode_piece(node: usize, orientation: Orientation) -> u32 {
    (2 * node + (orientation as usize)) as u32
}

// Decodes a piece.
fn decode_piece(encoded: u32) -> (usize, Orientation) {
    let encoded = encoded as usize;
    let orientation = if encoded & 1 == 0 { Orientation::Forward } else { Orientation::Reverse };
    (encoded / 2, orientation)
}

//-----------------------------------------------------------------------------
