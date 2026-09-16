use super::*;

use crate::algorithms;
use crate::graph::{GraphInt, GraphStr};
use crate::test_utils::*;
use crate::topology::{GbzTopology, IndexedGraph};

use gbz::{GBZ, Orientation};
use gbz::support;
use rand::SeedableRng;
use rand::rngs::StdRng;
use simple_sds::serialize;

use std::fs::OpenOptions;
use std::io::BufReader;

//-----------------------------------------------------------------------------

fn options(allow_flips: bool) -> Options {
    Options { allow_flips, ..Options::default() }
}

fn gfa_text(text: &str) -> IndexedGraph {
    let graph: GraphStr = algorithms::parse_gfa(BufReader::new(text.as_bytes())).unwrap();
    IndexedGraph::from(&graph)
}

fn gfa_file(filename: &'static str) -> IndexedGraph {
    let path = support::get_test_data(filename);
    let file = OpenOptions::new().read(true).open(&path).unwrap();
    let graph: GraphInt = algorithms::parse_gfa(BufReader::new(file)).unwrap();
    IndexedGraph::from(&graph)
}

fn gbz_file(filename: &'static str) -> GBZ {
    let path = support::get_test_data(filename);
    serialize::load_from(&path).unwrap()
}

// Checks that the result is an isomorphism, using the independent checker.
fn check_isomorphic<A: Topology, B: Topology>(
    first: &A, second: &B, allow_flips: bool, context: &str
) -> NodeMapping {
    let result = are_isomorphic(first, second, &options(allow_flips));
    let mapping = match result {
        Isomorphism::Isomorphic(mapping) => mapping,
        other => panic!("Expected an isomorphism for {}, got {}", context, other),
    };
    if let Err(error) = is_isomorphism(first, second, &mapping, allow_flips) {
        panic!("The mapping for {} is not an isomorphism: {}", context, error);
    }
    mapping
}

//-----------------------------------------------------------------------------

#[test]
fn permuted_graphs_are_isomorphic() {
    for &seed in SEEDS.iter() {
        let mut rng = StdRng::seed_from_u64(seed);
        let graph = random_graph(30, 45, &mut rng);
        let permutation = random_permutation(graph.nodes(), &mut rng);
        let flips = no_flips(graph.nodes());
        let permuted = permute(&graph, &permutation, &flips);

        // If the fixture itself is wrong, the test would be vacuous.
        let injected = mapping_of(&permutation, &flips);
        assert!(
            is_isomorphism(&graph, &permuted, &injected, false).is_ok(),
            "The injected permutation is not an isomorphism (seed {})", seed
        );

        check_isomorphic(&graph, &permuted, false, &format!("seed {}", seed));
    }
}

#[test]
fn flipped_graphs_are_isomorphic() {
    for &seed in SEEDS.iter() {
        let mut rng = StdRng::seed_from_u64(seed);
        let graph = random_graph(30, 45, &mut rng);
        let permutation = random_permutation(graph.nodes(), &mut rng);
        let flips = random_flips(graph.nodes(), &mut rng);
        let permuted = permute(&graph, &permutation, &flips);

        let injected = mapping_of(&permutation, &flips);
        assert!(
            is_isomorphism(&graph, &permuted, &injected, true).is_ok(),
            "The injected permutation is not an isomorphism (seed {})", seed
        );

        check_isomorphic(&graph, &permuted, true, &format!("seed {}", seed));
    }
}

#[test]
fn rigid_graphs_have_a_unique_isomorphism() {
    for &seed in SEEDS.iter() {
        let mut rng = StdRng::seed_from_u64(seed);
        let graph = rigid_graph(8);
        let permutation = random_permutation(graph.nodes(), &mut rng);
        let flips = no_flips(graph.nodes());
        let permuted = permute(&graph, &permutation, &flips);

        let mapping = check_isomorphic(&graph, &permuted, false, &format!("seed {}", seed));
        // The graph has no automorphisms, so the mapping must be the injected permutation.
        assert_eq!(
            mapping, mapping_of(&permutation, &flips),
            "Wrong mapping for a rigid graph (seed {})", seed
        );
    }
}

#[test]
fn a_graph_is_isomorphic_to_itself() {
    for &seed in SEEDS.iter() {
        let mut rng = StdRng::seed_from_u64(seed);
        let graph = random_graph(30, 45, &mut rng);
        for allow_flips in [false, true] {
            check_isomorphic(&graph, &graph, allow_flips, &format!("seed {}, flips {}", seed, allow_flips));
        }
    }
}

