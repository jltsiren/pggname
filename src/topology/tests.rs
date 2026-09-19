use super::*;

use crate::Graph;
use crate::algorithms;
use crate::graph::{GBZStr, GraphInt, GraphStr};
use crate::test_utils::*;

use gbz::GBZ;
use rand::SeedableRng;
use rand::rngs::StdRng;
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

fn indexed_from_text(text: &str) -> IndexedGraph {
    IndexedGraph::from(&parse_gfa_text::<GraphStr>(text))
}

// A path of three nodes.
const PATH: &str = "\
S\t1\tGATT\n\
S\t2\tACA\n\
S\t3\tTTAG\n\
L\t1\t+\t2\t+\n\
L\t2\t+\t3\t+\n";

#[test]
fn subgraph_identical() {
    let graph = indexed_from_text(PATH);
    let copy = indexed_from_text(PATH);
    assert!(is_subgraph(&graph, &copy), "A graph is not a subgraph of its copy");
    assert!(is_subgraph(&copy, &graph), "A copy is not a subgraph of the graph");
    assert!(is_subgraph(&graph, &graph), "A graph is not a subgraph of itself");
}

#[test]
fn subgraph_node_subset() {
    let graph = indexed_from_text(PATH);
    // The same graph without node 3 and the edge to it.
    let subset = indexed_from_text("S\t1\tGATT\nS\t2\tACA\nL\t1\t+\t2\t+\n");
    assert!(is_subgraph(&subset, &graph), "A node subset is not a subgraph");
    assert!(!is_subgraph(&graph, &subset), "A graph is a subgraph of a node subset");
}

#[test]
fn subgraph_edge_subset() {
    let graph = indexed_from_text(PATH);
    // The same nodes, with one edge missing.
    let subset = indexed_from_text("S\t1\tGATT\nS\t2\tACA\nS\t3\tTTAG\nL\t1\t+\t2\t+\n");
    assert!(is_subgraph(&subset, &graph), "An edge subset is not a subgraph");
    assert!(!is_subgraph(&graph, &subset), "A graph is a subgraph of an edge subset");
}

#[test]
fn subgraph_different_labels() {
    let graph = indexed_from_text(PATH);

    // A different sequence for node 2.
    let sequence = indexed_from_text("S\t1\tGATT\nS\t2\tACC\nL\t1\t+\t2\t+\n");
    assert!(!is_subgraph(&sequence, &graph), "A different sequence is a subgraph");

    // Node 2 renamed. The graph is the same, but the identifiers are not.
    let renamed = indexed_from_text("S\t1\tGATT\nS\t4\tACA\nL\t1\t+\t4\t+\n");
    assert!(!is_subgraph(&renamed, &graph), "A renamed node is a subgraph");
}

#[test]
fn subgraph_self_loops() {
    let graph = indexed_from_text(SELF_LOOPS);
    // The same nodes with no edges at all.
    let no_loops = indexed_from_text("S\t1\tGAT\nS\t2\tTA\nS\t3\tCAG\n");
    assert!(is_subgraph(&no_loops, &graph), "A graph without the self-loops is not a subgraph");
    assert!(!is_subgraph(&graph, &no_loops), "The self-loops are a subgraph of a graph without them");

    // Each kind of self-loop on its own. A side self-loop is listed once, on one side only, so it
    // is the case most likely to go wrong.
    for line in ["L\t1\t+\t1\t+\n", "L\t2\t+\t2\t-\n", "L\t3\t-\t3\t+\n"] {
        let text = format!("S\t1\tGAT\nS\t2\tTA\nS\t3\tCAG\n{}", line);
        let single = indexed_from_text(&text);
        assert!(is_subgraph(&single, &graph), "A graph with only {} is not a subgraph", line.trim());
        assert!(!is_subgraph(&graph, &single), "All self-loops are a subgraph of {}", line.trim());
    }
}

