//! Algorithms for testing pangenome graph isomorphism.
//!
//! Two graphs are isomorphic if one can be transformed into the other by renaming the nodes.
//! After the transformation, node labels (sequences) and edges must match.
//! Because pangenome graphs are bidirectional, the transformation may flip the orientation of a node.
//!
//! If two graphs are isomorphic, there is a bidirectional translation between their coordinates.
//! See [`Translation`] for more details.
//!
//! The core algorithm is [`are_isomorphic`], which compares the graphs directly.
//! [`are_isomorphic_unitigs`] considers unitig graphs, where maximal non-branching paths are collapsed into single nodes.
//! It is more appropriate with pangenome graphs, where different tools have different opinions on node lengths.
//! See [`Unitigs`] for more details.
//!
//! # Overview
//!
//! The algorithm colors the nodes by hashing isomorphism-invariant data.
//! Then it searches for a mapping consistent with the colors.
//! A potential mapping is verified against the sequences and the edges.
//! The algorithm returns [`Isomorphism::Isomorphic`] with a [`NodeMapping`] if the graphs are isomorphic.
//! It returns [`Isomorphism::NotIsomorphic`] with a [`Mismatch`] if it can guarantee that the graphs are not isomorphic.
//! If the algorithm runs out of search budget, the result is [`Isomorphism::Unresolved`].
//! That should rarely happen with pangenome graphs, as node labels make the coloring stage very effective.
//!
//! When determining unitig-level isomorphism, the return value is [`UnitigIsomorphism`] instead.
//! A positive result then includes a [`Translation`].

use crate::topology::Topology;
use crate::unitigs::Unitigs;

use coloring::{Classes, Coloring};
use matching::{Matcher, Outcome};
use translation::Translation;

use gbz::Orientation;

use std::fmt;
use std::io::{self, Write};

pub mod hashing;
pub mod translation;

pub(crate) mod coloring;
pub(crate) mod matching;

#[cfg(test)]
mod tests;

//-----------------------------------------------------------------------------

/// Options for [`are_isomorphic`].
///
/// # Examples
///
/// ```
/// use pggname::isomorphism::Options;
///
/// let options = Options { refinement_rounds: 8, ..Options::default() };
/// assert_eq!(options.refinement_rounds, 8);
/// ```
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Options {
    /// Maximum number of color refinement rounds.
    ///
    /// Each round refines the coloring of a node by the colors of its neighbors.
    pub refinement_rounds: usize,

    /// Maximum number of choices the search may make before giving up.
    pub search_budget: usize,
}

impl Options {
    /// Default number of color refinement rounds.
    pub const DEFAULT_REFINEMENT_ROUNDS: usize = 4;

    /// Default search budget.
    pub const DEFAULT_SEARCH_BUDGET: usize = 1 << 20;
}

impl Default for Options {
    fn default() -> Self {
        Options {
            refinement_rounds: Self::DEFAULT_REFINEMENT_ROUNDS,
            search_budget: Self::DEFAULT_SEARCH_BUDGET,
        }
    }
}

/// Statistics on an isomorphism computation.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Statistics {
    /// Number of color refinement rounds used.
    pub rounds: usize,
    /// Number of color classes after refinement.
    pub color_classes: usize,
    /// Number of choices made by the search.
    pub choices: usize,
}

//-----------------------------------------------------------------------------

/// A mapping between the nodes of two isomorphic graphs.
///
/// Nodes are identified by their indexes in [`Topology`].
/// Each node in the first graph maps to the given node in in the second graph in the given orientation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeMapping {
    // Image of each node using GBWT encoding for oriented nodes.
    mapping: Vec<u32>,
}

impl NodeMapping {
    /// Returns the number of nodes in each graph.
    pub fn len(&self) -> usize {
        self.mapping.len()
    }

    /// Returns `true` if the mapping is empty.
    pub fn is_empty(&self) -> bool {
        self.mapping.is_empty()
    }

    /// Returns the image of the given node and its relative orientation.
    pub fn get(&self, node: usize) -> (usize, Orientation) {
        crate::decode_oriented_node(self.mapping[node])
    }

