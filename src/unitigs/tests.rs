use super::*;

use crate::algorithms;
use crate::graph::GraphStr;
use crate::isomorphism::{self, Isomorphism, Options};
use crate::test_utils::*;

use rand::SeedableRng;
use rand::rngs::StdRng;

use std::collections::BTreeSet;
use std::io::BufReader;

//-----------------------------------------------------------------------------

fn gfa_text(text: &str) -> IndexedGraph {
    let graph: GraphStr = algorithms::parse_gfa(BufReader::new(text.as_bytes())).unwrap();
    IndexedGraph::from(&graph)
}

// Checks that the pieces tile the sequence of each unitig, and that every node is in exactly one
// piece.
fn check_pieces<T: Topology>(source: &T, unitigs: &Unitigs, context: &str) {
    let mut seen = vec![false; source.nodes()];
    let mut total = 0;

    for unitig in 0..unitigs.len() {
        let sequence = unitigs.graph().sequence(unitig).to_vec();
        let mut expected: Vec<u8> = Vec::new();
        for piece in unitigs.pieces(unitig) {
            assert_eq!(piece.start, expected.len(), "Pieces do not tile unitig {} in {}", unitig, context);
            assert_eq!(
                piece.len(), source.sequence_len(piece.node),
                "Wrong length for a piece of unitig {} in {}", unitig, context
            );
            assert!(!seen[piece.node], "Node {} is in two pieces in {}", piece.node, context);
            seen[piece.node] = true;
            total += 1;

            assert_eq!(
                unitigs.unitig_of(piece.node), unitig,
                "Wrong unitig for node {} in {}", piece.node, context
            );
            assert_eq!(
                unitigs.piece_of(piece.node), piece,
                "Wrong piece for node {} in {}", piece.node, context
            );

            let node_sequence = source.sequence(piece.node).to_vec();
            match piece.orientation {
                Orientation::Forward => expected.extend_from_slice(&node_sequence),
                Orientation::Reverse => expected.extend(
                    isomorphism::hashing::reverse_complement(&node_sequence)
                ),
            }
        }
        assert_eq!(expected, sequence, "Wrong sequence for unitig {} in {}", unitig, context);
    }

    assert_eq!(total, source.nodes(), "The pieces do not cover every node in {}", context);
}

// Returns the sorted sequences of the unitigs.
fn unitig_sequences(unitigs: &Unitigs) -> Vec<Vec<u8>> {
    let mut result: Vec<Vec<u8>> = (0..unitigs.len())
        .map(|unitig| unitigs.graph().sequence(unitig).to_vec())
        .collect();
    result.sort();
    result
}

//-----------------------------------------------------------------------------

#[test]
fn a_path_collapses() {
    let graph = gfa_text("S\t1\tGAT\nS\t2\tTA\nS\t3\tCA\nL\t1\t+\t2\t+\nL\t2\t+\t3\t+\n");
    let unitigs = Unitigs::new(&graph).unwrap();
    assert_eq!(unitigs.len(), 1, "A path should collapse into one unitig");
    assert_eq!(unitigs.graph().sequence(0).as_ref(), b"GATTACA", "Wrong unitig sequence");
    assert_eq!(unitigs.graph().edges(), 0, "A path should have no edges after collapsing");
    check_pieces(&graph, &unitigs, "a path");
}

#[test]
fn branches_are_preserved() {
    // A bubble: one unitig per branch, plus the two flanks.
    let graph = gfa_text(
        "S\t1\tGAT\nS\t2\tA\nS\t3\tC\nS\t4\tTACA\n\
         L\t1\t+\t2\t+\nL\t1\t+\t3\t+\nL\t2\t+\t4\t+\nL\t3\t+\t4\t+\n"
    );
    let unitigs = Unitigs::new(&graph).unwrap();
    assert_eq!(unitigs.len(), 4, "A bubble should stay a bubble");
    assert_eq!(unitigs.graph().edges(), 4, "Wrong number of edges after collapsing a bubble");
    check_pieces(&graph, &unitigs, "a bubble");
}

#[test]
fn reverse_traversals_are_handled() {
    // The path enters node 2 from its right side, so the unitig reads it in reverse.
    let graph = gfa_text("S\t1\tGAT\nS\t2\tTAA\nS\t3\tCA\nL\t1\t+\t2\t-\nL\t2\t-\t3\t+\n");
    let unitigs = Unitigs::new(&graph).unwrap();
    assert_eq!(unitigs.len(), 1, "The path should collapse into one unitig");
    check_pieces(&graph, &unitigs, "a path with a reverse traversal");

    // `GAT` + revcomp(`TAA`) + `CA` = `GAT` + `TTA` + `CA`.
    let sequence = unitigs.graph().sequence(0).to_vec();
    let expected = b"GATTTACA".to_vec();
    let canonical = isomorphism::hashing::reverse_complement(&expected);
    assert!(
        sequence == expected || sequence == canonical,
        "Wrong unitig sequence: {}", String::from_utf8_lossy(&sequence)
    );
}

