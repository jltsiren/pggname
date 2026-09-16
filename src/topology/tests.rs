use super::*;

use crate::Graph;
use crate::algorithms;
use crate::graph::{GBZStr, GraphInt, GraphStr};

use gbz::GBZ;
use simple_sds::serialize;

use std::fs::OpenOptions;
use std::io::BufReader;

//-----------------------------------------------------------------------------

fn gfa_graph<G: Graph>(filename: &'static str) -> G {
    let path = support::get_test_data(filename);
    let file = OpenOptions::new().read(true).open(&path).unwrap();
    algorithms::parse_gfa(BufReader::new(file)).unwrap()
}

fn gbz_graph(filename: &'static str) -> GBZ {
    let path = support::get_test_data(filename);
    serialize::load_from(&path).unwrap()
}

fn parse_gfa_text<G: Graph>(text: &str) -> G {
    algorithms::parse_gfa(BufReader::new(text.as_bytes())).unwrap()
}

// Rebuilds a graph from the topological view alone.
//
// If the view preserves the graph exactly, the rebuilt graph has the same stable name as the
// original. This reuses the canonical serialization as an independent check of the view.
fn rebuild<T: Topology>(source: &T) -> GraphStr {
    let mut result = GraphStr::new();
    for node in 0..source.nodes() {
        result.add_node(&source.node_name(node), &source.sequence(node)).unwrap();
    }
    for node in 0..source.nodes() {
        let name = source.node_name(node);
        for side in [NodeSide::Left, NodeSide::Right] {
            for (neighbor, neighbor_side) in source.neighbors(node, side) {
                result.add_edge(
                    &name, support::exit_orientation(side),
                    &source.node_name(neighbor), support::entry_orientation(neighbor_side)
                ).unwrap();
            }
        }
    }
    result.finalize().unwrap();
    result
}

//-----------------------------------------------------------------------------

#[test]
fn indexed_graph_round_trip() {
    for filename in ["example.gfa", "translation.gfa"] {
        let original: GraphStr = gfa_graph(filename);
        let indexed = IndexedGraph::from(&original);
        assert_eq!(indexed.statistics(), original.statistics(), "Wrong statistics for {}", filename);
        assert_eq!(
            stable_name_of(&rebuild(&indexed)), stable_name_of(&original),
            "IndexedGraph round trip changed the graph for {}", filename
        );

        // A second round trip through `from_topology` must also be stable.
        let again = IndexedGraph::from_topology(&indexed);
        assert_eq!(again, indexed, "from_topology is not idempotent for {}", filename);
    }
}

#[test]
fn gbz_topology_round_trip() {
    for filename in ["example.gbz", "translation.gbz", "micb-kir3dl1.gbz"] {
        let gbz = gbz_graph(filename);
        let topology = GbzTopology::new(&gbz).unwrap();
        let expected = GBZStr { graph: gbz.clone() };
        assert_eq!(topology.statistics(), expected.statistics(), "Wrong statistics for {}", filename);
        assert_eq!(
            stable_name_of(&rebuild(&topology)), crate::stable_name(&expected),
            "GbzTopology round trip changed the graph for {}", filename
        );
    }
}

#[test]
fn backends_agree() {
    // The same graph as GFA with integer ids, as GFA with string ids, and as GBZ.
    let from_int = IndexedGraph::from(&gfa_graph::<GraphInt>("example.gfa"));
    let from_str = IndexedGraph::from(&gfa_graph::<GraphStr>("example.gfa"));
    let gbz = gbz_graph("example.gbz");
    let from_gbz = GbzTopology::new(&gbz).unwrap();

    assert_eq!(from_int, from_str, "GraphInt and GraphStr views differ");
    assert_eq!(from_int.statistics(), from_gbz.statistics(), "GFA and GBZ views differ in statistics");
    assert_eq!(
        stable_name_of(&rebuild(&from_int)), stable_name_of(&rebuild(&from_gbz)),
        "GFA and GBZ views describe different graphs"
    );

    // The views must agree node by node, as the node order is the same in both.
    for node in 0..from_int.nodes() {
        assert_eq!(from_int.node_name(node), from_gbz.node_name(node), "Wrong name for node {}", node);
        assert_eq!(from_int.sequence(node), from_gbz.sequence(node), "Wrong sequence for node {}", node);
        for side in [NodeSide::Left, NodeSide::Right] {
            assert_eq!(
                from_int.degree(node, side), from_gbz.degree(node, side),
                "Wrong degree for node {}, side {:?}", node, side
            );
            let mut int_neighbors: Vec<_> = from_int.neighbors(node, side).collect();
            let mut gbz_neighbors: Vec<_> = from_gbz.neighbors(node, side).collect();
            int_neighbors.sort_unstable();
            gbz_neighbors.sort_unstable();
            assert_eq!(
                int_neighbors, gbz_neighbors,
                "Wrong neighbors for node {}, side {:?}", node, side
            );
        }
    }
}