#[test]
fn subgraph_duplicate_names() {
    // `IndexedGraph` does not check the names, so a duplicate must not pass as two distinct nodes.
    let mut duplicates = IndexedGraph::new();
    duplicates.add_node(b"1", b"GAT");
    duplicates.add_node(b"1", b"GAT");
    duplicates.finalize();

    let graph = indexed_from_text("S\t1\tGAT\nS\t2\tTA\n");
    assert!(!is_subgraph(&duplicates, &graph), "Duplicate node names pass as distinct nodes");
}

#[test]
fn subgraph_empty() {
    let empty = IndexedGraph::new();
    let graph = indexed_from_text(PATH);
    assert!(is_subgraph(&empty, &graph), "An empty graph is not a subgraph");
    assert!(is_subgraph(&empty, &empty), "An empty graph is not a subgraph of itself");
    assert!(!is_subgraph(&graph, &empty), "A graph is a subgraph of an empty graph");
}

#[test]
fn subgraph_hub() {
    // A high-degree node, where a linear scan of the neighbor list would be quadratic.
    let nodes = 200;
    let mut graph = IndexedGraph::new();
    let hub = graph.add_node(b"hub", b"GATTACA");
    for node in 0..nodes {
        let leaf = graph.add_node(format!("{}", node).as_bytes(), b"A");
        graph.add_edge(hub, Orientation::Forward, leaf, Orientation::Forward);
    }
    graph.finalize();

    let mut without_last = IndexedGraph::new();
    let hub = without_last.add_node(b"hub", b"GATTACA");
    for node in 0..nodes {
        let leaf = without_last.add_node(format!("{}", node).as_bytes(), b"A");
        if node + 1 < nodes {
            without_last.add_edge(hub, Orientation::Forward, leaf, Orientation::Forward);
        }
    }
    without_last.finalize();

    assert_eq!(graph.degree(0, NodeSide::Right), nodes, "Wrong degree for the hub");
    assert!(is_subgraph(&without_last, &graph), "The hub with one edge less is not a subgraph");
    assert!(!is_subgraph(&graph, &without_last), "The hub is a subgraph of itself with one edge less");
}

#[test]
fn subgraph_backends_agree() {
    // The two implementations must format the node names the same way.
    let from_gfa = IndexedGraph::from(&gfa_graph::<GraphInt>("example.gfa"));
    let gbz = gbz_graph("example.gbz");
    let from_gbz = GbzTopology::new(&gbz).unwrap();
    assert!(is_subgraph(&from_gfa, &from_gbz), "The GFA graph is not a subgraph of the GBZ graph");
    assert!(is_subgraph(&from_gbz, &from_gfa), "The GBZ graph is not a subgraph of the GFA graph");
}

#[test]
fn subgraph_random() {
    for &seed in SEEDS {
        let mut rng = StdRng::seed_from_u64(seed);
        let graph = random_graph(20, 30, &mut rng);
        let subset = random_subgraph(&graph, 0.7, 0.8, &mut rng);
        assert!(is_subgraph(&subset, &graph), "Not a subgraph with seed {}", seed);
        if subset.nodes() < graph.nodes() || subset.edges() < graph.edges() {
            assert!(!is_subgraph(&graph, &subset), "A proper subgraph is a supergraph with seed {}", seed);
        }
    }
}

#[test]
fn subgraph_permuted() {
    // Permuting renames the nodes, so the result is isomorphic but not a subgraph.
    for &seed in SEEDS {
        let mut rng = StdRng::seed_from_u64(seed);
        let graph = random_graph(10, 15, &mut rng);
        let permutation = random_permutation(graph.nodes(), &mut rng);
        let permuted = permute(&graph, &permutation, &no_flips(graph.nodes()));
        assert!(!is_subgraph(&permuted, &graph), "A permuted graph is a subgraph with seed {}", seed);
        assert!(!is_subgraph(&graph, &permuted), "A graph is a subgraph of a permutation with seed {}", seed);
    }
}

//-----------------------------------------------------------------------------

fn stable_name_of<G: Graph>(graph: &G) -> String {
    crate::stable_name(graph)
}

//-----------------------------------------------------------------------------