#[test]
fn backends_are_interchangeable() {
    let from_gfa = gfa_file("example.gfa");
    let gbz = gbz_file("example.gbz");
    let from_gbz = GbzTopology::new(&gbz).unwrap();

    for allow_flips in [false, true] {
        let mapping = check_isomorphic(&from_gfa, &from_gbz, allow_flips, "example.gfa vs example.gbz");
        // Both views list the nodes in the same order, and the graph is rigid.
        assert!(
            mapping.iter().all(|(source, destination, _)| source == destination),
            "Wrong mapping between the GFA and GBZ views (flips {})", allow_flips
        );
        assert!(mapping.is_forward(), "The mapping should not flip any node (flips {})", allow_flips);
    }
}

#[test]
fn mapping_inverts() {
    for &seed in SEEDS.iter() {
        let mut rng = StdRng::seed_from_u64(seed);
        let graph = random_graph(20, 30, &mut rng);
        let permutation = random_permutation(graph.nodes(), &mut rng);
        let flips = random_flips(graph.nodes(), &mut rng);
        let permuted = permute(&graph, &permutation, &flips);

        let mapping = check_isomorphic(&graph, &permuted, true, &format!("seed {}", seed));
        let inverse = mapping.invert();
        assert!(
            is_isomorphism(&permuted, &graph, &inverse, true).is_ok(),
            "The inverse mapping is not an isomorphism (seed {})", seed
        );
        assert_eq!(inverse.invert(), mapping, "Inverting twice changed the mapping (seed {})", seed);
    }
}

//-----------------------------------------------------------------------------

const BASE: &str = "S\t1\tGATT\nS\t2\tACA\nS\t3\tTTG\nS\t4\tCCA\n\
                    L\t1\t+\t2\t+\nL\t1\t+\t3\t+\nL\t2\t+\t4\t+\nL\t3\t+\t4\t+\n";

#[test]
fn single_changes_are_detected() {
    let graph = gfa_text(BASE);

    // (description, GFA, expected mismatch if it is forced by an exact invariant)
    let cases: Vec<(&str, &str, Option<Mismatch>)> = vec![
        ("a removed edge",
         "S\t1\tGATT\nS\t2\tACA\nS\t3\tTTG\nS\t4\tCCA\n\
          L\t1\t+\t2\t+\nL\t1\t+\t3\t+\nL\t2\t+\t4\t+\n", Some(Mismatch::EdgeCount)),
        ("an added edge",
         "S\t1\tGATT\nS\t2\tACA\nS\t3\tTTG\nS\t4\tCCA\n\
          L\t1\t+\t2\t+\nL\t1\t+\t3\t+\nL\t2\t+\t4\t+\nL\t3\t+\t4\t+\nL\t1\t+\t4\t+\n",
         Some(Mismatch::EdgeCount)),
        ("a changed base",
         "S\t1\tGATT\nS\t2\tACC\nS\t3\tTTG\nS\t4\tCCA\n\
          L\t1\t+\t2\t+\nL\t1\t+\t3\t+\nL\t2\t+\t4\t+\nL\t3\t+\t4\t+\n", None),
        ("a longer sequence",
         "S\t1\tGATTA\nS\t2\tACA\nS\t3\tTTG\nS\t4\tCCA\n\
          L\t1\t+\t2\t+\nL\t1\t+\t3\t+\nL\t2\t+\t4\t+\nL\t3\t+\t4\t+\n",
         Some(Mismatch::SequenceLength)),
        ("an extra node",
         "S\t1\tGATT\nS\t2\tACA\nS\t3\tTTG\nS\t4\tCCA\nS\t5\tG\n\
          L\t1\t+\t2\t+\nL\t1\t+\t3\t+\nL\t2\t+\t4\t+\nL\t3\t+\t4\t+\n", Some(Mismatch::NodeCount)),
        ("a rewired edge",
         "S\t1\tGATT\nS\t2\tACA\nS\t3\tTTG\nS\t4\tCCA\n\
          L\t1\t+\t2\t+\nL\t1\t+\t3\t+\nL\t2\t+\t4\t+\nL\t3\t+\t4\t-\n", None),
    ];

    for (description, text, expected) in cases {
        let other = gfa_text(text);
        for allow_flips in [false, true] {
            let result = are_isomorphic(&graph, &other, &options(allow_flips));
            match (&result, expected) {
                (Isomorphism::NotIsomorphic(found), Some(expected)) => assert_eq!(
                    *found, expected,
                    "Wrong reason for {} (flips {})", description, allow_flips
                ),
                (Isomorphism::NotIsomorphic(_), None) => {},
                _ => panic!("Expected a mismatch for {} (flips {}), got {}", description, allow_flips, result),
            }
        }
    }
}

