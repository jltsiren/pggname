//! Relationships between pangenome graphs.
//!
//! The relationship between two graphs implementing [`Topology`] can be determined using [`compare`].
//! The result of a comparison is returned as a [`Verdict`].
//! Given a comparison result, the [`GraphName`] objects can be updated with [`update_relationships`].

use crate::isomorphism::{self, Options, UnitigIsomorphism};
use crate::isomorphism::translation::Translation;
use crate::topology::{self, Topology};

use gbz::GraphName;

use std::fmt;

#[cfg(test)]
mod tests;

//-----------------------------------------------------------------------------

/// The relationship between two graphs.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Same graph.
    Same,
    /// The first graph is a proper subgraph of the second.
    Subgraph,
    /// The second graph is a proper subgraph of the first.
    Supergraph,
    /// The graphs are isomorphic at the level of maximal non-branching paths.
    Isomorphic,
    /// The graphs are unrelated.
    Unrelated,
    /// The question could not be settled.
    Unresolved,
}

impl fmt::Display for Verdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Verdict::Same => write!(f, "same"),
            Verdict::Subgraph => write!(f, "subgraph"),
            Verdict::Supergraph => write!(f, "supergraph"),
            Verdict::Isomorphic => write!(f, "isomorphic"),
            Verdict::Unrelated => write!(f, "unrelated"),
            Verdict::Unresolved => write!(f, "unresolved"),
        }
    }
}

//-----------------------------------------------------------------------------

/// Determines the relationship between the two graphs.
///
/// The checks are made in increasing order of cost, and the first one that succeeds gives the answer:
///
/// 1. Graph names are defined and the same.
/// 2. One graph is a subgraph of the other (see [`topology::is_subgraph`]).
/// 3. The graphs are isomorphic at the level of maximal non-branching paths (see [`isomorphism::are_isomorphic_unitigs`]).
///
/// If the graphs are isomorphic, a [`Translation`] between the unitigs will be returned.
///
/// # Arguments
///
/// * `first`: The first graph to compare.
/// * `second`: The second graph to compare.
/// * `first_name`: The name of the first graph (may be empty).
/// * `second_name`: The name of the second graph (may be empty).
/// * `options`: Options for the isomorphism comparison.
///
/// # Examples
///
/// ```
/// use pggname::comparison::{self, Verdict};
/// use pggname::isomorphism::Options;
/// use pggname::topology::IndexedGraph;
/// use gbz::{GraphName, Orientation};
///
/// let mut large = IndexedGraph::new();
/// let a = large.add_node(b"1", b"GAT");
/// let b = large.add_node(b"2", b"TA");
/// large.add_edge(a, Orientation::Forward, b, Orientation::Forward);
/// large.finalize();
///
/// let mut small = IndexedGraph::new();
/// small.add_node(b"1", b"GAT");
/// small.finalize();
///
/// let large_name = GraphName::new(String::from("large"));
/// let small_name = GraphName::new(String::from("small"));
/// let options = Options::default();
///
/// let (verdict, translation) = comparison::compare(
///     &small, &large, &small_name, &large_name, &options
/// ).unwrap();
/// assert_eq!(verdict, Verdict::Subgraph);
/// assert!(translation.is_none());
///
/// let (verdict, _) = comparison::compare(
///     &large, &small, &large_name, &small_name, &options
/// ).unwrap();
/// assert_eq!(verdict, Verdict::Supergraph);
/// ```
pub fn compare<A: Topology, B: Topology>(
    first: &A, second: &B,
    first_name: &GraphName, second_name: &GraphName,
    options: &Options
) -> Result<(Verdict, Option<Translation>), String> {
    if first_name.is_same(second_name) {
        // NOTE: Here we assume that name collisions do not happen in practice.
        // This should be true for the SHA-256 names used in the actual naming scheme.
        return Ok((Verdict::Same, None));
    }

    if topology::is_subgraph(first, second) {
        // With missing graph names, we may learn here that the graphs are the same.
        let same = first.nodes() == second.nodes() && first.edges() == second.edges();
        let verdict = if same { Verdict::Same } else { Verdict::Subgraph };
        return Ok((verdict, None));
    }
    if topology::is_subgraph(second, first) {
        return Ok((Verdict::Supergraph, None));
    }

    let result = isomorphism::are_isomorphic_unitigs(first, second, options)?; // FIXME: rename
    Ok(match result {
        UnitigIsomorphism::Isomorphic(translation) => (Verdict::Isomorphic, Some(translation)),
        UnitigIsomorphism::NotIsomorphic(_) => (Verdict::Unrelated, None), // FIXME: do we need the mismatch?
        UnitigIsomorphism::Unresolved => (Verdict::Unresolved, None),
    })
}

//-----------------------------------------------------------------------------

/// Updates the relationships between the two graphs.
///
/// Does nothing if either graph has no name.
/// If the relationship is symmetric ([`Verdict::Same`] or [`Verdict::Isomorphic`]), copies existing relationships in both directions.
/// For asymmetric relationships ([`Verdict::Subgraph`] and [`Verdict::Supergraph`]), the name of the parent graph remains unchanged.
///
/// # Examples
///
/// ```
/// use pggname::comparison::{self, Verdict};
/// use gbz::GraphName;
///
/// let mut subgraph = GraphName::new(String::from("first"));
/// let mut supergraph = GraphName::new(String::from("second"));
/// comparison::update_relationships(Verdict::Subgraph, &mut subgraph, &mut supergraph);
/// assert!(subgraph.is_subgraph_of(&supergraph));
///
/// // Supergraph name remains unchanged.
/// let copy = GraphName::new(String::from("second"));
/// assert_eq!(supergraph, copy);
/// ```
pub fn update_relationships(verdict: Verdict, first: &mut GraphName, second: &mut GraphName) {
    if !first.has_name() || !second.has_name() {
        return;
    }

    match verdict {
        Verdict::Same => {
            first.add_relationships(second);
            second.add_relationships(first);
        },
        Verdict::Subgraph => first.make_subgraph_of(second),
        Verdict::Supergraph => second.make_subgraph_of(first),
        Verdict::Isomorphic => {
            first.add_translation_to(second);
            second.add_translation_to(first);
            // We still need to copy the translation relationship from `first` to `second`.
            first.add_relationships(second);
        },
        Verdict::Unrelated => (),
        Verdict::Unresolved => (),
    }
}

//-----------------------------------------------------------------------------
