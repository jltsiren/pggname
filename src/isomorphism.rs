//! Testing whether two pangenome graphs are isomorphic.
//!
//! Two graphs are isomorphic if one can be transformed into the other by renaming the nodes.
//! More precisely, there must be a bijection between the nodes that preserves the sequences and the
//! edges. If [`Options::allow_flips`] is set, a node may also map to the reverse complement of
//! another node. Because flipping a node swaps its left and right sides, every edge endpoint at
//! that node then changes orientation.
//!
//! Note that [`stable_name`](crate::stable_name) depends on the node identifiers, while isomorphism
//! does not. Two isomorphic graphs typically have different stable names.
//!
//! # Guarantees
//!
//! The algorithm colors the nodes by hashing isomorphism-invariant data and then searches for a
//! mapping consistent with the colors. A positive answer is always verified against the sequences
//! and the edges, without using any hash values, so it is never wrong. A negative answer is
//! returned only when it follows from an exact invariant or from an exhaustive search. If neither
//! holds, the result is [`Isomorphism::Unresolved`].
//!
//! Graph isomorphism is not known to be solvable in polynomial time, so an unresolved answer is a
//! real possibility. It is rare in practice, because the sequences make most nodes easy to tell
//! apart.

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

//-----------------------------------------------------------------------------

/// Default number of color refinement rounds in [`Options`].
pub const DEFAULT_REFINEMENT_ROUNDS: usize = 4;

/// Default search budget in [`Options`].
pub const DEFAULT_SEARCH_BUDGET: usize = 1 << 20;

/// Options for [`are_isomorphic`].
///
/// # Examples
///
/// ```
/// use pggname::isomorphism::Options;
///
/// let options = Options { allow_flips: true, ..Options::default() };
/// assert!(options.allow_flips);
/// ```
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Options {
    /// Allow mapping a node to the reverse complement of another node.
    ///
    /// When this is not set, isomorphism preserves orientations. In particular, the reverse
    /// complement of a graph is then generally not isomorphic to the graph itself.
    pub allow_flips: bool,

    /// Maximum number of color refinement rounds.
    ///
    /// Refinement stops early when the coloring becomes stable. More rounds make the search
    /// smaller, but they do not change the answer.
    pub refinement_rounds: usize,

    /// Maximum number of choices the search may make before giving up.
    ///
    /// Zero disables the search, which also avoids storing the undo trail. Any ambiguity then
    /// yields [`Isomorphism::Unresolved`].
    pub search_budget: usize,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            allow_flips: false,
            refinement_rounds: DEFAULT_REFINEMENT_ROUNDS,
            search_budget: DEFAULT_SEARCH_BUDGET,
        }
    }
}

//-----------------------------------------------------------------------------

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

//-----------------------------------------------------------------------------

/// A bijection between the nodes of two graphs.
///
/// Nodes are identified by dense indexes in the [`Topology`] views of the graphs. Each node of the
/// first graph maps to a node of the second graph and a relative orientation.
/// [`Orientation::Forward`] means that the sequences are the same, while [`Orientation::Reverse`]
/// means that the second sequence is the reverse complement of the first.
///
/// The mapping does not borrow the graphs. Use [`Topology::node_name`] to convert the indexes back
/// to node names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeMapping {
    // Image of each node, encoded as `2 * index + flip`.
    mapping: Vec<u32>,
}

impl NodeMapping {
    /// Returns the number of mapped nodes.
    pub fn len(&self) -> usize {
        self.mapping.len()
    }

    /// Returns `true` if the mapping is empty.
    pub fn is_empty(&self) -> bool {
        self.mapping.is_empty()
    }

    /// Returns the image of the given node and its relative orientation.
    pub fn get(&self, node: usize) -> (usize, Orientation) {
        let encoded = self.mapping[node] as usize;
        let orientation = if encoded & 1 == 0 { Orientation::Forward } else { Orientation::Reverse };
        (encoded / 2, orientation)
    }

    /// Returns an iterator over the mapping as `(source, destination, orientation)`.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (usize, usize, Orientation)> {
        self.mapping.iter().enumerate().map(|(source, &encoded)| {
            let encoded = encoded as usize;
            let orientation = if encoded & 1 == 0 { Orientation::Forward } else { Orientation::Reverse };
            (source, encoded / 2, orientation)
        })
    }

    /// Returns `true` if no node maps to the reverse complement of another node.
    pub fn is_forward(&self) -> bool {
        self.mapping.iter().all(|&encoded| encoded & 1 == 0)
    }

    /// Returns the inverse mapping.
    pub fn invert(&self) -> NodeMapping {
        let mut mapping = vec![0; self.mapping.len()];
        for (source, destination, orientation) in self.iter() {
            mapping[destination] = (2 * source + (orientation as usize)) as u32;
        }
        NodeMapping { mapping }
    }

    // Creates a mapping from encoded images.
    pub(crate) fn from_encoded(mapping: Vec<u32>) -> Self {
        NodeMapping { mapping }
    }
}

