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

fn as_graph_name(name: &str) -> GraphName {
    GraphName::new(String::from(name))
}

fn subgraphs(name: &GraphName) -> Vec<(String, String)> {
    name.subgraph_iter().map(|(from, to)| (String::from(from), String::from(to))).collect()
}

fn translations(name: &GraphName) -> Vec<(String, String)> {
    name.translation_iter().map(|(from, to)| (String::from(from), String::from(to))).collect()
}

fn compare_graph_names(name: &GraphName, truth: &GraphName, label: &str) {
    assert_eq!(name.name(), truth.name(), "Wrong graph name for {}", label);

    let name_subgraphs = subgraphs(name);
    let truth_subgraphs = subgraphs(truth);
    assert_eq!(name_subgraphs.len(), truth_subgraphs.len(), "Wrong number of subgraph relationships for {}", label);
    for (i, (name_subgraph, truth_subgraph)) in name_subgraphs.iter().zip(truth_subgraphs.iter()).enumerate() {
        assert_eq!(name_subgraph, truth_subgraph, "Wrong subgraph relationship at index {} for {}", i, label);
    }

    let name_translations = translations(name);
    let truth_translations = translations(truth);
    assert_eq!(name_translations.len(), truth_translations.len(), "Wrong number of translation relationships for {}", label);
    for (i, (name_translation, truth_translation)) in name_translations.iter().zip(truth_translations.iter()).enumerate() {
        assert_eq!(name_translation, truth_translation, "Wrong translation relationship at index {} for {}", i, label);
    }
}

//-----------------------------------------------------------------------------

#[test]
fn compare_same_name() {
    let first = base_graph();
    let second = other_graph();
    let name = as_graph_name("shared");
    let options = Options::default();

    // The name is taken at face value, without looking at the graphs.
    let (verdict, translation) = compare(&first, &second, &name, &name, &options).unwrap();
    assert_eq!(verdict, Verdict::Same, "Wrong verdict for graphs with the same name");
    assert!(translation.is_none(), "Got a translation for graphs with the same name");
}

#[test]
fn compare_missing_names() {
    let first = base_graph();
    let second = base_graph();
    let name = as_graph_name("name");
    let missing_name = GraphName::default();
    let options = Options::default();

    // Both names are missing.
    let (verdict, _) = compare(&first, &second, &missing_name, &missing_name, &options).unwrap();
    assert_eq!(verdict, Verdict::Same, "Wrong verdict for identical graphs with missing names");

    // First name is missing.
    let (verdict, _) = compare(&first, &second, &missing_name, &name, &options).unwrap();
    assert_eq!(verdict, Verdict::Same, "Wrong verdict for identical graphs with the first name missing");

    // Second name is missing.
    let (verdict, _) = compare(&first, &second, &name, &missing_name, &options).unwrap();
    assert_eq!(verdict, Verdict::Same, "Wrong verdict for identical graphs with the second name missing");
}

#[test]
fn compare_subgraph() {
    let large = base_graph();
    let small = truncated_graph();
    let large_name = as_graph_name("large");
    let small_name = as_graph_name("small");
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
    let graph_name = as_graph_name("graph");
    let chopped_name = as_graph_name("chopped");
    let options = Options::default();

    let (verdict, translation) = compare(
        &graph, &chopped, &graph_name, &chopped_name, &options
    ).unwrap();
    assert_eq!(verdict, Verdict::Isomorphic, "Wrong verdict for a chopped graph");
    assert!(translation.is_some(), "No translation for a chopped graph");
}

#[test]
fn compare_unrelated() {
    let first = base_graph();
    let second = other_graph();
    let first_name = as_graph_name("first");
    let second_name = as_graph_name("second");
    let options = Options::default();

    let (verdict, translation) = compare(
        &first, &second, &first_name, &second_name, &options
    ).unwrap();
    assert!(
        matches!(verdict, Verdict::Unrelated),
        "Wrong verdict for unrelated graphs: {}", verdict
    );
    assert!(translation.is_none(), "Got a translation for unrelated graphs");
}

//-----------------------------------------------------------------------------