    /// Returns an iterator over the mapping as `(source, destination, orientation)`.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (usize, usize, Orientation)> {
        self.mapping.iter().enumerate().map(|(source, &encoded)| {
            let (destination, orientation) = crate::decode_oriented_node(encoded);
            (source, destination, orientation)
        })
    }

    /// Returns `true` if no node maps to the reverse complement of another node.
    pub fn is_forward(&self) -> bool {
        self.mapping.iter().all(|&encoded| crate::decode_oriented_node(encoded).1 == Orientation::Forward)
    }

    /// Returns the inverse mapping.
    pub fn invert(&self) -> NodeMapping {
        let mut mapping = vec![0; self.mapping.len()];
        for (source, destination, orientation) in self.iter() {
            mapping[destination] = crate::encode_oriented_node(source, orientation);
        }
        NodeMapping { mapping }
    }

    // Creates a mapping from encoded images.
    pub(crate) fn from_encoded(mapping: Vec<u32>) -> Self {
        NodeMapping { mapping }
    }
}

/// The reason why two graphs are not isomorphic.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Mismatch {
    /// The graphs have different numbers of nodes.
    NodeCount,
    /// The graphs have different numbers of edges.
    EdgeCount,
    /// The graphs have different total sequence lengths.
    SequenceLength,
    /// The graphs have different multisets of node colors.
    Colors,
    /// No mapping consistent with the node colors exists.
    Structure,
}

impl fmt::Display for Mismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Mismatch::NodeCount => write!(f, "different numbers of nodes"),
            Mismatch::EdgeCount => write!(f, "different numbers of edges"),
            Mismatch::SequenceLength => write!(f, "different total sequence lengths"),
            Mismatch::Colors => write!(f, "different node colors"),
            Mismatch::Structure => write!(f, "different structure"),
        }
    }
}

/// The result of a node-level isomorphism test.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Isomorphism {
    /// The graphs are isomorphic, with the given node mapping.
    Isomorphic(NodeMapping),
    /// The graphs are not isomorphic, for the given reason.
    NotIsomorphic(Mismatch),
    /// The search budget was exhausted before the question could be settled.
    Unresolved,
}

impl Isomorphism {
    /// Returns `true` if the graphs are isomorphic.
    pub fn is_isomorphic(&self) -> bool {
        matches!(self, Isomorphism::Isomorphic(_))
    }

    /// Returns the node mapping, if the graphs are isomorphic.
    pub fn mapping(&self) -> Option<&NodeMapping> {
        match self {
            Isomorphism::Isomorphic(mapping) => Some(mapping),
            _ => None,
        }
    }

    /// Returns the node mapping, if the graphs are isomorphic, consuming the result.
    pub fn into_mapping(self) -> Option<NodeMapping> {
        match self {
            Isomorphism::Isomorphic(mapping) => Some(mapping),
            _ => None,
        }
    }
}

impl fmt::Display for Isomorphism {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Isomorphism::Isomorphic(_) => write!(f, "isomorphic"),
            Isomorphism::NotIsomorphic(mismatch) => write!(f, "not isomorphic: {}", mismatch),
            Isomorphism::Unresolved => write!(f, "unresolved"),
        }
    }
}

/// The result of a unitig-level isomorphism test.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnitigIsomorphism {
    /// The graphs are equivalent, with the given translation.
    Isomorphic(Translation),
    /// The graphs are not equivalent, for the given reason.
    NotIsomorphic(Mismatch),
    /// The search budget was exhausted before the question could be settled.
    Unresolved,
}

impl UnitigIsomorphism {
    /// Returns `true` if the graphs are equivalent.
    pub fn is_isomorphic(&self) -> bool {
        matches!(self, UnitigIsomorphism::Isomorphic(_))
    }

    /// Returns the translation, if the graphs are equivalent.
    pub fn translation(&self) -> Option<&Translation> {
        match self {
            UnitigIsomorphism::Isomorphic(translation) => Some(translation),
            _ => None,
        }
    }