#[test]
fn direction_is_canonical() {
    // The same path written from either end must give the same unitig sequence.
    let forward = gfa_text("S\t1\tGAT\nS\t2\tTA\nS\t3\tCA\nL\t1\t+\t2\t+\nL\t2\t+\t3\t+\n");
    let backward = gfa_text("S\t1\tTG\nS\t2\tTA\nS\t3\tATC\nL\t1\t+\t2\t+\nL\t2\t+\t3\t+\n");
    let first = Unitigs::new(&forward).unwrap();
    let second = Unitigs::new(&backward).unwrap();
    // `TGTAATC` is the reverse complement of `GATTACA`.
    assert_eq!(
        unitig_sequences(&first), unitig_sequences(&second),
        "The two directions gave different canonical sequences"
    );
}

#[test]
fn self_loops_are_preserved() {
    let graph = gfa_text(
        "S\t1\tGAT\nS\t2\tTAC\nS\t3\tCAG\nL\t1\t+\t1\t+\nL\t2\t+\t2\t-\nL\t3\t-\t3\t+\n"
    );
    let unitigs = Unitigs::new(&graph).unwrap();
    assert_eq!(unitigs.len(), 3, "Self-loops should not be collapsed");
    assert_eq!(unitigs.graph().edges(), 3, "Wrong number of self-loops after collapsing");
    check_pieces(&graph, &unitigs, "self-loops");
}

#[test]
fn isolated_nodes_become_unitigs() {
    let graph = gfa_text("S\t1\tGATTACA\nS\t2\tCCC\n");
    let unitigs = Unitigs::new(&graph).unwrap();
    assert_eq!(unitigs.len(), 2, "Isolated nodes should become unitigs");
    assert_eq!(unitigs.graph().edges(), 0, "Isolated nodes should have no edges");
    check_pieces(&graph, &unitigs, "isolated nodes");
}

#[test]
fn branch_free_cycles_are_rejected() {
    // Two nodes joined at both ends, with no branching anywhere.
    let graph = gfa_text("S\t1\tGAT\nS\t2\tTACA\nL\t1\t+\t2\t+\nL\t2\t+\t1\t+\n");
    let result = Unitigs::new(&graph);
    assert!(result.is_err(), "A branch-free cycle should be rejected");

    // The same component next to a normal one: the error must still be reported.
    let graph = gfa_text(
        "S\t1\tGAT\nS\t2\tTACA\nS\t3\tCCC\nL\t1\t+\t2\t+\nL\t2\t+\t1\t+\n"
    );
    assert!(Unitigs::new(&graph).is_err(), "A branch-free cycle should be rejected");
}

#[test]
fn empty_graph() {
    let graph = IndexedGraph::new();
    let unitigs = Unitigs::new(&graph).unwrap();
    assert!(unitigs.is_empty(), "An empty graph should have no unitigs");
}

//-----------------------------------------------------------------------------

#[test]
fn chopping_does_not_change_the_unitigs() {
    for &seed in SEEDS.iter() {
        let mut rng = StdRng::seed_from_u64(seed);
        let graph = random_graph(30, 40, &mut rng);
        let Ok(unitigs) = Unitigs::new(&graph) else { continue };
        check_pieces(&graph, &unitigs, &format!("seed {}", seed));

        for max_len in [1, 2, 3] {
            let chopped = chop(&graph, max_len);
            let other = Unitigs::new(&chopped).unwrap();
            check_pieces(&chopped, &other, &format!("chopped at {}, seed {}", max_len, seed));

            assert_eq!(
                unitig_sequences(&unitigs), unitig_sequences(&other),
                "Chopping at {} changed the unitig sequences (seed {})", max_len, seed
            );
            let result = isomorphism::are_isomorphic(
                unitigs.graph(), other.graph(), &Options::default()
            );
            assert!(
                matches!(result, Isomorphism::Isomorphic(_)),
                "Chopping at {} changed the unitig graph (seed {}): {}", max_len, seed, result
            );
        }
    }
}

#[test]
fn chopping_a_real_graph() {
    let path = support::get_test_data("example.gfa");
    let file = std::fs::OpenOptions::new().read(true).open(&path).unwrap();
    let graph: GraphStr = algorithms::parse_gfa(BufReader::new(file)).unwrap();
    let graph = IndexedGraph::from(&graph);

    let unitigs = Unitigs::new(&graph).unwrap();
    check_pieces(&graph, &unitigs, "example.gfa");

    // The node sequences are 1 bp, so the unitigs are already maximal.
    let names: BTreeSet<Vec<u8>> = (0..unitigs.len())
        .map(|unitig| unitigs.graph().node_name(unitig))
        .collect();
    assert_eq!(names.len(), unitigs.len(), "The unitig names are not distinct");
}

//-----------------------------------------------------------------------------
