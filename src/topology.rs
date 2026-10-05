//! Graph structures for isomorphism algorithms.
//!
//! To simplify the algorithms, we refer to the nodes using integers `0..nodes`, regardless of their identifiers.
//! The underlying graph could be [`GBZ`], a GFA graph, or a unitig graph derived from either.
//!
//! The primary interface is trait [`Topology`].
//! We implement it for [`IndexedGraph`] (a graph built from a GFA file) and [`GbzTopology`] (a wrapper over [`GBZ`]).
//! See [`crate::unitigs`] for the unitig graph and [`crate::graph`] for the trait used in stable graph name computation.

use crate::MAX_NODES;
use crate::graph::{GraphInt, GraphStr};

use gbz::{GBZ, NodeSide, Orientation};
use gbz::support;

use simple_sds::bit_vector::BitVector;
use simple_sds::ops::{Rank, Select};

use std::borrow::Cow;
use std::collections::HashMap;

#[cfg(test)]
mod tests;

//-----------------------------------------------------------------------------

/// A bidirected sequence graph with edges connecting node sides.
///
/// Nodes are identified by dense indexes in `0..self.nodes()`.
/// The graph may contain at most [`MAX_NODES`] nodes.
/// Methods taking a node index panic if the index is out of bounds.
///
/// # Contract
///
/// Implementations must satisfy the following:
///
/// * Each node has a distinct name.
/// * Neighbor lists are symmetric and contain no duplicates.
/// * Neighbor lists contain no duplicates.
/// * [`Topology::degree`] agrees with the number of items yielded by [`Topology::neighbors`].
/// * The results are stable for the lifetime of the object.
pub trait Topology {
    /// Returns the number of nodes in the graph.
    fn nodes(&self) -> usize;

    /// Returns the number of distinct edges in the graph.
    ///
    /// Note that this is not `sum_of_degrees / 2`, as there can be self-loops connecting a node side to itself.
    fn edges(&self) -> usize;

    /// Returns the sequence of the node with the given index.
    fn sequence(&self, node: usize) -> Cow<'_, [u8]>;

    /// Returns the length of the sequence of the node with the given index.
    fn sequence_len(&self, node: usize) -> usize;

    /// Returns the number of edges adjacent to the given side of the given node.
    fn degree(&self, node: usize, side: NodeSide) -> usize;

    /// Returns an iterator over the neighbors of the given side of the given node.
    ///
    /// The iterator yields the index of the neighboring node and the side the edge is adjacent to.
    /// The order is unspecified.
    fn neighbors(&self, node: usize, side: NodeSide) -> impl Iterator<Item = (usize, NodeSide)>;

    /// Returns the name (original identifier) of the node with the given index.
    fn node_name(&self, node: usize) -> Vec<u8>;

    // FIXME: This should return a struct.
    /// Returns the number of nodes, the number of edges, and total sequence length in the graph.
    ///
    /// These must match [`Graph::statistics`](crate::Graph::statistics) for the same graph.
    fn statistics(&self) -> (usize, usize, usize) {
        let nodes = self.nodes();
        let mut seq_len = 0;
        for node in 0..nodes {
            seq_len += self.sequence_len(node);
        }
        (nodes, self.edges(), seq_len)
    }
}

//-----------------------------------------------------------------------------

