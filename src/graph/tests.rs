use super::*;

use gbwt::support;
use rand::Rng;

use std::collections::BTreeSet;

//-----------------------------------------------------------------------------

// Returns the orientation as `+` or `-`.
fn as_char(o: Orientation) -> char {
    match o {
        Orientation::Forward => '+',
        Orientation::Reverse => '-',
    }
}

fn create_gfa_int(id: usize, sequence: &str) -> String {
    format!("S\t{}\t{}\n", id, sequence)
}

fn random_orientation(rng: &mut impl Rng) -> Orientation {
    if rng.random_bool(0.5) {
        Orientation::Forward
    } else {
        Orientation::Reverse
    }
}

// Returns a random set of edges to the given nodes.
// The edges may be in either orientation, and there may be duplicates.
fn random_edges_int(nodes: &[usize], n: usize) -> Vec<(Orientation, usize, Orientation)> {
    let mut rng = rand::rng();
    let mut edges = Vec::new();

    for _ in 0..n {
        let to_id = nodes[rng.random_range(0..nodes.len())];
        let from_o = random_orientation(&mut rng);
        let to_o = random_orientation(&mut rng);
        edges.push((from_o, to_id, to_o));
    }

    edges
}

// Returns the canonical GFA L-line for an edge.
fn gfa_edge_int(from_id: usize, from_o: Orientation, to_id: usize, to_o: Orientation) -> String {
    format!("L\t{}\t{}\t{}\t{}\n", from_id, as_char(from_o), to_id, as_char(to_o))
}

fn create_gfa_str(id: &str, sequence: &str) -> String {
    format!("S\t{}\t{}\n", id, sequence)
}

// Returns a random set of edges to the given nodes.
// The edges may be in either orientation, and there may be duplicates.
fn random_edges_str(nodes: &[&str], n: usize) -> Vec<(Orientation, String, Orientation)> {
    let mut rng = rand::rng();
    let mut edges = Vec::new();

    for _ in 0..n {
        let to_id = nodes[rng.random_range(0..nodes.len())].to_string();
        let from_o = random_orientation(&mut rng);
        let to_o = random_orientation(&mut rng);
        edges.push((from_o, to_id, to_o));
    }

    edges
}

fn edge_is_canonical_str(from_id: &str, from_o: Orientation, to_id: &str, to_o: Orientation) -> bool {
    if from_id != to_id {
        return from_id < to_id;
    }
    return from_o == Orientation::Forward || to_o == Orientation::Forward;
}

// Returns the canonical GFA L-line for an edge.
fn gfa_edge_str(from_id: &str, from_o: Orientation, to_id: &str, to_o: Orientation) -> String {
    format!("L\t{}\t{}\t{}\t{}\n", from_id, as_char(from_o), to_id, as_char(to_o))
}

const NODE_ROUNDS: usize = 10;
const EDGES_PER_NODE: usize = 6;

//-----------------------------------------------------------------------------

#[test]
fn nodes_seen() {
    let unseen = NodeInt::new(None);
    assert!(!unseen.seen, "NodeInt without sequence should be unseen");
    let seen = NodeInt::new(Some(b"ACGT".to_vec()));
    assert!(seen.seen, "NodeInt with sequence should be seen");

    let unseen = NodeStr::new(None);
    assert!(!unseen.seen, "NodeStr without sequence should be unseen");
    let seen = NodeStr::new(Some(b"ACGT".to_vec()));
    assert!(seen.seen, "NodeStr with sequence should be seen");
}

#[test]
fn node_int() {
    let nodes = vec![1, 2, 3, 4, 5];
    for round in 0..NODE_ROUNDS {
        let from_id = round + 1;
        let mut node = NodeInt::new(Some(b"GATTACA".to_vec()));
        let mut gfa = create_gfa_int(from_id, &"GATTACA");
        let mut canonical_edges: BTreeSet<(Orientation, usize, Orientation)> = BTreeSet::new();

        // Create edges.
        let edges = random_edges_int(&nodes, EDGES_PER_NODE);
        for (from_o, to_id, to_o) in edges {
            if !support::edge_is_canonical((from_id, from_o), (to_id, to_o)) {
                continue;
            }
            node.edges.push((from_o, to_id, to_o));
            canonical_edges.insert((from_o, to_id, to_o));
        }
        node.finalize();

        // Construct the canonical GFA representation and compare it to the serialized node.
        for (from_o, to_id, to_o) in &canonical_edges {
            let edge_gfa = gfa_edge_int(from_id, *from_o, *to_id, *to_o);
            gfa.push_str(&edge_gfa);
        }
        let serialized = node.serialize(from_id);
        let serialized = String::from_utf8_lossy(&serialized);
        assert_eq!(serialized, gfa, "Wrong serialization in round {}", round);
    }
}