#[test]
fn reverse_complementing_one_node_is_detected() {
    // The sequence is reverse complemented, but the edges are not adjusted. The graph changes even
    // when flips are allowed.
    let graph = gfa_text(BASE);
    let flipped = gfa_text(
        "S\t1\tGATT\nS\t2\tTGT\nS\t3\tTTG\nS\t4\tCCA\n\
         L\t1\t+\t2\t+\nL\t1\t+\t3\t+\nL\t2\t+\t4\t+\nL\t3\t+\t4\t+\n"
    );
    for allow_flips in [false, true] {
        let result = are_isomorphic(&graph, &flipped, &options(allow_flips));
        assert!(
            matches!(result, Isomorphism::NotIsomorphic(_)),
            "A reverse complemented sequence was not detected (flips {}), got {}", allow_flips, result
        );
    }
}

#[test]
fn different_sequence_lengths_with_equal_totals() {
    // Total sequence lengths agree, so the mismatch must come from the colors.
    let first = gfa_text("S\t1\tGGG\nS\t2\tCCCCC\nL\t1\t+\t2\t+\n");
    let second = gfa_text("S\t1\tGGGG\nS\t2\tCCCC\nL\t1\t+\t2\t+\n");
    let result = are_isomorphic(&first, &second, &options(false));
    assert_eq!(
        result, Isomorphism::NotIsomorphic(Mismatch::Colors),
        "Wrong result for graphs with equal total sequence length"
    );
}

//-----------------------------------------------------------------------------

#[test]
fn chopped_graphs_are_not_isomorphic() {
    // `translation.gbz` is `translation.gfa` with the nodes chopped to at most 2 bp, and without
    // the segment that is not on any path. Node-level isomorphism cannot see these as the same
    // pangenome. This becomes an isomorphism once unitig-level isomorphism is implemented.
    let from_gfa = {
        let path = support::get_test_data("translation.gfa");
        let file = OpenOptions::new().read(true).open(&path).unwrap();
        let graph: GraphStr = algorithms::parse_gfa(BufReader::new(file)).unwrap();
        IndexedGraph::from(&graph)
    };
    let gbz = gbz_file("translation.gbz");
    let from_gbz = GbzTopology::new(&gbz).unwrap();

    let result = are_isomorphic(&from_gfa, &from_gbz, &options(false));
    assert_eq!(
        result, Isomorphism::NotIsomorphic(Mismatch::NodeCount),
        "Wrong result for a chopped graph"
    );
}

#[test]
fn empty_graphs_are_isomorphic() {
    let graph = IndexedGraph::new();
    let result = are_isomorphic(&graph, &graph, &options(false));
    match result {
        Isomorphism::Isomorphic(mapping) => assert!(mapping.is_empty(), "The mapping should be empty"),
        other => panic!("Empty graphs should be isomorphic, got {}", other),
    }
}

#[test]
fn self_loops_are_distinguished() {
    // A self-loop at one side is not the same as an edge to a twin node, even though the degree
    // pairs can agree.
    let loop_graph = gfa_text("S\t1\tGAT\nS\t2\tTA\nL\t1\t+\t2\t+\nL\t2\t+\t2\t-\n");
    let twin_graph = gfa_text("S\t1\tGAT\nS\t2\tTA\nS\t3\tTA\nL\t1\t+\t2\t+\nL\t2\t+\t3\t-\n");
    for allow_flips in [false, true] {
        let result = are_isomorphic(&loop_graph, &twin_graph, &options(allow_flips));
        assert!(
            matches!(result, Isomorphism::NotIsomorphic(_)),
            "A self-loop was confused with a twin node (flips {}), got {}", allow_flips, result
        );
    }

    // All three kinds of self-loop survive a permutation.
    let graph = gfa_text(
        "S\t1\tGAT\nS\t2\tTAC\nS\t3\tCAG\nL\t1\t+\t1\t+\nL\t2\t+\t2\t-\nL\t3\t-\t3\t+\n"
    );
    let mut rng = StdRng::seed_from_u64(SEEDS[0]);
    let permutation = random_permutation(graph.nodes(), &mut rng);
    let permuted = permute(&graph, &permutation, &no_flips(graph.nodes()));
    check_isomorphic(&graph, &permuted, false, "a graph with self-loops");
}

