//! Determining and recording the relationship between two pangenome graphs.
//!
//! Two graphs may be the same graph, one may be contained in the other, or they may represent the
//! same pangenome with different node identifiers. [`compare`] tries these in order and returns
//! the strongest relationship it finds as a [`Verdict`].
//!
//! The relationships are also the ones that [`GraphName`] can store in GBZ tags and GFA header
//! lines. [`update_relationships`] records a verdict in the names of the two graphs.

use crate::isomorphism::{self, Mismatch, Options, UnitigIsomorphism};
use crate::isomorphism::translation::Translation;
use crate::topology::{self, Topology};

use gbz::GraphName;

use std::fmt;

#[cfg(test)]
mod tests;

//-----------------------------------------------------------------------------

/// The relationship between two graphs.
///
/// [`Verdict::Subgraph`] and [`Verdict::Supergraph`] are relative to the first graph.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// The graphs are the same graph, with the same node identifiers.
    Same,
    /// The first graph is a subgraph of the second graph.
    Subgraph,
    /// The second graph is a subgraph of the first graph.
    Supergraph,
    /// The graphs are isomorphic at the level of maximal non-branching paths.
    Isomorphic,
    /// The graphs are unrelated, for the given reason.
    ///
    /// The reason refers to the compacted graphs, where each node is a maximal non-branching path.
    NotIsomorphic(Mismatch),
    /// The question could not be settled.
    Unresolved,
}

impl Verdict {
    /// Returns the process exit code corresponding to the verdict.
    ///
    /// The graphs represent the same pangenome if and only if the code is 0.
    pub fn exit_code(&self) -> i32 {
        match self {
            Verdict::Same => 0,
            Verdict::Isomorphic => 0,
            Verdict::NotIsomorphic(_) => 1,
            Verdict::Unresolved => 2,
            Verdict::Subgraph => 3,
            Verdict::Supergraph => 4,
        }
    }
}

impl fmt::Display for Verdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Verdict::Same => write!(f, "same"),
            Verdict::Subgraph => write!(f, "subgraph"),
            Verdict::Supergraph => write!(f, "supergraph"),
            Verdict::Isomorphic => write!(f, "isomorphic"),
            Verdict::NotIsomorphic(_) => write!(f, "not isomorphic"),
            Verdict::Unresolved => write!(f, "unresolved"),
        }
    }
}

//-----------------------------------------------------------------------------

/// Determines the relationship between the two graphs.
///
/// The checks are made in increasing order of cost, and the first one that succeeds gives the
/// answer:
///
/// 1. The graphs have the same name.
/// 2. One graph is a subgraph of the other (see [`topology::is_subgraph`]).
/// 3. The graphs are isomorphic at the level of maximal non-branching paths (see
///    [`isomorphism::are_isomorphic_unitigs`]).
///
/// A translation is returned only in case 3, as the other cases are answered without computing
/// one. Returns an error if either graph has a connected component that is a cycle with no
/// branches, as in [`isomorphism::are_isomorphic_unitigs`].
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
        return Ok((Verdict::Same, None));
    }

    if topology::is_subgraph(first, second) {
        // Mutual containment means the same canonical GFA and hence the same name. If the names
        // say otherwise, at least one of them was stale, and the graphs win.
        let same = first.nodes() == second.nodes() && first.edges() == second.edges();
        let verdict = if same { Verdict::Same } else { Verdict::Subgraph };
        return Ok((verdict, None));
    }
    if topology::is_subgraph(second, first) {
        return Ok((Verdict::Supergraph, None));
    }

    let result = isomorphism::are_isomorphic_unitigs(first, second, options)?;
    Ok(match result {
        UnitigIsomorphism::Isomorphic(translation) => (Verdict::Isomorphic, Some(translation)),
        UnitigIsomorphism::NotIsomorphic(mismatch) => (Verdict::NotIsomorphic(mismatch), None),
        UnitigIsomorphism::Unresolved => (Verdict::Unresolved, None),
    })
}

//-----------------------------------------------------------------------------

/// Records the relationship between the two graphs in their names.
///
/// [`Verdict::Subgraph`] and [`Verdict::Supergraph`] update the name of the subgraph only, because
/// the relationship is not symmetric and the supergraph knows nothing new. The other relationships
/// are symmetric, and both names end up with the union of the relationships known to either.
///
/// Does nothing if the verdict is [`Verdict::NotIsomorphic`] or [`Verdict::Unresolved`], or if
/// either graph has no name.
///
/// # Examples
///
/// ```
/// use pggname::comparison::{self, Verdict};
/// use gbz::GraphName;
///
/// let mut first = GraphName::new(String::from("first"));
/// let mut second = GraphName::new(String::from("second"));
/// comparison::update_relationships(Verdict::Subgraph, &mut first, &mut second);
///
/// assert!(first.is_subgraph_of(&second));
/// let relationships: Vec<(&str, &str)> = first.subgraph_iter().collect();
/// assert_eq!(relationships, vec![("first", "second")]);
/// // The supergraph is left alone.
/// assert_eq!(second, GraphName::new(String::from("second")));
/// ```
pub fn update_relationships(verdict: Verdict, first: &mut GraphName, second: &mut GraphName) {
    match verdict {
        // The order matters: the second call copies the already merged relationships back.
        Verdict::Same => {
            first.add_relationships(second);
            second.add_relationships(first);
        },
        Verdict::Subgraph => first.make_subgraph_of(second),
        Verdict::Supergraph => second.make_subgraph_of(first),
        // The order matters here as well. The second call must see the updated `first`, so that
        // `second` learns about the forward translation, and the third call must come last, so
        // that `first` learns about the reverse translation. Two calls cannot do this.
        Verdict::Isomorphic => {
            first.add_translation_to(second);
            second.add_translation_to(first);
            first.add_relationships(second);
        },
        Verdict::NotIsomorphic(_) => (),
        Verdict::Unresolved => (),
    }
}

//-----------------------------------------------------------------------------