//-----------------------------------------------------------------------------

// All three kinds of self-loop. The last two are their own reverse and therefore appear in the
// neighbor list of one side only, which is what makes `sum_of_degrees / 2` the wrong edge count.
const SELF_LOOPS: &str = "\
S\t1\tGAT\n\
S\t2\tTA\n\
S\t3\tCAG\n\
L\t1\t+\t1\t+\n\
L\t2\t+\t2\t-\n\
L\t3\t-\t3\t+\n";

#[test]
fn self_loops() {
    let original: GraphStr = parse_gfa_text(SELF_LOOPS);
    let indexed = IndexedGraph::from(&original);

    // Three distinct edges, even though the degrees sum to four.
    assert_eq!(original.statistics(), (3, 3, 8), "Wrong statistics for the original graph");
    assert_eq!(indexed.statistics(), (3, 3, 8), "Wrong statistics for the indexed graph");

    let degrees = [(1, 1), (0, 1), (1, 0)];
    let mut degree_sum = 0;
    for (node, (left, right)) in degrees.iter().enumerate() {
        assert_eq!(indexed.degree(node, NodeSide::Left), *left, "Wrong left degree for node {}", node);
        assert_eq!(indexed.degree(node, NodeSide::Right), *right, "Wrong right degree for node {}", node);
        degree_sum += left + right;
    }
    assert_eq!(degree_sum, 4, "Wrong degree sum");
    assert_ne!(degree_sum / 2, indexed.edges(), "The degree sum should not determine the edge count");

    // `(1, +) -> (1, +)` joins the two sides of node 1, so each side sees the other.
    assert_eq!(
        indexed.neighbors(0, NodeSide::Left).collect::<Vec<_>>(), vec![(0, NodeSide::Right)],
        "Wrong neighbors for the left side of node 1"
    );
    assert_eq!(
        indexed.neighbors(0, NodeSide::Right).collect::<Vec<_>>(), vec![(0, NodeSide::Left)],
        "Wrong neighbors for the right side of node 1"
    );
    // `(2, +) -> (2, -)` is a loop on the right side of node 2, listed exactly once.
    assert_eq!(
        indexed.neighbors(1, NodeSide::Right).collect::<Vec<_>>(), vec![(1, NodeSide::Right)],
        "Wrong neighbors for the right side of node 2"
    );
    // `(3, -) -> (3, +)` is a loop on the left side of node 3, listed exactly once.
    assert_eq!(
        indexed.neighbors(2, NodeSide::Left).collect::<Vec<_>>(), vec![(2, NodeSide::Left)],
        "Wrong neighbors for the left side of node 3"
    );

    assert_eq!(
        stable_name_of(&rebuild(&indexed)), stable_name_of(&original),
        "Self-loop round trip changed the graph"
    );
}

#[test]
fn duplicate_edges() {
    // Duplicate `L` lines do not create parallel edges.
    let with_duplicates: GraphStr = parse_gfa_text(
        "S\t1\tGAT\nS\t2\tTA\nL\t1\t+\t2\t+\nL\t1\t+\t2\t+\nL\t2\t-\t1\t-\n"
    );
    let without: GraphStr = parse_gfa_text("S\t1\tGAT\nS\t2\tTA\nL\t1\t+\t2\t+\n");
    assert_eq!(
        IndexedGraph::from(&with_duplicates), IndexedGraph::from(&without),
        "Duplicate edges changed the graph"
    );
}

#[test]
fn empty_graph() {
    let graph = IndexedGraph::new();
    assert_eq!(graph.statistics(), (0, 0, 0), "Wrong statistics for an empty graph");
    assert_eq!(IndexedGraph::from_topology(&graph), graph, "Round trip changed the empty graph");
}

//-----------------------------------------------------------------------------

fn stable_name_of<G: Graph>(graph: &G) -> String {
    crate::stable_name(graph)
}

//-----------------------------------------------------------------------------