#[test]
fn node_str() {
    let nodes = vec!["A", "B", "C", "D", "E"];
    for round in 0..NODE_ROUNDS {
        let from_id = format!("N{}", round + 1);
        let mut node = NodeStr::new(Some(b"GATTACA".to_vec()));
        let mut gfa = create_gfa_str(&from_id, &"GATTACA");
        let mut canonical_edges: BTreeSet<(Orientation, String, Orientation)> = BTreeSet::new();

        // Create edges.
        let edges = random_edges_str(&nodes, EDGES_PER_NODE);
        for (from_o, to_id, to_o) in edges {
            if !edge_is_canonical_str(&from_id, from_o, &to_id, to_o) {
                continue;
            }
            node.edges.push((from_o, to_id.as_bytes().to_vec(), to_o));
            canonical_edges.insert((from_o, to_id, to_o));
        }
        node.finalize();

        // Construct the canonical GFA representation and compare it to the serialized node.
        for (from_o, to_id, to_o) in &canonical_edges {
            let edge_gfa = gfa_edge_str(&from_id, *from_o, to_id, *to_o);
            gfa.push_str(&edge_gfa);
        }
        let serialized = node.serialize(from_id.as_bytes());
        let serialized = String::from_utf8_lossy(&serialized);
        assert_eq!(serialized, gfa, "Wrong serialization in round {}", round);
    }
}

//-----------------------------------------------------------------------------

const GRAPH_ROUNDS: usize = 5;
const NODE_COUNT: usize = 5;

fn nodes_and_sequences(int_ids: bool) -> (Vec<String>, Vec<String>) {
    let nodes = if int_ids {
        vec![
            String::from("1"),
            String::from("2"),
            String::from("3"),
            String::from("4"),
            String::from("5"),
        ]
    } else {
        vec![
            String::from("N1"),
            String::from("N2"),
            String::from("N3"),
            String::from("N4"),
            String::from("N5"),
        ]
    };
    let sequences = vec![
        String::from("GATTACA"),
        String::from("CTAGGTA"),
        String::from("TTCAGG"),
        String::from("GGATC"),
        String::from("ACCTGA"),
    ];
    (nodes, sequences)
}

fn parse_node_ids(nodes: &[String]) -> Vec<usize> {
    nodes.iter()
        .map(|s| s.parse::<usize>().unwrap())
        .collect()
}

//-----------------------------------------------------------------------------

fn add_nodes<G: Graph>(graph: &mut G, nodes: &[String], sequences: &[String]) {
    for i in 0..NODE_COUNT {
        let res = graph.add_node(nodes[i].as_bytes(), sequences[i].as_bytes());
        assert!(res.is_ok(), "Error adding node: {}", res.unwrap_err());
    }
}

// Returns the canonical edge sets for each node.
fn add_edges_int(graph: &mut GraphInt, nodes: &[String], node_ids: &[usize]) -> Vec<BTreeSet<(Orientation, usize, Orientation)>> {
    let mut canonical_edges: Vec<BTreeSet<(Orientation, usize, Orientation)>> = vec![BTreeSet::new(); NODE_COUNT];
    let mut rng = rand::rng();

    for _ in 0..(NODE_COUNT * EDGES_PER_NODE) {
        let from = rng.random_range(0..NODE_COUNT);
        let to = rng.random_range(0..NODE_COUNT);
        let from_id = node_ids[from];
        let to_id = node_ids[to];
        let from_o = random_orientation(&mut rng);
        let to_o = random_orientation(&mut rng);
        if !support::edge_is_canonical((from_id, from_o), (to_id, to_o)) {
            continue;
        }
        let res = graph.add_edge(nodes[from].as_bytes(), from_o, nodes[to].as_bytes(), to_o);
        assert!(res.is_ok(), "Error adding edge: {}", res.unwrap_err());
        canonical_edges[from].insert((from_o, to_id, to_o));
    }

    canonical_edges
}

// Returns the canonical edge sets for each node.
fn add_edges_str(graph: &mut GraphStr, nodes: &[String]) -> Vec<BTreeSet<(Orientation, String, Orientation)>> {
    let mut canonical_edges: Vec<BTreeSet<(Orientation, String, Orientation)>> = vec![BTreeSet::new(); NODE_COUNT];
    let mut rng = rand::rng();

    for _ in 0..(NODE_COUNT * EDGES_PER_NODE) {
        let from = rng.random_range(0..NODE_COUNT);
        let to = rng.random_range(0..NODE_COUNT);
        let from_id = &nodes[from];
        let to_id = &nodes[to];
        let from_o = random_orientation(&mut rng);
        let to_o = random_orientation(&mut rng);
        if !edge_is_canonical_str(from_id, from_o, to_id, to_o) {
            continue;
        }
        let res = graph.add_edge(from_id.as_bytes(), from_o, to_id.as_bytes(), to_o);
        assert!(res.is_ok(), "Error adding edge: {}", res.unwrap_err());
        canonical_edges[from].insert((from_o, to_id.clone(), to_o));
    }

    canonical_edges
}

fn check_statistics<G: Graph, E: Sized>(graph: &G, canonical_edges: &[BTreeSet<E>], sequences: &[String], round: usize) {
    let true_node_count = NODE_COUNT;
    let true_edge_count: usize = canonical_edges.iter().map(|edges| edges.len()).sum();
    let true_seq_len: usize = sequences.iter().map(|s| s.len()).sum();
    let (node_count, edge_count, seq_len) = graph.statistics();
    assert_eq!(node_count, true_node_count, "Wrong node count in round {}", round);
    assert_eq!(edge_count, true_edge_count, "Wrong edge count in round {}", round);
    assert_eq!(seq_len, true_seq_len, "Wrong sequence length in round {}", round);
}