//-----------------------------------------------------------------------------

//-----------------------------------------------------------------------------

// Builds a graph from an undirected edge list, with every edge joining two right sides.
//
// Every node then has degree profile `(0, d)` and no self-loop, so on a regular graph with equal
// sequences the refinement has nothing to work with. This turns the graph into a pure test of the
// search.
fn undirected_graph(nodes: usize, edges: &[(usize, usize)], sequence: &str) -> IndexedGraph {
    let mut result = IndexedGraph::new();
    for node in 0..nodes {
        result.add_node((node + 1).to_string().as_bytes(), sequence.as_bytes());
    }
    for &(from, to) in edges.iter() {
        result.add_edge(from, Orientation::Forward, to, Orientation::Reverse);
    }
    result.finalize();
    result
}

// Both parts of K(3,3), fully connected.
const COMPLETE_BIPARTITE: &[(usize, usize)] = &[
    (0, 3), (0, 4), (0, 5), (1, 3), (1, 4), (1, 5), (2, 3), (2, 4), (2, 5),
];

// Two triangles joined by three rungs.
const PRISM: &[(usize, usize)] = &[
    (0, 1), (1, 2), (2, 0), (3, 4), (4, 5), (5, 3), (0, 3), (1, 4), (2, 5),
];

// Cycles of the given lengths, laid end to end.
fn cycles(lengths: &[usize]) -> (usize, Vec<(usize, usize)>) {
    let mut edges = Vec::new();
    let mut offset = 0;
    for &len in lengths.iter() {
        for i in 0..len {
            edges.push((offset + i, offset + (i + 1) % len));
        }
        offset += len;
    }
    (offset, edges)
}

// The sequence is a palindrome, so it cannot tell the sides apart even when flips are allowed.
const PALINDROME: &str = "AT";

//-----------------------------------------------------------------------------

#[test]
fn refinement_cannot_separate_regular_graphs() {
    let (cycle_nodes, cycle_edges) = cycles(&[6]);
    let (split_nodes, split_edges) = cycles(&[3, 3]);
    let pairs: Vec<(&str, IndexedGraph, IndexedGraph)> = vec![
        (
            "K(3,3) and the prism",
            undirected_graph(6, COMPLETE_BIPARTITE, PALINDROME),
            undirected_graph(6, PRISM, PALINDROME),
        ),
        (
            "a 6-cycle and two 3-cycles",
            undirected_graph(cycle_nodes, &cycle_edges, PALINDROME),
            undirected_graph(split_nodes, &split_edges, PALINDROME),
        ),
    ];

    for (description, first, second) in pairs.iter() {
        for allow_flips in [false, true] {
            let settings = options(allow_flips);

            // The refinement must not be able to tell the graphs apart. Without this, the test
            // would silently become an early-reject test and stop exercising the search.
            let first_coloring = coloring::Coloring::new(first, &settings);
            let second_coloring = coloring::Coloring::new(second, &settings);
            assert_eq!(
                first_coloring.signature(), second_coloring.signature(),
                "The refinement separated {} (flips {})", description, allow_flips
            );

            // With a budget, the search must settle the question.
            let result = are_isomorphic(first, second, &settings);
            assert_eq!(
                result, Isomorphism::NotIsomorphic(Mismatch::Structure),
                "Wrong result for {} (flips {})", description, allow_flips
            );

            // Without a budget, the question cannot be settled.
            let result = are_isomorphic(
                first, second, &Options { search_budget: 0, ..settings }
            );
            assert_eq!(
                result, Isomorphism::Unresolved,
                "Wrong result for {} without a budget (flips {})", description, allow_flips
            );
        }
    }
}

#[test]
fn search_finds_an_isomorphism_by_backtracking() {
    // The same graphs as above, so the refinement is of no help, but now they are isomorphic.
    let graph = undirected_graph(6, COMPLETE_BIPARTITE, PALINDROME);
    for &seed in SEEDS.iter() {
        let mut rng = StdRng::seed_from_u64(seed);
        let permutation = random_permutation(graph.nodes(), &mut rng);
        let permuted = permute(&graph, &permutation, &no_flips(graph.nodes()));

        let (result, statistics) = are_isomorphic_with_statistics(&graph, &permuted, &options(false));
        let mapping = match result {
            Isomorphism::Isomorphic(mapping) => mapping,
            other => panic!("Expected an isomorphism for K(3,3) (seed {}), got {}", seed, other),
        };
        assert!(
            is_isomorphism(&graph, &permuted, &mapping, false).is_ok(),
            "The mapping for K(3,3) is not an isomorphism (seed {})", seed
        );
        assert!(
            statistics.individualizations > 0,
            "The search should have made choices for K(3,3) (seed {})", seed
        );
    }
}

