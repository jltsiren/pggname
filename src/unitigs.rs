//! Unitig graphs, where each maximal non-branching path in the underlying graph is a single node.
//!
//! Pangenome graph implementations may use 1 bp nodes or maximal nodes or chop long nodes into smaller pieces to keep the length manageable.
//! Since all three approaches correspond to the same alignment of the underlying sequences, the graph should be considered isomorphic.
//! [`Unitigs`] normalizes a graph into one with maximal nodes and maintains a mapping from its nodes to paths in the underlying graph.
//! [`crate::isomorphism::are_isomorphic_unitigs`] uses it for determining if two graphs are isomorphic at the unitig level.
//!
//! The current implementation cannot handle graph components that consist of a cycle with no branches.
//! Any sequence position in a cycle is a potential starting point for the unitig.
//! But even if two graphs contain the same sequence as a cycle, there may not be any sequence position that starts a node in both graphs.

use crate::isomorphism::hashing;
use crate::topology::{IndexedGraph, Topology};

use gbz::{NodeSide, Orientation};
use gbz::support;

#[cfg(test)]
mod tests;

//-----------------------------------------------------------------------------

/// A normalized graph with maximal non-branching paths as nodes.
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
    // The graph itself.
    graph: IndexedGraph,
    // Starting offset of each path, with a sentinel at the end.
    offsets: Vec<u32>,
    // Concatenated paths for each unitig, using GBWT encoding.
    paths: Vec<u32>,
}

impl Unitigs {
    // Sentinel for a node that has not been assigned to a unitig.
    const NONE: u32 = u32::MAX;

    /// Decomposes the graph into maximal non-branching paths.
    ///
    /// Returns an error if the graph has a connected component that is a cycle with no branches.
    pub fn new<T: Topology>(graph: &T) -> Result<Self, String> {
        let nodes = graph.nodes();
        let mut node_to_unitig = vec![Self::NONE; nodes];
        let mut offsets: Vec<u32> = vec![0];
        let mut paths: Vec<u32> = Vec::new();

        for node in 0..nodes {
            for side in [NodeSide::Left, NodeSide::Right] {
                if node_to_unitig[node] != Self::NONE || Self::unitig_continues_to(graph, node, side).is_some() {
                    // Already assigned or the unitig continues from this side.
                    continue;
                }
                let unitig = (offsets.len() - 1) as u32;
                // We are at the start of a unitig that continues to the other side.
                let (mut curr_node, mut curr_side) = (node, side);
                loop {
                    node_to_unitig[curr_node] = unitig;
                    paths.push(crate::encode_oriented_node(curr_node, support::entry_orientation(curr_side)));
                    match Self::unitig_continues_to(graph, curr_node, curr_side.flip()) {
                        Some((next_node, next_side)) => { curr_node = next_node; curr_side = next_side; },
                        None => break,
                    }
                }
                offsets.push(paths.len() as u32);
            }
        }

        if let Some(node) = node_to_unitig.iter().position(|&unitig| unitig == Self::NONE) {
            return Err(format!(
                "Node {} is in a connected component that is a cycle with no branches",
                String::from_utf8_lossy(&graph.node_name(node))
            ));
        }

        let mut result = Unitigs {
            graph: IndexedGraph::new(),
            offsets,
            paths,
        };
        result.canonicalize();
        result.build_graph(graph, &node_to_unitig);

        Ok(result)
    }

    // Returns the successor of the given node side in the given graph, if it is unique.
    // Returns the node side the unitig continues to from the given side.
    fn unitig_continues_to<T: Topology>(graph: &T, node: usize, side: NodeSide) -> Option<(usize, NodeSide)> {
        if graph.degree(node, side) != 1 {
            return None;
        }
        let (next_node, next_side) = graph.neighbors(node, side).next()?;
        if next_node == node || graph.degree(next_node, next_side) != 1 {
            return None;
        }
        Some((next_node, next_side))
    }

    /// Returns the number of unitigs.
    pub fn len(&self) -> usize {
        self.offsets.len() - 1
    }

    /// Returns `true` if there are no unitigs.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the graph with one node per unitig.
    pub fn graph(&self) -> &IndexedGraph {
        &self.graph
    }

    /// Returns the path corresponding to the given unitig.
    ///
    /// The path uses GBWT encoding for oriented nodes.
    /// It is given in the canonical orientation (see [`support::path_is_canonical`]).
    pub fn unitig(&self, index: usize) -> impl ExactSizeIterator<Item = u32> {
        let range = self.offsets[index] as usize..self.offsets[index + 1] as usize;
        self.paths[range].iter().copied()
    }

    // Canonicalizes the orientation of each unitig.
    fn canonicalize(&mut self) {
        for unitig in 0..self.len() {
            let range = self.offsets[unitig] as usize..self.offsets[unitig + 1] as usize;
            let path = &mut self.paths[range];
            if !crate::encoded_path_is_canonical(path) {
                crate::reverse_encoded_path(path);
            }
        }
    }

    // Builds the unitig graph from the stored unitigs and the given underlying graph.
    fn build_graph<T: Topology>(&mut self, graph: &T, node_to_unitig: &[u32]) {
        let mut sequence: Vec<u8> = Vec::new();
        for index in 0..self.len() {
            sequence.clear();
            for encoded in self.unitig(index) { 
                let (node, orientation) = crate::decode_oriented_node(encoded);
                append_sequence(graph, node, orientation, &mut sequence);
            }
            // The unitig is named after the first node in it, which identifies it uniquely.
            let name = graph.node_name(crate::decode_oriented_node(self.paths[self.offsets[index] as usize]).0);
            self.graph.add_node(&name, &sequence);
        }

        // Create edges connecting the ends of the unitigs.
        for unitig in 0..self.len() {
            for side in [NodeSide::Left, NodeSide::Right] {
                let (node, node_side) = self.end(unitig, side);
                for (neighbor, neighbor_side) in graph.neighbors(node, node_side) {
                    let other = node_to_unitig[neighbor] as usize;
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
        let index = match side {
            NodeSide::Left => self.offsets[unitig] as usize,
            NodeSide::Right => self.offsets[unitig + 1] as usize - 1,
        };
        let (node, orientation) = crate::decode_oriented_node(self.paths[index]);
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

// Appends the sequence of the node to the target in the given orientation.
pub(crate) fn append_sequence<T: Topology>(graph: &T, node: usize, orientation: Orientation, target: &mut Vec<u8>) {
    let sequence = graph.sequence(node);
    match orientation {
        Orientation::Forward => target.extend_from_slice(&sequence),
        Orientation::Reverse => target.extend(
            sequence.iter().rev().map(|&c| hashing::COMPLEMENT[c as usize])
        ),
    }
}

//-----------------------------------------------------------------------------
