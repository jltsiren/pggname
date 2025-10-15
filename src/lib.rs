//! Pangenome graph naming based on hashing in a canonical order.
//!
//! A pangenome graph is a bidirected sequence graph.
//! The name of a node may be either a string or an integer identifier.
//! Nodes are ordered by their names.
//!
//! Forward edges are adjacent to the right side of the node, while reverse edges are adjacent to the left side.
//! Edges are ordered by source orientation, destination node, and destination orientation.
//! The edges are bidirectional.
//! Their canonical orientation is from the lexicographically smaller node to the larger node.
//! A self-loop is canonical, if at least one node in in forward orientation.
//!
//! The name of a graph is a hash of its canonical GFA representation.
//! The nodes are ordered by their names.
//! Each node is followed by its canonical edges in sorted order.
//! Edge lines do not include the overlap field, as pangenome graphs do not use it.
//! Header, path, and walk lines are not included in the hash, and neither are optional fields.

use gbwt::{GBZ, Orientation};
use gbwt::support;

use std::collections::BTreeMap;

//-----------------------------------------------------------------------------

/// Parses the orientation from GFA field.
///
/// Accepts `+` for forward and `-` for reverse orientation.
/// Returns an error if the field is not recognized.
pub fn parse_orientation(field: &[u8]) -> Result<Orientation, String> {
    match field {
        b"+" => Ok(Orientation::Forward),
        b"-" => Ok(Orientation::Reverse),
        _ => Err(format!("Invalid orientation: {}", String::from_utf8_lossy(field))),
    }
}

/// Returns the orientation as `+` or `-`.
pub fn as_byte(o: Orientation) -> u8 {
    match o {
        Orientation::Forward => b'+',
        Orientation::Reverse => b'-',
    }
}

//-----------------------------------------------------------------------------

/// A node with a string name in a bidirected sequence graph.
///
/// The node does not store its name, as the user is expected to know it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeStr {
    /// Sequence associated with the node.
    pub sequence: Vec<u8>,
    /// Canonical edges as (source orientation, destination node, destination orientation).
    pub edges: Vec<(Orientation, Vec<u8>, Orientation)>,
    /// Have we seen the node in the graph?
    pub seen: bool,
}

impl NodeStr {
    /// Creates a new node.
    ///
    /// If a sequence is provided, the node is marked as seen.
    pub fn new(sequence: Option<Vec<u8>>) -> Self {
        if let Some(sequence) = sequence {
            NodeStr {
                sequence,
                edges: Vec::new(),
                seen: true,
            }
        } else {
            NodeStr {
                sequence: Vec::new(),
                edges: Vec::new(),
                seen: false,
            }
        }
    }

    /// Sorts the edges and removes duplicates.
    pub fn finalize(&mut self) {
        self.edges.sort();
        self.edges.dedup();
    }

    /// Serializes the node and its edges in GFA format.
    pub fn serialize(&self, name: &[u8]) -> Vec<u8> {
        let mut result = Vec::new();

        result.extend_from_slice(b"S\t");
        result.extend_from_slice(name);
        result.extend_from_slice(b"\t");
        result.extend_from_slice(&self.sequence);
        result.extend_from_slice(b"\n");

        for (source_o, dest_name, dest_o) in &self.edges {
            result.extend_from_slice(b"L\t");
            result.extend_from_slice(name);
            result.extend_from_slice(b"\t");
            result.push(as_byte(*source_o));
            result.extend_from_slice(b"\t");
            result.extend_from_slice(dest_name);
            result.extend_from_slice(b"\t");
            result.push(as_byte(*dest_o));
            result.extend_from_slice(b"\n");
        }

        result
    }
}

//-----------------------------------------------------------------------------

/// A node with an integer identifier in a bidirected sequence graph.
///
/// The node does not store its identifier, as the user is expected to know it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeInt {
    /// Sequence associated with the node.
    pub sequence: Vec<u8>,
    /// Canonical edges as (source orientation, destination node, destination orientation).
    pub edges: Vec<(Orientation, usize, Orientation)>,
    /// Have we seen the node in the graph?
    pub seen: bool,
}

impl NodeInt {
    /// Creates a new node.
    ///
    /// If a sequence is provided, the node is marked as seen.
    pub fn new(sequence: Option<Vec<u8>>) -> Self {
        if let Some(sequence) = sequence {
            NodeInt {
                sequence,
                edges: Vec::new(),
                seen: true,
            }
        } else {
            NodeInt {
                sequence: Vec::new(),
                edges: Vec::new(),
                seen: false,
            }
        }
    }

