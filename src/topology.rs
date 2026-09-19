//! Structural views of bidirected sequence graphs.
//!
//! The [`Graph`](crate::Graph) trait is oriented towards building a graph and serializing it in the
//! canonical GFA format. It cannot answer topological queries, such as listing the neighbors of a
//! node. The [`Topology`] trait in this module provides that view.
//!
//! A bidirected sequence graph is interpreted as an undirected graph over node sides.
//! Each node pairs its left side with its right side, and an edge `(v, o1) -> (w, o2)` joins side
//! [`support::exit_side`]`(o1)` of `v` to side [`support::entry_side`]`(o2)` of `w`.
//! Because the reverse edge `(w, flip(o2)) -> (v, flip(o1))` yields the same unordered pair of
//! sides, a bidirected edge is exactly an undirected edge over node sides.
//!
//! The module also provides [`is_subgraph`], which tests whether all nodes and edges of one graph
//! are present in another. Unlike the questions in [`crate::isomorphism`], that relationship
//! depends on the node identifiers.
//!
//! Nodes are identified by dense indexes in `0..nodes()`. Node identifiers in the original graph
//! may be sparse, and mapping between the two is the responsibility of the view, not of the
//! algorithms using it.
//!
//! Note that the integer / string identifier distinction made by [`GBZInt`](crate::graph::GBZInt)
//! and [`GBZStr`](crate::graph::GBZStr) exists only to choose a canonical order for hashing.
//! Topological queries do not depend on the order, so there is a single GBZ view here.

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

/// The maximum number of nodes in a [`Topology`].
///
/// Node sides are encoded as `2 * index + side` in a `u32` in some algorithms, which limits the
/// number of nodes to `2^31`.
pub const MAX_NODES: usize = 1 << 31;

/// A bidirected sequence graph that supports topological queries.
///
/// Nodes are identified by dense indexes in `0..self.nodes()`.
/// Methods taking a node index panic if the index is out of bounds.
///
/// # Contract
///
/// Implementations must satisfy the following:
///
/// * Neighbor lists contain no duplicates.
/// * The neighbor lists are symmetric: if `(w, t)` is a neighbor of side `s` of `v`, then `(v, s)`
///   is a neighbor of side `t` of `w`.
/// * A side self-loop (edge `(v, +) -> (v, -)` or `(v, -) -> (v, +)`) appears exactly once, in the
///   neighbor list of that side only.
/// * [`Topology::degree`] agrees with the number of items yielded by [`Topology::neighbors`].
/// * The results are stable for the lifetime of the object.
///
/// Because duplicate neighbors are not allowed, the graph has no parallel edges. This matches the
/// canonical GFA format, where duplicate `L` lines are ignored, and the behavior of
/// [`NodeInt::finalize`](crate::graph::NodeInt::finalize).
pub trait Topology {
    /// Returns the number of nodes in the graph.
    fn nodes(&self) -> usize;

    /// Returns the number of edges in the graph.
    ///
    /// This counts canonical edges, as in [`Graph::statistics`](crate::Graph::statistics), which is
    /// the same as the number of distinct unordered pairs of node sides.
    ///
    /// Note that this is not `sum_of_degrees / 2`. A self-loop `(v, +) -> (v, -)` is its own
    /// reverse and appears in one neighbor list rather than two, so that formula undercounts the
    /// edges by one half for every such loop.
    fn edges(&self) -> usize;

    /// Returns the sequence of the node with the given index.
    ///
    /// The sequence is borrowed when the implementation stores it contiguously.
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

    /// Returns the name of the node with the given index, as in the original graph.
    fn node_name(&self, node: usize) -> Vec<u8>;

    /// Returns the number of nodes, the number of edges, and total sequence length in the graph.
    ///
    /// The values are the same as in [`Graph::statistics`](crate::Graph::statistics) for the same
    /// graph.
    fn statistics(&self) -> (usize, usize, usize) {
        let mut seq_len = 0;
        for node in 0..self.nodes() {
            seq_len += self.sequence_len(node);
        }
        (self.nodes(), self.edges(), seq_len)
    }
}

//-----------------------------------------------------------------------------