/// Returns `true` if the first graph is a subgraph of the second graph.
///
/// Two nodes are considered the same if they have the same name and the same sequence.
/// Node indexes are technical artifacts of the given [`Topology`].
/// Note that this also accepts the case where the graphs are identical.
///
/// # Examples
///
/// ```
/// use pggname::topology::{self, IndexedGraph};
/// use gbz::Orientation;
///
/// let mut large = IndexedGraph::new();
/// let a = large.add_node(b"1", b"GAT");
/// let b = large.add_node(b"2", b"TA");
/// large.add_edge(a, Orientation::Forward, b, Orientation::Forward);
/// large.finalize();
///
/// // The same graph without node 2.
/// let mut small = IndexedGraph::new();
/// small.add_node(b"1", b"GAT");
/// small.finalize();
///
/// assert!(topology::is_subgraph(&small, &large));
/// assert!(!topology::is_subgraph(&large, &small));
/// // Every graph is a subgraph of itself.
/// assert!(topology::is_subgraph(&large, &large));
/// ```
pub fn is_subgraph<A: Topology, B: Topology>(subgraph: &A, supergraph: &B) -> bool {
    if subgraph.nodes() > supergraph.nodes() || subgraph.edges() > supergraph.edges() {
        return false;
    }

    // Create a mapping from node indexes in the subgraph to their indexes in the supergraph.
    let mut index_in_subgraph: HashMap<Vec<u8>, usize> = HashMap::with_capacity(subgraph.nodes());
    for node in 0..subgraph.nodes() {
        index_in_subgraph.insert(subgraph.node_name(node), node);
    }
    let mut subgraph_to_supergraph = vec![usize::MAX; subgraph.nodes()];
    let mut matched = 0;
    for node in 0..supergraph.nodes() {
        if let Some(&source) = index_in_subgraph.get(&supergraph.node_name(node)) && subgraph_to_supergraph[source] == usize::MAX {
            subgraph_to_supergraph[source] = node;
            matched += 1;
        }
    }
    if matched != subgraph.nodes() {
        return false;
    }

    // Check that the sequences match.
    for (node, &target) in subgraph_to_supergraph.iter().enumerate() {
        if subgraph.sequence(node).as_ref() != supergraph.sequence(target).as_ref() {
            return false;
        }
    }

    // Check that all subgraph edges exist in the supergraph.
    let mut supergraph_edges: Vec<(usize, NodeSide)> = Vec::new();
    for node in 0..subgraph.nodes() {
        for side in [NodeSide::Left, NodeSide::Right] {
            supergraph_edges.clear();
            supergraph_edges.extend(supergraph.neighbors(subgraph_to_supergraph[node], side));
            supergraph_edges.sort_unstable();
            for (neighbor, neighbor_side) in subgraph.neighbors(node, side) {
                if supergraph_edges.binary_search(&(subgraph_to_supergraph[neighbor], neighbor_side)).is_err() {
                    return false;
                }
            }
        }
    }

    true
}

// Returns the unordered pair of encoded node sides corresponding to the given edge.
//
// The sides are returned in increasing order, which makes the pair canonical.
fn side_pair(
    from: usize, from_o: Orientation, to: usize, to_o: Orientation
) -> (usize, usize) {
    let source = support::encode_node_side(from, support::exit_side(from_o));
    let dest = support::encode_node_side(to, support::entry_side(to_o));
    if source <= dest { (source, dest) } else { (dest, source) }
}

//-----------------------------------------------------------------------------

/// A graph that can be built incrementally and queried using the [`Topology`] trait.
///
/// # Examples
///
/// ```
/// use pggname::Topology;
/// use pggname::topology::IndexedGraph;
/// use gbz::{NodeSide, Orientation};
///
/// let mut graph = IndexedGraph::new();
/// let a = graph.add_node(b"11", b"GAT");
/// let b = graph.add_node(b"12", b"TA");
/// graph.add_edge(a, Orientation::Forward, b, Orientation::Forward);
/// graph.finalize();
///
/// assert_eq!(graph.statistics(), (2, 1, 5));
/// assert_eq!(graph.degree(a, NodeSide::Right), 1);
/// assert_eq!(graph.degree(a, NodeSide::Left), 0);
/// let neighbors: Vec<_> = graph.neighbors(a, NodeSide::Right).collect();
/// assert_eq!(neighbors, vec![(b, NodeSide::Left)]);
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexedGraph {
    // Concatenated node names.
    names: Vec<u8>,
    // Starting offset of each node name, with a sentinel at the end.
    name_offsets: Vec<usize>,
    // Concatenated sequences.
    sequences: Vec<u8>,
    // Starting offset of each sequence, with a sentinel at the end.
    sequence_offsets: Vec<usize>,
    // Concatenated adjacency lists for each node side, using the GBWT encoding.
    neighbors: Vec<u32>,
    // Starting offset of each adjacency list, with a sentinel at the end.
    edge_offsets: Vec<usize>,
    // Canonical edges as unordered pairs of encoded node sides, before finalization.
    pending: Vec<(u32, u32)>,
    edge_count: usize,
}

impl IndexedGraph {
    /// Creates a new empty graph.
    pub fn new() -> Self {
        IndexedGraph {
            names: Vec::new(),
            name_offsets: vec![0],
            sequences: Vec::new(),
            sequence_offsets: vec![0],
            neighbors: Vec::new(),
            edge_offsets: vec![0],
            pending: Vec::new(),
            edge_count: 0,
        }
    }

    /// Adds a node to the graph and returns its index.
    ///
    /// This does not check for duplicate node names.
    pub fn add_node(&mut self, name: &[u8], sequence: &[u8]) -> usize {
        let index = self.nodes();
        self.names.extend_from_slice(name);
        self.name_offsets.push(self.names.len());
        self.sequences.extend_from_slice(sequence);
        self.sequence_offsets.push(self.sequences.len());
        index
    }

    /// Adds an edge between the given nodes.
    ///
    /// The nodes must already exist. Duplicate edges are ignored.
    /// Adjacency information is not available until [`IndexedGraph::finalize`] is called.
    pub fn add_edge(&mut self, from: usize, from_o: Orientation, to: usize, to_o: Orientation) {
        let (source, dest) = side_pair(from, from_o, to, to_o);
        self.pending.push((source as u32, dest as u32));
    }