    /// Sorts the edges and removes duplicates.
    pub fn finalize(&mut self) {
        self.edges.sort();
        self.edges.dedup();
    }

    /// Serializes the node and its edges in GFA format.
    pub fn serialize(&self, id: usize) -> Vec<u8> {
        let mut result = Vec::new();
        let name = id.to_string();

        result.push(b'S');
        result.push(b'\t');
        result.extend_from_slice(name.as_bytes());
        result.push(b'\t');
        result.extend_from_slice(&self.sequence);
        result.push(b'\n');

        for (source_o, dest_id, dest_o) in &self.edges {
            result.push(b'L');
            result.push(b'\t');
            result.extend_from_slice(name.as_bytes());
            result.push(b'\t');
            result.push(as_byte(*source_o));
            result.push(b'\t');
            result.extend_from_slice(dest_id.to_string().as_bytes());
            result.push(b'\t');
            result.push(as_byte(*dest_o));
            result.push(b'\n');
        }

        result
    }
}

//-----------------------------------------------------------------------------

/// A bidirected sequence graph.
pub trait Graph {
    /// Creates a new empty graph.
    fn new() -> Self;

    /// Adds a node to the graph.
    ///
    /// Returns an error if the node already exists with a different sequence.
    /// Returns an error if the name of the node is not valid.
    fn add_node(&mut self, name: &[u8], sequence: &[u8]) -> Result<(), String>;

    /// Adds an edge to the graph.
    ///
    /// Also adds the nodes implied by the edge if they do not exist.
    /// Returns an error if the name of the node is not valid.
    fn add_edge(&mut self, source_name: &[u8], source_o: Orientation, dest_name: &[u8], dest_o: Orientation) -> Result<(), String>;

    /// Finalizes the graph by sorting and deduplicating edges.
    ///
    /// Returns an error if some nodes required by the edges are missing.
    fn finalize(&mut self) -> Result<(), String>;

    /// Returns the number of nodes, the number of edges, and total sequence length in the graph.
    fn statistics(&self) -> (usize, usize, usize);

    /// Returns an iterator over serialized nodes in sorted order.
    fn node_iter(&self) -> impl Iterator<Item=Vec<u8>>;
}

//-----------------------------------------------------------------------------

/// A bidirected sequence graph using string names for the nodes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphStr {
    /// Nodes in the graph.
    pub nodes: BTreeMap<Vec<u8>, NodeStr>,
}

impl GraphStr {
    /// Returns `true` if the edge is in its canonical orientation.
    pub fn edge_is_canonical(
        source_name: &[u8], source_o: Orientation, dest_name: &[u8], dest_o: Orientation
    ) -> bool {
        match source_name.cmp(dest_name) {
            std::cmp::Ordering::Less => true,
            std::cmp::Ordering::Greater => false,
            std::cmp::Ordering::Equal => source_o == Orientation::Forward || dest_o == Orientation::Forward,
        }
    }
}

impl Graph for GraphStr {
    fn new() -> Self {
        GraphStr {
            nodes: BTreeMap::new(),
        }
    }

    fn add_node(&mut self, name: &[u8], sequence: &[u8]) -> Result<(), String> {
        let name = name.to_vec();
        if let Some(node) = self.nodes.get_mut(&name) {
            if node.seen && sequence != node.sequence {
                let msg = format!("Node {} already exists with a different sequence", String::from_utf8_lossy(&name));
                return Err(msg);
            }
            // If the node already exists, update its sequence.
            node.sequence = sequence.to_vec();
            node.seen = true;
            Ok(())
        } else {
            // If the node doesn't exist, create a new one.
            let node = NodeStr::new(Some(sequence.to_vec()));
            self.nodes.insert(name, node);
            Ok(())
        }
    }

    fn add_edge(&mut self, source_name: &[u8], source_o: Orientation, dest_name: &[u8], dest_o: Orientation) -> Result<(), String> {
        // Ensure that the nodes exist.
        if !self.nodes.contains_key(source_name) {
            self.nodes.insert(source_name.to_vec(), NodeStr::new(None));
        }
        if !self.nodes.contains_key(dest_name) {
            self.nodes.insert(dest_name.to_vec(), NodeStr::new(None));
        }

        if Self::edge_is_canonical(source_name, source_o, dest_name, dest_o) {
            let source_node = self.nodes.get_mut(source_name).unwrap();
            source_node.edges.push((source_o, dest_name.to_vec(), dest_o));
        } else {
            let dest_node = self.nodes.get_mut(dest_name).unwrap();
            dest_node.edges.push((dest_o.flip(), source_name.to_vec(), source_o.flip()));
        }

        Ok(())
    }