/// Returns `true` if the first graph is a subgraph of the second graph.
///
/// Every node of the subgraph must be a node of the supergraph with the same name and the same
/// sequence, and every edge of the subgraph must be an edge of the supergraph. This is containment
/// of labeled graphs rather than a subgraph isomorphism test: the node names are compared as byte
/// strings, so the two graphs must agree on the identifiers. Every graph is a subgraph of itself.
///
/// Note that [`crate::isomorphism`] answers the opposite question. Isomorphism does not depend on
/// the node identifiers at all, while this relationship is all about them.
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

    // Index the names in the smaller graph and find the image of each node by scanning the larger
    // one. Duplicate names in the subgraph leave some nodes without an image, which the counter
    // catches.
    let mut names: HashMap<Vec<u8>, usize> = HashMap::with_capacity(subgraph.nodes());
    for node in 0..subgraph.nodes() {
        names.insert(subgraph.node_name(node), node);
    }
    let mut image = vec![usize::MAX; subgraph.nodes()];
    let mut matched = 0;
    for node in 0..supergraph.nodes() {
        if let Some(&source) = names.get(&supergraph.node_name(node))
            && image[source] == usize::MAX {
            image[source] = node;
            matched += 1;
        }
    }
    if matched != subgraph.nodes() {
        return false;
    }

    for (node, &target) in image.iter().enumerate() {
        if subgraph.sequence_len(node) != supergraph.sequence_len(target) {
            return false;
        }
        if subgraph.sequence(node).as_ref() != supergraph.sequence(target).as_ref() {
            return false;
        }
    }

    // Visiting each side of each node covers every edge, because the neighbor lists are symmetric.
    // A side self-loop needs no special case: it is listed once in both graphs, on the same side.
    // The neighbor lists are sorted rather than scanned, as a hub node would make the scan
    // quadratic. One buffer is reused for the whole call.
    let mut buffer: Vec<(usize, NodeSide)> = Vec::new();
    for node in 0..subgraph.nodes() {
        for side in [NodeSide::Left, NodeSide::Right] {
            let degree = subgraph.degree(node, side);
            if degree == 0 {
                continue;
            }
            if degree > supergraph.degree(image[node], side) {
                return false;
            }
            buffer.clear();
            buffer.extend(supergraph.neighbors(image[node], side));
            buffer.sort_unstable();
            for (neighbor, neighbor_side) in subgraph.neighbors(node, side) {
                if buffer.binary_search(&(image[neighbor], neighbor_side)).is_err() {
                    return false;
                }
            }
        }
    }

    true
}

//-----------------------------------------------------------------------------

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

/// An owned bidirected sequence graph with dense node indexes and full adjacency information.
///
/// [`GraphInt`] and [`GraphStr`] store only canonical edges, on the endpoint that comes first in
/// the sorting order. This type builds the adjacency information for both endpoints, which the
/// topological queries need.
///
/// Node names are stored as byte strings. This is simpler than mirroring the integer / string
/// split in [`crate::graph`], at the cost of some memory. Graphs large enough for that to matter
/// are normally GBZ graphs, which use [`GbzTopology`] and format the names on demand.
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
    // Concatenated node names, with starting offsets of length `nodes + 1`.
    names: Vec<u8>,
    name_offsets: Vec<usize>,
    // Concatenated sequences, with starting offsets of length `nodes + 1`.
    sequences: Vec<u8>,
    sequence_offsets: Vec<usize>,
    // Adjacency in CSR format over node sides, with offsets of length `2 * nodes + 1`.
    // The neighbors are stored as encoded node sides.
    edge_offsets: Vec<usize>,
    neighbors: Vec<u32>,
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
            // Kept consistent with `finalize` so that an empty graph is usable as it is.
            edge_offsets: vec![0],
            neighbors: Vec::new(),
            pending: Vec::new(),
            edge_count: 0,
        }
    }

    /// Adds a node to the graph and returns its index.
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
    /// The adjacency information is not available until [`IndexedGraph::finalize`] is called.
    pub fn add_edge(&mut self, from: usize, from_o: Orientation, to: usize, to_o: Orientation) {
        let (source, dest) = side_pair(from, from_o, to, to_o);
        self.pending.push((source as u32, dest as u32));
    }

    /// Sorts and deduplicates the edges and builds the adjacency information.
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
        let mut next = self.edge_offsets.clone();
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

    /// Builds an owned indexed graph from any topology.
    // Seam for unitig-level isomorphism: the unitig view writes into this.
    pub fn from_topology<T: Topology>(source: &T) -> Self {
        let mut result = Self::new();
        for node in 0..source.nodes() {
            result.add_node(&source.node_name(node), &source.sequence(node));
        }
        for node in 0..source.nodes() {
            for side in [NodeSide::Left, NodeSide::Right] {
                for (neighbor, neighbor_side) in source.neighbors(node, side) {
                    let source_side = support::encode_node_side(node, side);
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
        let mut indexes = std::collections::HashMap::new();
        for (index, (id, node)) in source.nodes.iter().enumerate() {
            result.add_node(id.to_string().as_bytes(), &node.sequence);
            indexes.insert(*id, index);
        }
        for (id, node) in source.nodes.iter() {
            let from = indexes[id];
            for (from_o, dest_id, dest_o) in node.edges.iter() {
                result.add_edge(from, *from_o, indexes[dest_id], *dest_o);
            }
        }
        result.finalize();
        result
    }
}

impl From<&GraphStr> for IndexedGraph {
    fn from(source: &GraphStr) -> Self {
        let mut result = Self::new();
        let mut indexes = std::collections::HashMap::new();
        for (index, (name, node)) in source.nodes.iter().enumerate() {
            result.add_node(name, &node.sequence);
            indexes.insert(name.clone(), index);
        }
        for (name, node) in source.nodes.iter() {
            let from = indexes[name];
            for (from_o, dest_name, dest_o) in node.edges.iter() {
                result.add_edge(from, *from_o, indexes[dest_name], *dest_o);
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

/// A topological view of a GBZ graph.
///
/// The view borrows the graph and does not copy the adjacency information. When the node
/// identifiers are `min_node()..=max_node()` without gaps, which is the usual case, the view needs
/// no auxiliary structures at all.
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
    /// Returns an error if the graph has too many nodes (see [`MAX_NODES`]).
    pub fn new(graph: &'a GBZ) -> Result<Self, String> {
        let node_count = graph.nodes();
        if node_count > MAX_NODES {
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

        let mut result = GbzTopology { graph, offset, index, node_count, edge_count: 0 };
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
        result.edge_count = edge_count;

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
