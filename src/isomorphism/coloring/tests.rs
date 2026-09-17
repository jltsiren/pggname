use super::*;

use crate::algorithms;
use crate::graph::GraphStr;
use crate::test_utils::*;
use crate::topology::{GbzTopology, IndexedGraph};

use gbz::GBZ;
use rand::SeedableRng;
use rand::rngs::StdRng;
use simple_sds::serialize;

use std::fs::OpenOptions;
use std::io::BufReader;

//-----------------------------------------------------------------------------

fn signature_of<T: Topology>(graph: &T) -> (u64, u64) {
    Coloring::new(graph, &Options::default()).signature()
}

fn gfa_text(text: &str) -> IndexedGraph {
    let graph: GraphStr = algorithms::parse_gfa(BufReader::new(text.as_bytes())).unwrap();
    IndexedGraph::from(&graph)
}

//-----------------------------------------------------------------------------

#[test]
fn permutation_does_not_change_the_signature() {
    for &seed in SEEDS.iter() {
        let mut rng = StdRng::seed_from_u64(seed);
        let graph = random_graph(30, 45, &mut rng);
        let permutation = random_permutation(graph.nodes(), &mut rng);
        let permuted = permute(&graph, &permutation, &no_flips(graph.nodes()));
        assert_eq!(
            signature_of(&graph), signature_of(&permuted),
            "Permuting the nodes changed the signature (seed {})", seed
        );
    }
}

#[test]
fn flips_do_not_change_the_signature() {
    for &seed in SEEDS.iter() {
        let mut rng = StdRng::seed_from_u64(seed);
        let graph = random_graph(30, 45, &mut rng);
        let permutation = random_permutation(graph.nodes(), &mut rng);
        let flips = random_flips(graph.nodes(), &mut rng);
        let permuted = permute(&graph, &permutation, &flips);
        assert_eq!(
            signature_of(&graph), signature_of(&permuted),
            "Flipping the nodes changed the signature (seed {})", seed
        );
    }

    // A path of distinct, non-palindromic sequences, with a single node reverse complemented.
    let graph = rigid_graph(6);
    let mut flips = no_flips(graph.nodes());
    flips[2] = true;
    let flipped = permute(&graph, &(0..graph.nodes()).collect::<Vec<_>>(), &flips);
    assert_eq!(
        signature_of(&graph), signature_of(&flipped),
        "Flipping a single node changed the signature"
    );
}

#[test]
fn signature_detects_changes() {
    let base = "S\t1\tGATT\nS\t2\tACA\nS\t3\tTTG\nS\t4\tCCA\n\
                L\t1\t+\t2\t+\nL\t1\t+\t3\t+\nL\t2\t+\t4\t+\nL\t3\t+\t4\t+\n";
    let graph = gfa_text(base);

    let changed_base = "S\t1\tGATT\nS\t2\tACC\nS\t3\tTTG\nS\t4\tCCA\n\
                        L\t1\t+\t2\t+\nL\t1\t+\t3\t+\nL\t2\t+\t4\t+\nL\t3\t+\t4\t+\n";
    let extra_edge = "S\t1\tGATT\nS\t2\tACA\nS\t3\tTTG\nS\t4\tCCA\n\
                      L\t1\t+\t2\t+\nL\t1\t+\t3\t+\nL\t2\t+\t4\t+\nL\t3\t+\t4\t+\nL\t1\t+\t4\t+\n";
    let moved_edge = "S\t1\tGATT\nS\t2\tACA\nS\t3\tTTG\nS\t4\tCCA\n\
                      L\t1\t+\t2\t+\nL\t1\t+\t3\t+\nL\t2\t+\t4\t+\nL\t3\t+\t4\t-\n";

    for (name, text) in [("a changed base", changed_base), ("an added edge", extra_edge), ("a moved edge", moved_edge)] {
        let other = gfa_text(text);
        assert_ne!(
            signature_of(&graph), signature_of(&other),
            "The signature did not detect {}", name
        );
    }
}