    /// Returns the translation, if the graphs are equivalent, consuming the result.
    pub fn into_translation(self) -> Option<Translation> {
        match self {
            UnitigIsomorphism::Isomorphic(translation) => Some(translation),
            _ => None,
        }
    }
}

impl fmt::Display for UnitigIsomorphism {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UnitigIsomorphism::Isomorphic(_) => write!(f, "isomorphic"),
            UnitigIsomorphism::NotIsomorphic(mismatch) => write!(f, "not isomorphic: {}", mismatch),
            UnitigIsomorphism::Unresolved => write!(f, "unresolved"),
        }
    }
}

//-----------------------------------------------------------------------------

/// Determines whether the two graphs are isomorphic at node level.
///
/// See [`are_isomorphic_with_statistics`] for a version of the algorithm that also returns statistics.
/// See [`are_isomorphic_unitigs`] for determining isomorphism at unitig level.
///
/// # Examples
///
/// ```
/// use pggname::Topology;
/// use pggname::algorithms;
/// use pggname::graph::GraphInt;
/// use pggname::isomorphism::{self, Isomorphism, Options};
/// use pggname::topology::{GbzTopology, IndexedGraph};
/// use gbz::GBZ;
/// use gbz::support;
/// use simple_sds::serialize;
/// use std::fs::OpenOptions;
/// use std::io::BufReader;
///
/// // The same graph as GFA and as GBZ.
/// let filename = support::get_test_data("example.gfa");
/// let file = OpenOptions::new().read(true).open(&filename).unwrap();
/// let gfa: GraphInt = algorithms::parse_gfa(BufReader::new(file)).unwrap();
/// let from_gfa = IndexedGraph::from(&gfa);
///
/// let filename = support::get_test_data("example.gbz");
/// let gbz: GBZ = serialize::load_from(&filename).unwrap();
/// let from_gbz = GbzTopology::new(&gbz).unwrap();
///
/// let result = isomorphism::are_isomorphic(&from_gfa, &from_gbz, &Options::default());
/// assert!(result.is_isomorphic());
/// let mapping = result.mapping().unwrap();
/// assert_eq!(mapping.len(), 12);
/// // In this toy example, every node maps to itself.
/// assert!(mapping.iter().all(|(source, destination, _)| source == destination));
/// ```
pub fn are_isomorphic<A: Topology, B: Topology>(
    first: &A, second: &B, options: &Options
) -> Isomorphism {
    are_isomorphic_with_statistics(first, second, options).0
}

/// A version of [`are_isomorphic`] that also returns statistics.
pub fn are_isomorphic_with_statistics<A: Topology, B: Topology>(
    first: &A, second: &B, options: &Options
) -> (Isomorphism, Statistics) {
    let mut statistics = Statistics::default();

    // Exact invariants that need no coloring.
    if first.nodes() != second.nodes() {
        return (Isomorphism::NotIsomorphic(Mismatch::NodeCount), statistics);
    }
    if first.edges() != second.edges() {
        return (Isomorphism::NotIsomorphic(Mismatch::EdgeCount), statistics);
    }
    if total_sequence_length(first) != total_sequence_length(second) {
        return (Isomorphism::NotIsomorphic(Mismatch::SequenceLength), statistics);
    }

    let first_coloring = Coloring::new(first, options);
    let second_coloring = Coloring::new(second, options);
    statistics.rounds = first_coloring.rounds();
    // Isomorphic graphs refine identically, so they also stop at the same round.
    if first_coloring.rounds() != second_coloring.rounds()
        || first_coloring.classes() != second_coloring.classes()
        || first_coloring.signature() != second_coloring.signature() {
        return (Isomorphism::NotIsomorphic(Mismatch::Colors), statistics);
    }

    let classes = match Classes::new(&first_coloring, &second_coloring) {
        Ok(classes) => classes,
        Err(mismatch) => return (Isomorphism::NotIsomorphic(mismatch), statistics),
    };
    statistics.color_classes = classes.len();

    let mut matcher = Matcher::new(
        first, second, &first_coloring, &second_coloring, &classes, options
    );
    let outcome = matcher.run();
    statistics.choices = matcher.choices();

    match outcome {
        Outcome::Complete => {
            let mapping = matcher.into_mapping();
            match verify(first, second, &mapping) {
                Ok(()) => (Isomorphism::Isomorphic(mapping), statistics),
                // Every pair passed the local checks, so this can only happen if the choices made
                // along the way were wrong. Without choices, the mapping was the only candidate.
                Err(mismatch) if statistics.choices == 0 => {
                    (Isomorphism::NotIsomorphic(mismatch), statistics)
                },
                Err(_) => (Isomorphism::Unresolved, statistics),
            }
        },
        // The search tried every option, so no isomorphism exists.
        Outcome::Conflict => (Isomorphism::NotIsomorphic(Mismatch::Structure), statistics),
        Outcome::Exhausted => (Isomorphism::Unresolved, statistics),
    }
}