//-----------------------------------------------------------------------------

/// The result of an isomorphism test.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Isomorphism {
    /// The graphs are isomorphic, with the given verified node mapping.
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
            Isomorphism::NotIsomorphic(_) => write!(f, "not isomorphic"),
            Isomorphism::Unresolved => write!(f, "unresolved"),
        }
    }
}

//-----------------------------------------------------------------------------

/// Statistics on an isomorphism computation.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Statistics {
    /// Number of color refinement rounds.
    pub rounds: usize,
    /// Number of color classes after refinement.
    pub color_classes: usize,
    /// Number of choices made by the search.
    pub individualizations: usize,
}

//-----------------------------------------------------------------------------

/// Determines whether the two graphs are isomorphic.
///
/// A positive answer includes a node mapping that has been verified against the sequences and the
/// edges, so it never depends on hash values. A negative answer is returned only when it follows
/// from an exact invariant or from an exhaustive search. Otherwise the result is
/// [`Isomorphism::Unresolved`].
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
/// // The graph has no automorphisms, so every node maps to itself.
/// assert!(mapping.iter().all(|(source, destination, _)| source == destination));
/// ```
pub fn are_isomorphic<A: Topology, B: Topology>(
    first: &A, second: &B, options: &Options
) -> Isomorphism {
    are_isomorphic_with_statistics(first, second, options).0
}