#[test]
fn update_same() {
    let mut first = as_graph_name("graph");
    first.add_subgraph("graph", "parent");
    let mut second = as_graph_name("graph");
    second.add_translation("graph", "other");

    let mut truth = as_graph_name("graph");
    truth.add_subgraph("graph", "parent");
    truth.add_translation("graph", "other");

    update_relationships(Verdict::Same, &mut first, &mut second);
    compare_graph_names(&first, &truth, "first");
    compare_graph_names(&second, &truth, "second");
}

#[test]
fn update_subgraph() {
    let mut first = as_graph_name("first");
    let mut first_truth = first.clone();
    let mut second = as_graph_name("second");
    second.add_subgraph("second", "parent");
    let second_truth = second.clone();

    first_truth.add_subgraph("first", "second");
    first_truth.add_subgraph("second", "parent");

    update_relationships(Verdict::Subgraph, &mut first, &mut second);
    compare_graph_names(&first, &first_truth, "first");
    compare_graph_names(&second, &second_truth, "second");
}

#[test]
fn update_supergraph() {
    let mut first = as_graph_name("first");
    first.add_subgraph("first", "parent");
    let first_truth = first.clone();
    let mut second = as_graph_name("second");
    let mut second_truth = second.clone();

    second_truth.add_subgraph("second", "first");
    second_truth.add_subgraph("first", "parent");

    update_relationships(Verdict::Supergraph, &mut first, &mut second);
    compare_graph_names(&first, &first_truth, "first");
    compare_graph_names(&second, &second_truth, "second");
}

#[test]
fn update_isomorphic() {
    let mut first = as_graph_name("first");
    let mut first_truth = first.clone();
    let mut second = as_graph_name("second");
    let mut second_truth = second.clone();

    first_truth.add_translation("first", "second");
    first_truth.add_translation("second", "first");
    second_truth.add_translation("first", "second");
    second_truth.add_translation("second", "first");

    update_relationships(Verdict::Isomorphic, &mut first, &mut second);
    compare_graph_names(&first, &first_truth, "first");
    compare_graph_names(&second, &second_truth, "second");
}

#[test]
fn update_isomorphic_inherited() {
    let mut first = as_graph_name("first");
    first.add_subgraph("first", "parent");
    let mut first_truth = first.clone();
    let mut second = as_graph_name("second");
    let mut second_truth = second.clone();

    first_truth.add_translation("first", "second");
    first_truth.add_translation("second", "first");
    second_truth.add_translation("first", "second");
    second_truth.add_translation("second", "first");
    second_truth.add_subgraph("first", "parent");

    update_relationships(Verdict::Isomorphic, &mut first, &mut second);
    compare_graph_names(&first, &first_truth, "first");
    compare_graph_names(&second, &second_truth, "second");
}

#[test]
fn update_unnamed() {
    let mut named = as_graph_name("named");
    named.add_subgraph("named", "parent");
    let named_truth = named.clone();
    let mut unnamed = GraphName::default();
    let unnamed_truth = unnamed.clone();

    for verdict in [Verdict::Same, Verdict::Subgraph, Verdict::Supergraph, Verdict::Isomorphic] {
        update_relationships(verdict, &mut unnamed, &mut named);
        let label = format!("({}, first, first unnamed)", verdict);
        compare_graph_names(&named, &named_truth, &label);
        let label = format!("({}, second, first unnamed)", verdict);
        compare_graph_names(&unnamed, &unnamed_truth, &label);

        update_relationships(verdict, &mut named, &mut unnamed);
        let label = format!("({}, first, second unnamed)", verdict);
        compare_graph_names(&named, &named_truth, &label);
        let label = format!("({}, second, second unnamed)", verdict);
        compare_graph_names(&unnamed, &unnamed_truth, &label);
    }
}

#[test]
fn update_unrelated() {
    let mut first = as_graph_name("first");
    first.add_translation("first", "original");
    let first_truth = first.clone();
    let mut second = as_graph_name("second");
    second.add_subgraph("second", "parent");
    let second_truth = second.clone();

    for verdict in [Verdict::Unrelated, Verdict::Unresolved] {
        update_relationships(verdict, &mut first, &mut second);
        let label = format!("({}, first)", verdict);
        compare_graph_names(&first, &first_truth, &label);
        let label = format!("({}, second)", verdict);
        compare_graph_names(&second, &second_truth, &label);
    }
}

//-----------------------------------------------------------------------------