//-----------------------------------------------------------------------------

// Builds a chain of bubbles. Each bubble has two alleles with the same sequence, so it has a
// nontrivial automorphism, and a chain of `count` bubbles has `2^count` automorphisms.
fn bubble_chain(count: usize) -> IndexedGraph {
    let mut result = IndexedGraph::new();
    let mut spine = vec![result.add_node(b"start", b"GGGGA")];
    for bubble in 0..count {
        // Distinct spine sequences, so the bubbles themselves are easy to tell apart.
        let sequence = format!("C{}A", "T".repeat(bubble + 1));
        let next = result.add_node(format!("s{}", bubble).as_bytes(), sequence.as_bytes());
        let left = result.add_node(format!("a{}", bubble).as_bytes(), b"GAG");
        let right = result.add_node(format!("b{}", bubble).as_bytes(), b"GAG");
        let previous = *spine.last().unwrap();
        for allele in [left, right] {
            result.add_edge(previous, Orientation::Forward, allele, Orientation::Forward);
            result.add_edge(allele, Orientation::Forward, next, Orientation::Forward);
        }
        spine.push(next);
    }
    result.finalize();
    result
}

#[test]
fn many_automorphisms_do_not_cause_backtracking() {
    // A chain of 12 bubbles has 4096 automorphisms. Any choice extends to an isomorphism, so the
    // search must succeed on the first try every time.
    let count = 12;
    let graph = bubble_chain(count);
    let mut rng = StdRng::seed_from_u64(SEEDS[0]);
    let permutation = random_permutation(graph.nodes(), &mut rng);
    let permuted = permute(&graph, &permutation, &no_flips(graph.nodes()));

    let (result, statistics) = are_isomorphic_with_statistics(&graph, &permuted, &options(false));
    match result {
        Isomorphism::Isomorphic(mapping) => assert!(
            is_isomorphism(&graph, &permuted, &mapping, false).is_ok(),
            "The mapping for a bubble chain is not an isomorphism"
        ),
        other => panic!("Expected an isomorphism for a bubble chain, got {}", other),
    }
    // One choice per bubble, not one per automorphism.
    assert!(
        statistics.individualizations <= count,
        "The search made {} choices for {} bubbles", statistics.individualizations, count
    );
}

#[test]
fn identical_isolated_nodes_are_matched_directly() {
    // Isolated nodes with the same sequence are interchangeable, so they must not become choice
    // points. Without that, this graph would exhaust any reasonable budget.
    let count = 1000;
    let mut graph = IndexedGraph::new();
    for node in 0..count {
        graph.add_node((node + 1).to_string().as_bytes(), b"GATTACA");
    }
    graph.finalize();

    let mut rng = StdRng::seed_from_u64(SEEDS[0]);
    let permutation = random_permutation(graph.nodes(), &mut rng);
    let permuted = permute(&graph, &permutation, &no_flips(graph.nodes()));

    for allow_flips in [false, true] {
        let (result, statistics) = are_isomorphic_with_statistics(&graph, &permuted, &options(allow_flips));
        match result {
            Isomorphism::Isomorphic(mapping) => assert!(
                is_isomorphism(&graph, &permuted, &mapping, allow_flips).is_ok(),
                "The mapping for isolated nodes is not an isomorphism (flips {})", allow_flips
            ),
            other => panic!("Expected an isomorphism for isolated nodes (flips {}), got {}", allow_flips, other),
        }
        assert_eq!(
            statistics.individualizations, 0,
            "Isolated nodes should not become choice points (flips {})", allow_flips
        );
    }
}