    fn finalize(&mut self) -> Result<(), String> {
        let mut unseen = 0;
        for node in self.nodes.values_mut() {
            node.finalize();
            if !node.seen {
                unseen += 1;
            }
        }
        if unseen > 0 {
            return Err(format!("{} nodes required by the edges are missing", unseen));
        }
        Ok(())
    }

    fn statistics(&self) -> (usize, usize, usize) {
        let mut edge_count = 0;
        let mut seq_len = 0;
        for node in self.nodes.values() {
            edge_count += node.edges.len();
            seq_len += node.sequence.len();
        }
        (self.nodes.len(), edge_count, seq_len)
    }

    fn node_iter(&self) -> impl Iterator<Item=Vec<u8>> {
        self.nodes.iter().map(|(name, node)| node.serialize(name))
    }
}

//-----------------------------------------------------------------------------

/// A bidirected sequence graph using integer identifiers for the nodes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphInt {
    /// Nodes in the graph.
    pub nodes: BTreeMap<usize, NodeInt>,
}

impl GraphInt {
    /// Returns `true` if the edge is in its canonical orientation.
    pub fn edge_is_canonical(
        source_id: usize, source_o: Orientation, dest_id: usize, dest_o: Orientation
    ) -> bool {
        match source_id.cmp(&dest_id) {
            std::cmp::Ordering::Less => true,
            std::cmp::Ordering::Greater => false,
            std::cmp::Ordering::Equal => source_o == Orientation::Forward || dest_o == Orientation::Forward,
        }
    }

    /// Parses the node identifier from a byte slice.
    pub fn parse_id(name: &[u8]) -> Result<usize, String> {
        let name_str = str::from_utf8(name)
            .map_err(|e| format!("Error parsing node name {}: {}", String::from_utf8_lossy(name), e))?;
        let id = name_str.parse::<usize>()
            .map_err(|e| format!("Error parsing node name {}: {}", String::from_utf8_lossy(name), e))?;
        if id == 0 {
            return Err(String::from("Node identifier 0 is reserved for technical purposes"));
        }
        Ok(id)
    }
}

impl Graph for GraphInt {
    fn new() -> Self {
        GraphInt {
            nodes: BTreeMap::new(),
        }
    }

    fn add_node(&mut self, name: &[u8], sequence: &[u8]) -> Result<(), String> {
        let id = std::str::from_utf8(name)
            .map_err(|e| format!("Error parsing node name {}: {}", String::from_utf8_lossy(name), e))?
            .parse::<usize>()
            .map_err(|e| format!("Error parsing node name {}: {}", String::from_utf8_lossy(name), e))?;
        if let Some(node) = self.nodes.get_mut(&id) {
            if node.seen && sequence != node.sequence {
                let msg = format!("Node {} already exists with a different sequence", String::from_utf8_lossy(&name));
                return Err(msg);
            }
            // If the node already exists, update its sequence.
            node.sequence = sequence.to_vec();
            node.seen = true;
            Ok(())
        } else {
            // If the node doesn't exist, create a new one.
            let node = NodeInt::new(Some(sequence.to_vec()));
            self.nodes.insert(id, node);
            Ok(())
        }
    }

    fn add_edge(&mut self, source_name: &[u8], source_o: Orientation, dest_name: &[u8], dest_o: Orientation) -> Result<(), String> {
        let source_id = Self::parse_id(source_name)?;
        let dest_id = Self::parse_id(dest_name)?;

        // Ensure that the nodes exist.
        if !self.nodes.contains_key(&source_id) {
            self.nodes.insert(source_id, NodeInt::new(None));
        }
        if !self.nodes.contains_key(&dest_id) {
            self.nodes.insert(dest_id, NodeInt::new(None));
        }

        if Self::edge_is_canonical(source_id, source_o, dest_id, dest_o) {
            let source_node = self.nodes.get_mut(&source_id).unwrap();
            source_node.edges.push((source_o, dest_id, dest_o));
        } else {
            let dest_node = self.nodes.get_mut(&dest_id).unwrap();
            dest_node.edges.push((dest_o.flip(), source_id, source_o.flip()));
        }

        Ok(())
    }

    fn finalize(&mut self) -> Result<(), String> {
        let mut unseen = 0;
        for node in self.nodes.values_mut() {
            node.finalize();
            if !node.seen {
                unseen += 1;
            }
        }
        if unseen > 0 {
            return Err(format!("{} nodes required by the edges are missing", unseen));
        }
        Ok(())
    }