//-----------------------------------------------------------------------------

/// Determines whether two graphs are isomorphic at unitig level.
///
/// See [`are_isomorphic_unitigs_with_statistics`] for a version of the algorithm that also returns statistics.
/// See [`are_isomorphic`] for determining isomorphism at node level.
///
/// Returns an error if either graph has a connected component that is a cycle with no branches.
/// See [`crate::unitigs`] for further details.
///
/// # Examples
///
/// ```
/// use pggname::Topology;
/// use pggname::isomorphism::{self, Options};
/// use pggname::topology::IndexedGraph;
/// use gbz::Orientation;
///
/// // One graph has a single node, the other has it chopped in two.
/// let mut whole = IndexedGraph::new();
/// whole.add_node(b"1", b"GATTACA");
/// whole.finalize();
///
/// let mut chopped = IndexedGraph::new();
/// let a = chopped.add_node(b"1", b"GATT");
/// let b = chopped.add_node(b"2", b"ACA");
/// chopped.add_edge(a, Orientation::Forward, b, Orientation::Forward);
/// chopped.finalize();
///
/// // They are not isomorphic as graphs.
/// let result = isomorphism::are_isomorphic(&whole, &chopped, &Options::default());
/// assert!(!result.is_isomorphic());
///
/// // They are isomorphic as unitig graphs.
/// let result = isomorphism::are_isomorphic_unitigs(&whole, &chopped, &Options::default()).unwrap();
/// assert!(result.is_isomorphic());
///
/// // Node 1 of the first graph covers both nodes of the second.
/// let translation = result.translation().unwrap();
/// assert_eq!(translation.len(), 1);
/// let first_walk: Vec<_> = translation.first_walk(0).collect();
/// assert_eq!(first_walk, vec![(0, Orientation::Forward)]);
/// let second_walk: Vec<_> = translation.second_walk(0).collect();
/// assert_eq!(second_walk, vec![(0, Orientation::Forward), (1, Orientation::Forward)]);
/// ```
pub fn are_isomorphic_unitigs<A: Topology, B: Topology>(
    first: &A, second: &B, options: &Options
) -> Result<UnitigIsomorphism, String> {
    are_isomorphic_unitigs_with_statistics(first, second, options).map(|(result, _)| result)
}

/// A version of [`are_isomorphic_unitigs`] that also returns statistics.
pub fn are_isomorphic_unitigs_with_statistics<A: Topology, B: Topology>(
    first: &A, second: &B, options: &Options
) -> Result<(UnitigIsomorphism, Statistics), String> {
    let first_unitigs = Unitigs::new(first)?;
    let second_unitigs = Unitigs::new(second)?;

    let (result, statistics) = are_isomorphic_with_statistics(
        first_unitigs.graph(), second_unitigs.graph(), options
    );

    let result = match result {
        Isomorphism::Isomorphic(mapping) => {
            let translation = Translation::new(&first_unitigs, &second_unitigs, &mapping);
            match translation::verify_translation(first, second, &translation) {
                Ok(()) => UnitigIsomorphism::Isomorphic(translation),
                // The isomorphism of the compacted graphs has already been verified, so this
                // cannot happen. Report it as unresolved rather than claim an equivalence.
                Err(_) => UnitigIsomorphism::Unresolved,
            }
        },
        Isomorphism::NotIsomorphic(mismatch) => UnitigIsomorphism::NotIsomorphic(mismatch),
        Isomorphism::Unresolved => UnitigIsomorphism::Unresolved,
    };

    Ok((result, statistics))
}