#[test]
fn palindromic_components_find_the_flip() {
    // Every sequence is a palindrome, so the refinement cannot decide the orientations. With flips
    // allowed, the mapping still has to be found.
    let mut graph = IndexedGraph::new();
    let sequences: [&[u8]; 5] = [b"AT", b"GC", b"ACGT", b"GGCC", b"TTAA"];
    for (node, sequence) in sequences.iter().enumerate() {
        graph.add_node((node + 1).to_string().as_bytes(), sequence);
    }
    for node in 1..sequences.len() {
        graph.add_edge(node - 1, Orientation::Forward, node, Orientation::Forward);
    }
    graph.finalize();

    for &seed in SEEDS.iter() {
        let mut rng = StdRng::seed_from_u64(seed);
        let permutation = random_permutation(graph.nodes(), &mut rng);
        let flips = random_flips(graph.nodes(), &mut rng);
        let permuted = permute(&graph, &permutation, &flips);
        check_isomorphic(&graph, &permuted, true, &format!("a palindromic path, seed {}", seed));
    }
}

#[test]
fn disconnected_components_are_matched() {
    for &seed in SEEDS.iter() {
        let mut rng = StdRng::seed_from_u64(seed);
        // Two copies of the same component, plus a different one.
        let mut graph = IndexedGraph::new();
        for copy in 0..3 {
            let length = if copy < 2 { 4 } else { 5 };
            let first = graph.nodes();
            for node in 0..length {
                graph.add_node(format!("{}_{}", copy, node).as_bytes(), b"GATT");
            }
            for node in 1..length {
                graph.add_edge(first + node - 1, Orientation::Forward, first + node, Orientation::Forward);
            }
        }
        graph.finalize();

        let permutation = random_permutation(graph.nodes(), &mut rng);
        let permuted = permute(&graph, &permutation, &no_flips(graph.nodes()));
        check_isomorphic(&graph, &permuted, false, &format!("disconnected components, seed {}", seed));
    }
}

//-----------------------------------------------------------------------------

//-----------------------------------------------------------------------------

#[test]
fn realistic_graph() {
    // A real pangenome graph. The sequences make the refinement discrete, so the mapping should be
    // found without any search.
    let gbz = gbz_file("micb-kir3dl1.gbz");
    let topology = GbzTopology::new(&gbz).unwrap();

    // The permuted copy has to be built in memory, as this crate cannot write GBZ files.
    let indexed = IndexedGraph::from_topology(&topology);
    let mut rng = StdRng::seed_from_u64(SEEDS[0]);
    let permutation = random_permutation(indexed.nodes(), &mut rng);

    for allow_flips in [false, true] {
        let flips = if allow_flips {
            random_flips(indexed.nodes(), &mut rng)
        } else {
            no_flips(indexed.nodes())
        };
        let permuted = permute(&indexed, &permutation, &flips);

        let (result, statistics) = are_isomorphic_with_statistics(
            &topology, &permuted, &options(allow_flips)
        );
        let mapping = match result {
            Isomorphism::Isomorphic(mapping) => mapping,
            other => panic!("Expected an isomorphism for micb-kir3dl1.gbz (flips {}), got {}", allow_flips, other),
        };
        assert!(
            is_isomorphism(&topology, &permuted, &mapping, allow_flips).is_ok(),
            "The mapping for micb-kir3dl1.gbz is not an isomorphism (flips {})", allow_flips
        );
        assert_eq!(
            statistics.individualizations, 0,
            "The search should not have been needed for micb-kir3dl1.gbz (flips {})", allow_flips
        );
        assert_eq!(
            mapping, mapping_of(&permutation, &flips),
            "Wrong mapping for micb-kir3dl1.gbz (flips {})", allow_flips
        );
    }
}

#[test]
#[ignore = "slow; run with cargo test -- --ignored"]
fn large_graph() {
    // A graph large enough to show that the algorithm is not accidentally quadratic.
    let nodes = 1_000_000;
    let mut rng = StdRng::seed_from_u64(SEEDS[0]);
    let mut graph = IndexedGraph::new();
    for node in 0..nodes {
        graph.add_node((node + 1).to_string().as_bytes(), &random_sequence(32, &mut rng));
    }
    for node in 1..nodes {
        graph.add_edge(node - 1, Orientation::Forward, node, Orientation::Forward);
        if node % 3 == 0 {
            graph.add_edge(node - 1, Orientation::Forward, node / 3, Orientation::Forward);
        }
    }
    graph.finalize();

    let permutation = random_permutation(graph.nodes(), &mut rng);
    let flips = random_flips(graph.nodes(), &mut rng);
    let permuted = permute(&graph, &permutation, &flips);

    let (result, statistics) = are_isomorphic_with_statistics(&graph, &permuted, &options(true));
    assert!(result.is_isomorphic(), "Expected an isomorphism for a large graph, got {}", result);
    assert_eq!(statistics.individualizations, 0, "The search should not have been needed");
}

//-----------------------------------------------------------------------------