    fn statistics(&self) -> (usize, usize, usize) {
        let mut edge_count = 0;
        let mut seq_len = 0;
        for node in self.nodes.values() {
            edge_count += node.edges.len();
            seq_len += node.sequence.len();
        }
        (self.nodes.len(), edge_count, seq_len)
    }

    fn node_iter(&self) -> impl Iterator<Item=Vec<u8>> {
        self.nodes.iter().map(|(id, node)| node.serialize(*id))
    }
}

//-----------------------------------------------------------------------------

/// A GBZ wrapper using integer identifiers for the nodes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GBZInt {
    pub graph: GBZ,
}

impl Graph for GBZInt {
    fn new() -> Self {
        unimplemented!()
    }

    fn add_node(&mut self, _: &[u8], _sequence: &[u8]) -> Result<(), String> {
        unimplemented!()
    }

    fn add_edge(&mut self, _: &[u8], _: Orientation, _: &[u8], _: Orientation) -> Result<(), String> {
        unimplemented!()
    }

    fn finalize(&mut self) -> Result<(), String> {
        Ok(())
    }

    fn statistics(&self) -> (usize, usize, usize) {
        let node_count = self.graph.nodes();

        let mut edge_count = 0;
        let mut seq_len = 0;
        for source_id in self.graph.node_iter() {
            for source_o in [Orientation::Forward, Orientation::Reverse] {
                for (dest_id, dest_o) in self.graph.successors(source_id, source_o).unwrap() {
                    if support::edge_is_canonical((source_id, source_o), (dest_id, dest_o)) {
                        edge_count += 1;
                    }
                }
            }
            seq_len += self.graph.sequence_len(source_id).unwrap_or(0);
        }

        (node_count, edge_count, seq_len)
    }

    fn node_iter(&self) -> impl Iterator<Item=Vec<u8>> {
        self.graph.node_iter().map(|id| {
            let sequence = self.graph.sequence(id).unwrap_or(&[]);
            let mut node = NodeInt::new(Some(sequence.to_vec()));
            for source_o in [Orientation::Forward, Orientation::Reverse] {
                for (dest_id, dest_o) in self.graph.successors(id, source_o).unwrap() {
                    if support::edge_is_canonical((id, source_o), (dest_id, dest_o)) {
                        node.edges.push((source_o, dest_id, dest_o));
                    }
                }
            }
            node.finalize();
            node.serialize(id)
        })
    }
}

//-----------------------------------------------------------------------------

/// A GBZ wrapper using string names for the nodes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GBZStr {
    pub graph: GBZ,
}

impl Graph for GBZStr {
    fn new() -> Self {
        unimplemented!()
    }

    fn add_node(&mut self, _: &[u8], _sequence: &[u8]) -> Result<(), String> {
        unimplemented!()
    }

    fn add_edge(&mut self, _: &[u8], _: Orientation, _: &[u8], _: Orientation) -> Result<(), String> {
        unimplemented!()
    }

    fn finalize(&mut self) -> Result<(), String> {
        Ok(())
    }

    fn statistics(&self) -> (usize, usize, usize) {
        let node_count = self.graph.nodes();

        let mut edge_count = 0;
        let mut seq_len = 0;
        for source_id in self.graph.node_iter() {
            for source_o in [Orientation::Forward, Orientation::Reverse] {
                for (dest_id, dest_o) in self.graph.successors(source_id, source_o).unwrap() {
                    if support::edge_is_canonical((source_id, source_o), (dest_id, dest_o)) {
                        edge_count += 1;
                    }
                }
            }
            seq_len += self.graph.sequence_len(source_id).unwrap_or(0);
        }

        (node_count, edge_count, seq_len)
    }

    fn node_iter(&self) -> impl Iterator<Item=Vec<u8>> {
        let mut ordered_nodes: Vec<(String, usize)> = self.graph.node_iter().map(|id| (id.to_string(), id)).collect();
        ordered_nodes.sort_by(|a, b| a.0.cmp(&b.0));

        ordered_nodes.into_iter().map(|(source_name, source_id)| {
            let sequence = self.graph.sequence(source_id).unwrap_or(&[]);
            let mut node = NodeStr::new(Some(sequence.to_vec()));
            for source_o in [Orientation::Forward, Orientation::Reverse] {
                for (dest_id, dest_o) in self.graph.successors(source_id, source_o).unwrap() {
                    let dest_name = dest_id.to_string().as_bytes().to_vec();
                    if GraphStr::edge_is_canonical(source_name.as_bytes(), source_o, &dest_name, dest_o) {
                        node.edges.push((source_o, dest_name, dest_o));
                    }
                }
            }
            node.finalize();
            node.serialize(source_name.as_bytes())
        })
    }
}

//-----------------------------------------------------------------------------