/// As [`are_isomorphic`], but also returns statistics on the computation.
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
    statistics.individualizations = matcher.choices();

    match outcome {
        Outcome::Complete => {
            let mapping = matcher.into_mapping();
            match verify(first, second, &mapping, options) {
                Ok(()) => (Isomorphism::Isomorphic(mapping), statistics),
                // Every pair passed the local checks, so this can only happen if the choices made
                // along the way were wrong. Without choices, the mapping was the only candidate.
                Err(mismatch) if statistics.individualizations == 0 => {
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

/// The result of a unitig-level isomorphism test.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnitigIsomorphism {
    /// The graphs are equivalent, with the given verified translation.
    Isomorphic(Translation),
    /// The graphs are not equivalent, for the given reason.
    ///
    /// The reason refers to the compacted graphs, where each node is a maximal non-branching path.
    NotIsomorphic(Mismatch),
    /// The question could not be settled.
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
            UnitigIsomorphism::NotIsomorphic(_) => write!(f, "not isomorphic"),
            UnitigIsomorphism::Unresolved => write!(f, "unresolved"),
        }
    }
}

//-----------------------------------------------------------------------------

/// Determines whether two graphs are isomorphic at the level of maximal non-branching paths.
///
/// Two graphs that represent the same pangenome may still differ as graphs, because one of them
/// has chopped long nodes into shorter fragments. Collapsing each maximal non-branching path into
/// a single node removes the difference. A positive answer is a [`Translation`], which maps
/// intervals of nodes rather than whole nodes, because the two graphs cut the paths differently.
///
/// Returns an error if either graph has a connected component that is a cycle with no branches.
/// See [`crate::unitigs`] for why those are not supported.
///
/// # Orientation
///
/// A unitig whose sequence equals its own reverse complement can be stored in either direction, and
/// the two graphs may choose differently. The comparison of the compacted graphs therefore always
/// allows flips, and the resulting translation is verified against `options`. When `allow_flips` is
/// not set and such a unitig exists, the result may be [`UnitigIsomorphism::Unresolved`] even
/// though the graphs are equivalent.
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
/// // They are the same pangenome.
/// let result = isomorphism::are_isomorphic_unitigs(&whole, &chopped, &Options::default()).unwrap();
/// assert!(result.is_isomorphic());
///
/// // Node 1 of the first graph covers both nodes of the second.
/// let translation = result.translation().unwrap();
/// let parts = translation.parts(0);
/// assert_eq!(parts.len(), 2);
/// assert_eq!((parts[0].from, parts[0].node, parts[0].to, parts[0].len), (0, 0, 0, 4));
/// assert_eq!((parts[1].from, parts[1].node, parts[1].to, parts[1].len), (4, 1, 0, 3));
/// ```
pub fn are_isomorphic_unitigs<A: Topology, B: Topology>(
    first: &A, second: &B, options: &Options
) -> Result<UnitigIsomorphism, String> {
    are_isomorphic_unitigs_with_statistics(first, second, options).map(|(result, _)| result)
}

/// As [`are_isomorphic_unitigs`], but also returns statistics on the computation.
pub fn are_isomorphic_unitigs_with_statistics<A: Topology, B: Topology>(
    first: &A, second: &B, options: &Options
) -> Result<(UnitigIsomorphism, Statistics), String> {
    let first_unitigs = Unitigs::new(first)?;
    let second_unitigs = Unitigs::new(second)?;

    // The direction of a unitig whose sequence is its own reverse complement is arbitrary, so the
    // compacted graphs must be compared with flips allowed. For every other unitig, the canonical
    // direction rules a flip out anyway: a flip would require the sequence to equal the reverse
    // complement of another canonical sequence, which forces both to be their own reverse
    // complements.
    let unitig_options = Options { allow_flips: true, ..*options };
    let (result, statistics) = are_isomorphic_with_statistics(
        first_unitigs.graph(), second_unitigs.graph(), &unitig_options
    );

    let result = match result {
        Isomorphism::Isomorphic(mapping) => {
            let translation = translation::expand(&first_unitigs, &second_unitigs, &mapping);
            match translation::verify_translation(first, second, &translation, options) {
                Ok(()) => UnitigIsomorphism::Isomorphic(translation),
                // The translation reverses a unitig that the two graphs stored in opposite
                // directions, which is not allowed here. Another choice might work.
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

//-----------------------------------------------------------------------------

/// Verifies that the mapping is an isomorphism between the two graphs.
///
/// This runs in linear time and does not use any hash values, so it is an independent check of a
/// mapping obtained by any means.
pub fn verify<A: Topology, B: Topology>(
    a: &A, b: &B, mapping: &NodeMapping, options: &Options
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
            Orientation::Reverse => {
                if !options.allow_flips {
                    return Err(Mismatch::Structure);
                }
                hashing::reverse_complement(&sequence) == image.as_ref()
            },
        };
        if !matches {
            return Err(Mismatch::Structure);
        }

        // The neighbors must match, side by side.
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

/// Writes the node mapping in TSV format.
///
/// Each line has the name of a node in the first graph, the name of its image in the second graph,
/// and the relative orientation as `+` or `-`. There is no header line.
pub fn write_mapping<A: Topology, B: Topology, W: Write>(
    first: &A, second: &B, mapping: &NodeMapping, writer: &mut W
) -> io::Result<()> {
    for (source, destination, orientation) in mapping.iter() {
        writer.write_all(&first.node_name(source))?;
        writer.write_all(b"\t")?;
        writer.write_all(&second.node_name(destination))?;
        writer.write_all(b"\t")?;
        writer.write_all(match orientation {
            Orientation::Forward => b"+",
            Orientation::Reverse => b"-",
        })?;
        writer.write_all(b"\n")?;
    }

    Ok(())
}

/// Writes the translation in TSV format.
///
/// Each line has the name of a node in the first graph, the start of the interval in it, the length
/// of the interval, the name of the corresponding node in the second graph, the start of the
/// interval in it, and the relative orientation as `+` or `-`. There is no header line.
pub fn write_translation<A: Topology, B: Topology, W: Write>(
    first: &A, second: &B, translation: &Translation, writer: &mut W
) -> io::Result<()> {
    for node in 0..translation.len() {
        let name = first.node_name(node);
        for part in translation.parts(node).iter() {
            writer.write_all(&name)?;
            write!(writer, "\t{}\t{}\t", part.from, part.len)?;
            writer.write_all(&second.node_name(part.node))?;
            write!(writer, "\t{}\t", part.to)?;
            writer.write_all(match part.orientation {
                Orientation::Forward => b"+",
                Orientation::Reverse => b"-",
            })?;
            writer.write_all(b"\n")?;
        }
    }

    Ok(())
}

//-----------------------------------------------------------------------------

/// Returns the side corresponding to the given side under the given relative orientation.
pub(crate) fn map_side(side: gbz::NodeSide, orientation: Orientation) -> gbz::NodeSide {
    match orientation {
        Orientation::Forward => side,
        Orientation::Reverse => side.flip(),
    }
}

//-----------------------------------------------------------------------------

#[cfg(test)]
mod tests;

//-----------------------------------------------------------------------------
