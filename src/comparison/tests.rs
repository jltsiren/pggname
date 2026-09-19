use super::*;

use crate::test_utils::chop;
use crate::topology::IndexedGraph;

use gbz::Orientation;

//-----------------------------------------------------------------------------

// A path of three nodes, which collapses into a single unitig.
fn base_graph() -> IndexedGraph {
    let mut graph = IndexedGraph::new();
    let a = graph.add_node(b"1", b"GATT");
    let b = graph.add_node(b"2", b"ACA");
    let c = graph.add_node(b"3", b"TTAG");
    graph.add_edge(a, Orientation::Forward, b, Orientation::Forward);
    graph.add_edge(b, Orientation::Forward, c, Orientation::Forward);
    graph.finalize();
    graph
}

// The same graph without the last node.
fn truncated_graph() -> IndexedGraph {
    let mut graph = IndexedGraph::new();
    let a = graph.add_node(b"1", b"GATT");
    let b = graph.add_node(b"2", b"ACA");
    graph.add_edge(a, Orientation::Forward, b, Orientation::Forward);
    graph.finalize();
    graph
}

// A graph with the same shape but different identifiers and sequences.
fn other_graph() -> IndexedGraph {
    let mut graph = IndexedGraph::new();
    let a = graph.add_node(b"4", b"CCCC");
    let b = graph.add_node(b"5", b"GGG");
    let c = graph.add_node(b"6", b"CCCC");
    graph.add_edge(a, Orientation::Forward, b, Orientation::Forward);
    graph.add_edge(b, Orientation::Forward, c, Orientation::Forward);
    graph.finalize();
    graph
}

fn name_of(name: &str) -> GraphName {
    GraphName::new(String::from(name))
}

fn subgraphs(name: &GraphName) -> Vec<(String, String)> {
    name.subgraph_iter().map(|(from, to)| (String::from(from), String::from(to))).collect()
}

fn translations(name: &GraphName) -> Vec<(String, String)> {
    name.translation_iter().map(|(from, to)| (String::from(from), String::from(to))).collect()
}

fn pairs(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs.iter().map(|(from, to)| (String::from(*from), String::from(*to))).collect()
}

//-----------------------------------------------------------------------------

#[test]
fn exit_codes() {
    // These are a documented interface, so they are pinned here rather than derived.
    let verdicts = [
        (Verdict::Same, 0),
        (Verdict::Isomorphic, 0),
        (Verdict::NotIsomorphic(Mismatch::NodeCount), 1),
        (Verdict::Unresolved, 2),
        (Verdict::Subgraph, 3),
        (Verdict::Supergraph, 4),
    ];
    for (verdict, code) in verdicts {
        assert_eq!(verdict.exit_code(), code, "Wrong exit code for {}", verdict);
    }
}

#[test]
fn verdict_display() {
    let verdicts = [
        (Verdict::Same, "same"),
        (Verdict::Subgraph, "subgraph"),
        (Verdict::Supergraph, "supergraph"),
        (Verdict::Isomorphic, "isomorphic"),
        (Verdict::NotIsomorphic(Mismatch::Structure), "not isomorphic"),
        (Verdict::Unresolved, "unresolved"),
    ];
    for (verdict, expected) in verdicts {
        assert_eq!(verdict.to_string(), expected, "Wrong string for {:?}", verdict);
        // The output is written in a column of this width.
        assert!(expected.len() <= 14, "Too long string for {:?}", verdict);
    }
}

//-----------------------------------------------------------------------------

#[test]
fn compare_same_name() {
    let first = base_graph();
    let second = other_graph();
    let name = name_of("shared");
    let options = Options::default();

    // The name is taken at face value, without looking at the graphs.
    let (verdict, translation) = compare(&first, &second, &name, &name, &options).unwrap();
    assert_eq!(verdict, Verdict::Same, "Wrong verdict for graphs with the same name");
    assert!(translation.is_none(), "Got a translation for graphs with the same name");
}

#[test]
fn compare_stale_names() {
    let first = base_graph();
    let second = base_graph();
    let options = Options::default();

    // Mutual containment overrules the names.
    let (verdict, _) = compare(
        &first, &second, &name_of("first"), &name_of("second"), &options
    ).unwrap();
    assert_eq!(verdict, Verdict::Same, "Wrong verdict for identical graphs with different names");
}

#[test]
fn compare_subgraph() {
    let large = base_graph();
    let small = truncated_graph();
    let large_name = name_of("large");
    let small_name = name_of("small");
    let options = Options::default();

    let (verdict, translation) = compare(
        &small, &large, &small_name, &large_name, &options
    ).unwrap();
    assert_eq!(verdict, Verdict::Subgraph, "Wrong verdict for a subgraph");
    assert!(translation.is_none(), "Got a translation for a subgraph");

    let (verdict, _) = compare(&large, &small, &large_name, &small_name, &options).unwrap();
    assert_eq!(verdict, Verdict::Supergraph, "Wrong verdict for a supergraph");
}