fn check_gfa_int(
    graph: &GraphInt,
    node_ids: &[usize], sequences: &[String],
    canonical_edges: &[BTreeSet<(Orientation, usize, Orientation)>],
    round: usize
) {
    let serialized: Vec<Vec<u8>> = graph.node_iter().collect();
    assert_eq!(serialized.len(), NODE_COUNT, "Wrong number of serialized nodes in round {}", round);
    for i in 0..NODE_COUNT {
        let mut gfa = create_gfa_int(node_ids[i], &sequences[i]);
        for (from_o, to_id, to_o) in &canonical_edges[i] {
            let edge_gfa = gfa_edge_int(node_ids[i], *from_o, *to_id, *to_o);
            gfa.push_str(&edge_gfa);
        }
        let serialized_gfa = String::from_utf8_lossy(&serialized[i]);
        assert_eq!(serialized_gfa, gfa, "Wrong serialization of node {} in round {}", node_ids[i], round);
    }
}

fn check_gfa_str(
    graph: &GraphStr,
    nodes: &[String], sequences: &[String],
    canonical_edges: &[BTreeSet<(Orientation, String, Orientation)>],
    round: usize
) {
    let serialized: Vec<Vec<u8>> = graph.node_iter().collect();
    assert_eq!(serialized.len(), NODE_COUNT, "Wrong number of serialized nodes in round {}", round);
    for i in 0..NODE_COUNT {
        let mut gfa = create_gfa_str(&nodes[i], &sequences[i]);
        for (from_o, to_id, to_o) in &canonical_edges[i] {
            let edge_gfa = gfa_edge_str(&nodes[i], *from_o, to_id, *to_o);
            gfa.push_str(&edge_gfa);
        }
        let serialized_gfa = String::from_utf8_lossy(&serialized[i]);
        assert_eq!(serialized_gfa, gfa, "Wrong serialization of node {} in round {}", nodes[i], round);
    }
}

#[test]
fn graph_int_nodes_first() {
    let (nodes, sequences) = nodes_and_sequences(true);
    let node_ids = parse_node_ids(&nodes);
    for round in 0..GRAPH_ROUNDS {
        let mut graph = GraphInt::new();
        add_nodes(&mut graph, &nodes, &sequences);
        let canonical_edges = add_edges_int(&mut graph, &nodes, &node_ids);
        let res = graph.finalize();
        assert!(res.is_ok(), "Error finalizing graph in round {}: {}", round, res.unwrap_err());
        check_statistics(&graph, &canonical_edges, &sequences, round);
        check_gfa_int(&graph, &node_ids, &sequences, &canonical_edges, round);
    }
}

#[test]
fn graph_int_edges_first() {
    let (nodes, sequences) = nodes_and_sequences(true);
    let node_ids = parse_node_ids(&nodes);
    for round in 0..GRAPH_ROUNDS {
        let mut graph = GraphInt::new();
        let canonical_edges = add_edges_int(&mut graph, &nodes, &node_ids);
        add_nodes(&mut graph, &nodes, &sequences);
        let res = graph.finalize();
        assert!(res.is_ok(), "Error finalizing graph in round {}: {}", round, res.unwrap_err());
        check_statistics(&graph, &canonical_edges, &sequences, round);
        check_gfa_int(&graph, &node_ids, &sequences, &canonical_edges, round);
    }
}

#[test]
fn graph_str_nodes_first() {
    let (nodes, sequences) = nodes_and_sequences(false);
    for round in 0..GRAPH_ROUNDS {
        let mut graph = GraphStr::new();
        add_nodes(&mut graph, &nodes, &sequences);
        let canonical_edges = add_edges_str(&mut graph, &nodes);
        let res = graph.finalize();
        assert!(res.is_ok(), "Error finalizing graph in round {}: {}", round, res.unwrap_err());
        check_statistics(&graph, &canonical_edges, &sequences, round);
        check_gfa_str(&graph, &nodes, &sequences, &canonical_edges, round);
    }
}

#[test]
fn graph_str_edges_first() {
    let (nodes, sequences) = nodes_and_sequences(false);
    for round in 0..GRAPH_ROUNDS {
        let mut graph = GraphStr::new();
        let canonical_edges = add_edges_str(&mut graph, &nodes);
        add_nodes(&mut graph, &nodes, &sequences);
        let res = graph.finalize();
        assert!(res.is_ok(), "Error finalizing graph in round {}: {}", round, res.unwrap_err());
        check_statistics(&graph, &canonical_edges, &sequences, round);
        check_gfa_str(&graph, &nodes, &sequences, &canonical_edges, round);
    }
}

//-----------------------------------------------------------------------------

// FIXME: tests
// existing graphs using GBZInt and GBZStr
// statistics
// iterator vs. alternate canonical GFA implementation

//-----------------------------------------------------------------------------