#[test]
fn backends_agree() {
    let path = support::get_test_data("example.gfa");
    let file = OpenOptions::new().read(true).open(&path).unwrap();
    let gfa: GraphStr = algorithms::parse_gfa(BufReader::new(file)).unwrap();
    let from_gfa = IndexedGraph::from(&gfa);

    let path = support::get_test_data("example.gbz");
    let gbz: GBZ = serialize::load_from(&path).unwrap();
    let from_gbz = GbzTopology::new(&gbz).unwrap();

    assert_eq!(
        signature_of(&from_gfa), signature_of(&from_gbz),
        "The GFA and GBZ backends disagree on the signature"
    );
}

//-----------------------------------------------------------------------------

#[test]
fn refinement_separates_nodes() {
    // The two components of `example.gfa` have the same sequences but different structure.
    let path = support::get_test_data("example.gfa");
    let file = OpenOptions::new().read(true).open(&path).unwrap();
    let gfa: GraphStr = algorithms::parse_gfa(BufReader::new(file)).unwrap();
    let graph = IndexedGraph::from(&gfa);

    let coloring = Coloring::new(&graph, &Options::default());
    assert_eq!(
        coloring.classes(), graph.nodes(),
        "Refinement did not give every node of example.gfa a distinct color"
    );
    assert!(coloring.rounds() > 0, "Refinement did not run any rounds");
}

#[test]
fn refinement_stops_when_stable() {
    // A cycle of identical nodes is stable from the start: every node looks the same.
    let text = "S\t1\tAT\nS\t2\tAT\nS\t3\tAT\nL\t1\t+\t2\t+\nL\t2\t+\t3\t+\nL\t3\t+\t1\t+\n";
    let graph = gfa_text(text);
    let coloring = Coloring::new(&graph, &Options::default());
    assert_eq!(coloring.classes(), 1, "A cycle of identical nodes should have one color class");
    assert_eq!(coloring.rounds(), 0, "Refinement should stop immediately when the coloring is stable");
}

//-----------------------------------------------------------------------------

#[test]
fn classes_match_for_isomorphic_graphs() {
    for &seed in SEEDS.iter() {
        let mut rng = StdRng::seed_from_u64(seed);
        let graph = random_graph(30, 45, &mut rng);
        let permutation = random_permutation(graph.nodes(), &mut rng);
        let flips = random_flips(graph.nodes(), &mut rng);
        let permuted = permute(&graph, &permutation, &flips);

        let first = Coloring::new(&graph, &Options::default());
        let second = Coloring::new(&permuted, &Options::default());
        let classes = Classes::new(&first, &second).unwrap_or_else(|e| {
            panic!("Failed to build classes for isomorphic graphs (seed {}): {}", seed, e)
        });

        // Each node must be in the same class as its image.
        for (node, &image) in permutation.iter().enumerate() {
            assert_eq!(
                classes.in_first(node), classes.in_second(image),
                "Node {} and its image are in different classes (seed {})", node, seed
            );
        }
        let total: usize = (0..classes.len()).map(|c| classes.size(c as u32)).sum();
        assert_eq!(total, graph.nodes(), "The classes do not cover every node (seed {})", seed);
    }
}

#[test]
fn classes_detect_mismatches() {
    let graph = gfa_text("S\t1\tGATT\nS\t2\tACA\nL\t1\t+\t2\t+\n");
    let other = gfa_text("S\t1\tGATT\nS\t2\tACC\nL\t1\t+\t2\t+\n");
    let smaller = gfa_text("S\t1\tGATT\n");

    let first = Coloring::new(&graph, &Options::default());
    let second = Coloring::new(&other, &Options::default());
    let third = Coloring::new(&smaller, &Options::default());

    assert_eq!(Classes::new(&first, &second), Err(Mismatch::Colors), "Different sequences were not detected");
    assert_eq!(Classes::new(&first, &third), Err(Mismatch::Colors), "Different node counts were not detected");
    assert!(Classes::new(&first, &first).is_ok(), "A graph does not match itself");
}

//-----------------------------------------------------------------------------