#[test]
fn compare_isomorphic() {
    let graph = base_graph();
    // Chopping renames the nodes, so this is not a subgraph.
    let chopped = chop(&graph, 2);
    let options = Options::default();

    let (verdict, translation) = compare(
        &graph, &chopped, &name_of("graph"), &name_of("chopped"), &options
    ).unwrap();
    assert_eq!(verdict, Verdict::Isomorphic, "Wrong verdict for a chopped graph");
    assert!(translation.is_some(), "No translation for a chopped graph");
}

#[test]
fn compare_unrelated() {
    let first = base_graph();
    let second = other_graph();
    let options = Options::default();

    let (verdict, translation) = compare(
        &first, &second, &name_of("first"), &name_of("second"), &options
    ).unwrap();
    assert!(
        matches!(verdict, Verdict::NotIsomorphic(_)),
        "Wrong verdict for unrelated graphs: {}", verdict
    );
    assert!(translation.is_none(), "Got a translation for unrelated graphs");
}

//-----------------------------------------------------------------------------

#[test]
fn update_same() {
    let mut first = name_of("first");
    first.add_subgraph("first", "parent");
    let mut second = name_of("second");
    second.add_translation("second", "other");

    update_relationships(Verdict::Same, &mut first, &mut second);

    // Both end up with the union, and no new relationships are created.
    assert_eq!(subgraphs(&first), pairs(&[("first", "parent")]), "Wrong subgraphs for the first graph");
    assert_eq!(translations(&first), pairs(&[("second", "other")]), "Wrong translations for the first graph");
    assert_eq!(subgraphs(&second), subgraphs(&first), "The subgraph relationships disagree");
    assert_eq!(translations(&second), translations(&first), "The translation relationships disagree");
}

#[test]
fn update_subgraph() {
    let mut first = name_of("first");
    let mut second = name_of("second");
    second.add_subgraph("second", "parent");
    let original = second.clone();

    update_relationships(Verdict::Subgraph, &mut first, &mut second);

    assert_eq!(
        subgraphs(&first), pairs(&[("first", "second"), ("second", "parent")]),
        "Wrong subgraphs for the subgraph"
    );
    assert!(first.is_subgraph_of(&second), "The subgraph relationship was not stored");
    assert!(first.is_subgraph_of(&name_of("parent")), "The inherited relationship was not copied");
    assert_eq!(second, original, "The supergraph was modified");
}

#[test]
fn update_supergraph() {
    let mut first = name_of("first");
    let mut second = name_of("second");
    let original = first.clone();

    update_relationships(Verdict::Supergraph, &mut first, &mut second);

    assert_eq!(subgraphs(&second), pairs(&[("second", "first")]), "Wrong subgraphs for the subgraph");
    assert!(second.is_subgraph_of(&first), "The subgraph relationship was not stored");
    assert_eq!(first, original, "The supergraph was modified");
}

#[test]
fn update_isomorphic() {
    let mut first = name_of("first");
    let mut second = name_of("second");

    update_relationships(Verdict::Isomorphic, &mut first, &mut second);

    let expected = pairs(&[("first", "second"), ("second", "first")]);
    assert_eq!(translations(&first), expected, "Wrong translations for the first graph");
    assert_eq!(translations(&second), expected, "Wrong translations for the second graph");
    assert!(first.translates_to(&second), "The forward translation is missing");
    assert!(second.translates_to(&first), "The reverse translation is missing");
}

#[test]
fn update_isomorphic_inherited() {
    let mut first = name_of("first");
    first.add_subgraph("first", "parent");
    let mut second = name_of("second");
    second.add_translation("second", "other");

    update_relationships(Verdict::Isomorphic, &mut first, &mut second);

    // Both graphs know everything that either of them knew.
    assert_eq!(subgraphs(&second), subgraphs(&first), "The subgraph relationships disagree");
    assert_eq!(translations(&second), translations(&first), "The translation relationships disagree");
    assert_eq!(subgraphs(&first), pairs(&[("first", "parent")]), "Wrong subgraphs");
    assert_eq!(
        translations(&first),
        pairs(&[("first", "second"), ("second", "first"), ("second", "other")]),
        "Wrong translations"
    );
}

#[test]
fn update_unnamed() {
    // Every mutator in `GraphName` is a no-op if either graph has no name.
    for verdict in [Verdict::Same, Verdict::Subgraph, Verdict::Supergraph, Verdict::Isomorphic] {
        let mut first = name_of("first");
        let mut second = GraphName::default();
        update_relationships(verdict, &mut first, &mut second);
        assert_eq!(first, name_of("first"), "The first name changed for {}", verdict);
        assert_eq!(second, GraphName::default(), "The second name changed for {}", verdict);
    }
}

#[test]
fn update_unrelated() {
    let verdicts = [Verdict::NotIsomorphic(Mismatch::Colors), Verdict::Unresolved];
    for verdict in verdicts {
        let mut first = name_of("first");
        let mut second = name_of("second");
        update_relationships(verdict, &mut first, &mut second);
        assert_eq!(first, name_of("first"), "The first name changed for {}", verdict);
        assert_eq!(second, name_of("second"), "The second name changed for {}", verdict);
    }
}

//-----------------------------------------------------------------------------