//-----------------------------------------------------------------------------

// Returns the total length of the sequences in the graph.
fn total_sequence_length<T: Topology>(graph: &T) -> usize {
    (0..graph.nodes()).map(|node| graph.sequence_len(node)).sum()
}

/// Returns the side corresponding to the given side under the given relative orientation.
pub(crate) fn map_side(side: gbz::NodeSide, orientation: Orientation) -> gbz::NodeSide {
    match orientation {
        Orientation::Forward => side,
        Orientation::Reverse => side.flip(),
    }
}

/// Verifies that the mapping is an isomorphism between the two graphs.
///
/// See also [`translation::verify_translation`].
pub fn verify<A: Topology, B: Topology>(
    a: &A, b: &B, mapping: &NodeMapping
) -> Result<(), Mismatch> {
    if a.nodes() != b.nodes() || mapping.len() != a.nodes() {
        return Err(Mismatch::NodeCount);
    }
    if a.edges() != b.edges() {
        return Err(Mismatch::EdgeCount);
    }

    // The mapping must be a bijection.
    let mut taken = vec![false; b.nodes()];
    for (_, destination, _) in mapping.iter() {
        if destination >= b.nodes() || taken[destination] {
            return Err(Mismatch::Structure);
        }
        taken[destination] = true;
    }

    for (source, destination, orientation) in mapping.iter() {
        // The sequences must match, up to the relative orientation.
        let sequence = a.sequence(source);
        let image = b.sequence(destination);
        let matches = match orientation {
            Orientation::Forward => sequence == image,
            Orientation::Reverse => hashing::reverse_complement(&sequence) == image.as_ref(),
        };
        if !matches {
            return Err(Mismatch::Structure);
        }

        // Check the edges for each node side.
        for side in [gbz::NodeSide::Left, gbz::NodeSide::Right] {
            let image_side = map_side(side, orientation);
            if a.degree(source, side) != b.degree(destination, image_side) {
                return Err(Mismatch::Structure);
            }
            let mut expected: Vec<(usize, gbz::NodeSide)> = a.neighbors(source, side)
                .map(|(node, node_side)| {
                    let (image, image_orientation) = mapping.get(node);
                    (image, map_side(node_side, image_orientation))
                })
                .collect();
            let mut found: Vec<(usize, gbz::NodeSide)> = b.neighbors(destination, image_side).collect();
            expected.sort_unstable();
            found.sort_unstable();
            if expected != found {
                return Err(Mismatch::Structure);
            }
        }
    }

    Ok(())
}

/// Writes the translation in TSV format.
///
/// The output contains one line for each unitig.
/// Each line has two fields: the path in the first graph and the corresponding path in the second graph.
/// The paths are written as in GFA W-lines.
pub fn write_translation<A: Topology, B: Topology, W: Write>(
    first: &A, second: &B, translation: &Translation, writer: &mut W
) -> io::Result<()> {
    for index in 0..translation.len() {
        write_path(first, translation.first_walk(index), writer)?;
        writer.write_all(b"\t")?;
        write_path(second, translation.second_walk(index), writer)?;
        writer.write_all(b"\n")?;
    }

    Ok(())
}

// Writes a path using the GFA W-line format.
fn write_path<T: Topology, W: Write>(
    graph: &T, walk: impl Iterator<Item = (usize, Orientation)>, writer: &mut W
) -> io::Result<()> {
    for (node, orientation) in walk {
        writer.write_all(match orientation {
            Orientation::Forward => b">",
            Orientation::Reverse => b"<",
        })?;
        writer.write_all(&graph.node_name(node))?;
    }

    Ok(())
}

//-----------------------------------------------------------------------------