    /// Sorts and deduplicates the edges and builds the adjacency information.
    ///
    /// Discards all edges from previous calls.
    pub fn finalize(&mut self) {
        self.pending.sort_unstable();
        self.pending.dedup();
        self.edge_count = self.pending.len();

        // Count the degree of each node side. A side self-loop is counted once.
        let sides = 2 * self.nodes();
        self.edge_offsets = vec![0; sides + 1];
        for &(source, dest) in self.pending.iter() {
            self.edge_offsets[source as usize + 1] += 1;
            if source != dest {
                self.edge_offsets[dest as usize + 1] += 1;
            }
        }
        for i in 0..sides {
            self.edge_offsets[i + 1] += self.edge_offsets[i];
        }

        // Fill in the neighbors.
        let mut next = self.edge_offsets.clone(); // Next available position for an edge adjacent to the given node side.
        self.neighbors = vec![0; self.edge_offsets[sides]];
        for &(source, dest) in self.pending.iter() {
            self.neighbors[next[source as usize]] = dest;
            next[source as usize] += 1;
            if source != dest {
                self.neighbors[next[dest as usize]] = source;
                next[dest as usize] += 1;
            }
        }

        self.pending = Vec::new();
    }

    /// Converts any graph implementing [`Topology`] into an `IndexedGraph`.
    pub fn from_topology<T: Topology>(source: &T) -> Self {
        let mut result = Self::new();
        for node in 0..source.nodes() {
            // No need to keep track of node indexes, as `Topology` uses them in the same way.
            result.add_node(&source.node_name(node), &source.sequence(node));
        }
        for node in 0..source.nodes() {
            for side in [NodeSide::Left, NodeSide::Right] {
                let source_side = support::encode_node_side(node, side);
                for (neighbor, neighbor_side) in source.neighbors(node, side) {
                    let dest_side = support::encode_node_side(neighbor, neighbor_side);
                    // Add each edge once; `finalize` deduplicates the rest.
                    if source_side <= dest_side {
                        result.pending.push((source_side as u32, dest_side as u32));
                    }
                }
            }
        }
        result.finalize();
        result
    }
}

impl Default for IndexedGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl Topology for IndexedGraph {
    fn nodes(&self) -> usize {
        self.name_offsets.len() - 1
    }

    fn edges(&self) -> usize {
        self.edge_count
    }

    fn sequence(&self, node: usize) -> Cow<'_, [u8]> {
        Cow::Borrowed(&self.sequences[self.sequence_offsets[node]..self.sequence_offsets[node + 1]])
    }

    fn sequence_len(&self, node: usize) -> usize {
        self.sequence_offsets[node + 1] - self.sequence_offsets[node]
    }

    fn degree(&self, node: usize, side: NodeSide) -> usize {
        let encoded = support::encode_node_side(node, side);
        self.edge_offsets[encoded + 1] - self.edge_offsets[encoded]
    }

    fn neighbors(&self, node: usize, side: NodeSide) -> impl Iterator<Item = (usize, NodeSide)> {
        let encoded = support::encode_node_side(node, side);
        let range = self.edge_offsets[encoded]..self.edge_offsets[encoded + 1];
        self.neighbors[range].iter().map(|&encoded| support::decode_node_side(encoded as usize))
    }

    fn node_name(&self, node: usize) -> Vec<u8> {
        self.names[self.name_offsets[node]..self.name_offsets[node + 1]].to_vec()
    }
}

impl From<&GraphInt> for IndexedGraph {
    fn from(source: &GraphInt) -> Self {
        let mut result = Self::new();
        let mut id_to_index = std::collections::HashMap::new();
        for (index, (id, node)) in source.nodes.iter().enumerate() {
            result.add_node(id.to_string().as_bytes(), &node.sequence);
            id_to_index.insert(*id, index);
        }
        for (id, node) in source.nodes.iter() {
            let from = id_to_index[id];
            for (from_o, dest_id, dest_o) in node.edges.iter() {
                result.add_edge(from, *from_o, id_to_index[dest_id], *dest_o);
            }
        }
        result.finalize();
        result
    }
}

impl From<&GraphStr> for IndexedGraph {
    fn from(source: &GraphStr) -> Self {
        let mut result = Self::new();
        let mut name_to_index = std::collections::HashMap::new();
        for (index, (name, node)) in source.nodes.iter().enumerate() {
            result.add_node(name, &node.sequence);
            name_to_index.insert(name.clone(), index);
        }
        for (name, node) in source.nodes.iter() {
            let from = name_to_index[name];
            for (from_o, dest_name, dest_o) in node.edges.iter() {
                result.add_edge(from, *from_o, name_to_index[dest_name], *dest_o);
            }
        }
        result.finalize();
        result
    }
}

//-----------------------------------------------------------------------------

// Mapping between dense node indexes and GBZ node identifiers.
//
// The variants differ a lot in size, but there is one of these per graph rather than per node.
// Boxing the bitvector would only add an indirection to every index lookup.
#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(clippy::large_enum_variant)]
enum NodeIndex {
    // The identifiers are `offset..offset + nodes`, so no mapping is needed.
    Dense,
    // Bit `id - offset` is set if the identifier exists.
    Sparse(BitVector),
}

/// A [`Topology`] wrapper over [`GBZ`].
///
/// We cannot implement the trait directly for [`GBZ`], because we need a mapping from node identifiers to indexes.
///
/// # Examples
///
/// ```
/// use pggname::Topology;
/// use pggname::topology::GbzTopology;
/// use gbz::GBZ;
/// use gbz::support;
/// use simple_sds::serialize;
///
/// let filename = support::get_test_data("example.gbz");
/// let gbz: GBZ = serialize::load_from(&filename).unwrap();
/// let graph = GbzTopology::new(&gbz).unwrap();
/// assert_eq!(graph.statistics(), (12, 13, 12));
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GbzTopology<'a> {
    graph: &'a GBZ,
    offset: usize,
    index: NodeIndex,
    node_count: usize,
    edge_count: usize,
}

impl<'a> GbzTopology<'a> {
    /// Creates a topological view of the given graph.
    ///
    /// Returns an error if the graph has more than [`MAX_NODES`] nodes.
    pub fn new(graph: &'a GBZ) -> Result<Self, String> {
        let node_count = graph.nodes();
        if node_count > MAX_NODES {
            // This should not happen, as GBZ graphs currently have the same node limit.
            return Err(format!("The graph has {} nodes, but at most {} are supported", node_count, MAX_NODES));
        }

        let offset = graph.min_node();
        let index = if node_count == 0 {
            NodeIndex::Dense
        } else {
            let span = graph.max_node() - offset + 1;
            if span == node_count {
                NodeIndex::Dense
            } else {
                let mut present = vec![false; span];
                for id in graph.node_iter() {
                    present[id - offset] = true;
                }
                let mut present: BitVector = present.into_iter().collect();
                present.enable_rank();
                present.enable_select();
                NodeIndex::Sparse(present)
            }
        };

        // Cache the edge count, as GBZ does not store it directly.
        let mut edge_count = 0;
        for id in graph.node_iter() {
            for o in [Orientation::Forward, Orientation::Reverse] {
                for (dest_id, dest_o) in graph.successors(id, o).unwrap() {
                    if support::edge_is_canonical((id, o), (dest_id, dest_o)) {
                        edge_count += 1;
                    }
                }
            }
        }

        let result = GbzTopology { graph, offset, index, node_count, edge_count };
        Ok(result)
    }

    /// Returns the GBZ node identifier corresponding to the given node index.
    pub fn node_id(&self, node: usize) -> usize {
        match &self.index {
            NodeIndex::Dense => node + self.offset,
            NodeIndex::Sparse(present) => present.select(node).unwrap() + self.offset,
        }
    }

    /// Returns the node index corresponding to the given GBZ node identifier.
    pub fn node_index(&self, id: usize) -> usize {
        match &self.index {
            NodeIndex::Dense => id - self.offset,
            NodeIndex::Sparse(present) => present.rank(id - self.offset),
        }
    }
}

impl<'a> Topology for GbzTopology<'a> {
    fn nodes(&self) -> usize {
        self.node_count
    }

    fn edges(&self) -> usize {
        self.edge_count
    }

    fn sequence(&self, node: usize) -> Cow<'_, [u8]> {
        Cow::Borrowed(self.graph.sequence(self.node_id(node)).unwrap_or(&[]))
    }

    fn sequence_len(&self, node: usize) -> usize {
        self.graph.sequence_len(self.node_id(node)).unwrap_or(0)
    }

    fn degree(&self, node: usize, side: NodeSide) -> usize {
        self.graph.outdegree(self.node_id(node), support::exit_orientation(side)).unwrap_or(0)
    }

    fn neighbors(&self, node: usize, side: NodeSide) -> impl Iterator<Item = (usize, NodeSide)> {
        let id = self.node_id(node);
        self.graph.successors(id, support::exit_orientation(side)).unwrap()
            .map(move |(dest_id, dest_o)| (self.node_index(dest_id), support::entry_side(dest_o)))
    }

    fn node_name(&self, node: usize) -> Vec<u8> {
        self.node_id(node).to_string().into_bytes()
    }
}

//-----------------------------------------------------------------------------
