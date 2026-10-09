//! Color refinement.
//!
//! The nodes are colored by hashing isomorphism-invariant data.
//! The colors are then refined by one-dimensional Weisfeiler-Leman refinement over node sides.
//! Corresponding nodes in isomorphic graphs always get the same color
//! A difference in the multiset of colors proves that the graphs are not isomorphic.
//!
//! The refinement works on node sides.
//! Each side gets a color, and the color of a node is derived from the colors of its two sides.
//! Because a node may map to the reverse complement of another node, the two sides of a node are interchangeable a priori.
//! Node color is therefore built from the unordered pair of side colors.
//! The side with the smaller color becomes the head side.
//!
//! A symmetric node has two sides with the same color.
//! Such a node carries a free choice of relative orientation.
//! Because an edge determines the relative orientation of its endpoints, this costs at most one binary choice per connected component.

use crate::topology::Topology;

use super::{Mismatch, Options};
use super::hashing::{self, combine};

use gbz::NodeSide;
use gbz::support;

use simple_sds::raw_vector::{RawVector, AccessRaw};

//-----------------------------------------------------------------------------

/// An isomorphism-invariant coloring of the nodes of a graph.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Coloring {
    // Color of each node.
    colors: Vec<u64>,
    // `true` if the right side is the head side of the node.
    head_is_right: RawVector,
    // `true` if the two sides of the node cannot be told apart.
    symmetric: RawVector,
    // Number of refinement rounds actually performed.
    rounds: usize,
    // Number of distinct colors.
    classes: usize,
}

impl Coloring {
    /// Computes the coloring of the given graph.
    pub(crate) fn new<T: Topology>(graph: &T, options: &Options) -> Self {
        let nodes = graph.nodes();
        let mut result = Coloring {
            colors: vec![0; nodes],
            head_is_right: RawVector::with_len(nodes, false),
            symmetric: RawVector::with_len(nodes, false),
            rounds: 0,
            classes: 0,
        };

        let (left, right) = Self::initial_colors(graph);
        result.update(left, right);

        let mut scratch = Vec::new();
        result.classes = Self::count_classes(&result.colors, &mut scratch);
        for _ in 0..options.refinement_rounds {
            // The coloring cannot be refined further once every node has a distinct color.
            if result.classes >= nodes {
                break;
            }
            let (left, right) = result.refined_colors(graph);
            let mut candidate = result.clone();
            candidate.update(left, right);
            let classes = Self::count_classes(&candidate.colors, &mut scratch);
            // A round that does not split any class cannot be followed by one that does.
            if classes <= result.classes {
                break;
            }
            candidate.rounds = result.rounds + 1;
            candidate.classes = classes;
            result = candidate;
        }

        result
    }

    /// Returns the number of nodes.
    pub(crate) fn len(&self) -> usize {
        self.colors.len()
    }

    /// Returns the number of refinement rounds performed.
    pub(crate) fn rounds(&self) -> usize {
        self.rounds
    }

    /// Returns the number of distinct colors.
    pub(crate) fn classes(&self) -> usize {
        self.classes
    }

    /// Returns the color of the given node.
    pub(crate) fn color(&self, node: usize) -> u64 {
        self.colors[node]
    }

    /// Returns `true` if the two sides of the node cannot be told apart.
    pub(crate) fn is_symmetric(&self, node: usize) -> bool {
        self.symmetric.bit(node)
    }

    /// Returns the head side of the given node.
    pub(crate) fn head_side(&self, node: usize) -> NodeSide {
        if self.head_is_right.bit(node) { NodeSide::Right } else { NodeSide::Left }
    }

    /// Returns the color of the given side of the given node.
    ///
    /// The color is derived from the current color of the node.
    /// Both sides of a symmetric node get the same color.
    pub(crate) fn side_color(&self, node: usize, side: NodeSide) -> u64 {
        let bit = if self.symmetric.bit(node) { 0 } else { (side != self.head_side(node)) as u64 };
        combine(self.colors[node], bit)
    }

    /// Returns an order-independent signature of the multiset of colors.
    ///
    /// Isomorphic graphs always get the same signature.
    /// Some non-isomorphic graphs cannot be told apart in a limited number of color refinement rounds.
    /// Two disctinct multisets may also get the same hash signature.
    pub(crate) fn signature(&self) -> (u64, u64) {
        let mut sum = 0u64;
        let mut sum_of_squares = 0u64;
        for &color in self.colors.iter() {
            let value = hashing::mix(color ^ 0x243f6a8885a308d3);
            sum = sum.wrapping_add(value);
            sum_of_squares = sum_of_squares.wrapping_add(value.wrapping_mul(value));
        }
        (sum, sum_of_squares)
    }

    // Derives the node colors, head sides, and symmetry from the side colors.
    fn update(&mut self, left: Vec<u64>, right: Vec<u64>) {
        for node in 0..self.colors.len() {
            let (left, right) = (left[node], right[node]);
            // The sides are interchangeable a priori, so the pair must be unordered.
            self.colors[node] = combine(left.min(right), right.max(left));
            self.head_is_right.set_bit(node, right < left);
            self.symmetric.set_bit(node, left == right);
        }
    }

    // Computes the side colors for the next round.
    // Each node side gets an initial color derived from the current color of the node.
    // The actual color is obtained by combining it with the colors of its neighbors, including the other side.
    fn refined_colors<T: Topology>(&self, graph: &T) -> (Vec<u64>, Vec<u64>) {
        let nodes = graph.nodes();
        let mut left = vec![0; nodes];
        let mut right = vec![0; nodes];
        let mut buffer: Vec<u64> = Vec::new();

        for node in 0..nodes {
            let own = [self.side_color(node, NodeSide::Left), self.side_color(node, NodeSide::Right)];
            for side in [NodeSide::Left, NodeSide::Right] {
                buffer.clear();
                for (neighbor, neighbor_side) in graph.neighbors(node, side) {
                    buffer.push(self.side_color(neighbor, neighbor_side));
                }
                buffer.sort_unstable();

                // Include the color of the other side, so that information passes through the node.
                let index = side as usize;
                let mut color = combine(own[index], own[1 - index]);
                for &neighbor_color in buffer.iter() {
                    color = combine(color, neighbor_color);
                }
                if side == NodeSide::Left { left[node] = color; } else { right[node] = color; }
            }
        }

        (left, right)
    }

    // Computes the side colors for the first round.
    // The initial color of a node is a hash of the canonical sequence, with the entry side becoming the head side.
    // This is modified if there is a self-loop connecting the sides.
    // The initial color of each side is derived from that, with special handling for palindromes.
    // The final color is further modified by the presence of self-loops and the degree of the side.
    fn initial_colors<T: Topology>(graph: &T) -> (Vec<u64>, Vec<u64>) {
        let nodes = graph.nodes();
        let mut left = vec![0; nodes];
        let mut right = vec![0; nodes];

        for node in 0..nodes {
            let sequence = graph.sequence(node);
            let key = hashing::hash_canonical_sequence(&sequence);
            let palindrome = hashing::is_palindrome(&sequence);
            let reference = hashing::canonical_orientation(&sequence);
            // The side the sequence starts from gets bit 0. For a palindrome, the sequence cannot tell
            // the sides apart, so both get bit 0.
            let head = support::entry_side(reference);

            // A self-loop joining the two sides of the node is a property of the node, while a
            // self-loop at a single side is a property of that side. Flipping a node swaps its sides,
            // so the two must be kept separate.
            let (through_loop, side_loops) = Self::loop_flags(graph, node);
            let node_key = combine(key, through_loop as u64);

            for side in [NodeSide::Left, NodeSide::Right] {
                let bit = if palindrome { 0 } else { (side != head) as u64 };
                let mut color = combine(node_key, bit);
                color = combine(color, side_loops[side as usize] as u64);
                color = combine(color, graph.degree(node, side) as u64);
                if side == NodeSide::Left { left[node] = color; } else { right[node] = color; }
            }
        }

        (left, right)
    }

    // Returns (self-loop between sides, [self-loop at left side, self-loop at right side]).
    fn loop_flags<T: Topology>(graph: &T, node: usize) -> (bool, [bool; 2]) {
        let mut through = false;
        let mut sides = [false; 2];
        for side in [NodeSide::Left, NodeSide::Right] {
            for (neighbor, neighbor_side) in graph.neighbors(node, side) {
                if neighbor == node {
                    if neighbor_side == side {
                        sides[side as usize] = true;
                    } else {
                        through = true;
                    }
                }
            }
        }
        (through, sides)
    }

    // Returns the number of distinct colors, using the buffer as scratch space.
    fn count_classes(colors: &[u64], buffer: &mut Vec<u64>) -> usize {
        buffer.clear();
        buffer.extend_from_slice(colors);
        buffer.sort_unstable();
        buffer.dedup();
        buffer.len()
    }
}

//-----------------------------------------------------------------------------

/// A mapping from node colorings in two graphs to color classes for each node.
///
/// A color class is an integer in `0..num_classes()`.
/// This assumes that the multisets of colors in the two graphs are identical.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Classes {
    // Class of each node in the first graph.
    in_first: Vec<u32>,
    // Class of each node in the second graph.
    in_second: Vec<u32>,
    // Number of nodes in each class.
    sizes: Vec<u32>,
}

impl Classes {
    /// Builds the shared color classes of two colorings.
    ///
    /// Returns [`Mismatch::Colors`] if the graphs have different multisets of colors, which proves
    /// that they are not isomorphic.
    pub(crate) fn new(first: &Coloring, second: &Coloring) -> Result<Self, Mismatch> {
        if first.len() != second.len() {
            return Err(Mismatch::Colors);
        }
        let sorted_first = Self::sorted_colors(first);
        let sorted_second = Self::sorted_colors(second);

        let mut result = Classes {
            in_first: vec![0; first.len()],
            in_second: vec![0; second.len()],
            sizes: Vec::new(),
        };

        // Check that both colorings have the same number of nodes for each color.
        // Assign nodes to color classes.
        let mut class = 0;
        let mut prev_color = None;
        for ((first_color, first_node), (second_color, second_node)) in sorted_first.iter().zip(sorted_second.iter()) {
            if first_color != second_color {
                return Err(Mismatch::Colors);
            }
            if Some(*first_color) != prev_color {
                class = result.sizes.len() as u32;
                result.sizes.push(0);
                prev_color = Some(*first_color);
            }
            result.in_first[*first_node as usize] = class;
            result.in_second[*second_node as usize] = class;
            result.sizes[class as usize] += 1;
        }

        Ok(result)
    }

    /// Returns the number of classes.
    pub(crate) fn len(&self) -> usize {
        self.sizes.len()
    }

    /// Returns the class of the given node in the first graph.
    pub(crate) fn in_first(&self, node: usize) -> u32 {
        self.in_first[node]
    }

    /// Returns the class of the given node in the second graph.
    pub(crate) fn in_second(&self, node: usize) -> u32 {
        self.in_second[node]
    }

    /// Returns the number of nodes in the given class in either graph.
    pub(crate) fn size(&self, class: u32) -> usize {
        self.sizes[class as usize] as usize
    }

    /// Returns the class of each node in the first graph.
    pub(crate) fn first_classes(&self) -> &[u32] {
        &self.in_first
    }

    /// Returns the class of each node in the second graph.
    pub(crate) fn second_classes(&self) -> &[u32] {
        &self.in_second
    }

    // Returns the (color, node) pairs of the coloring in sorted order.
    fn sorted_colors(coloring: &Coloring) -> Vec<(u64, u32)> {
        let mut result: Vec<(u64, u32)> = (0..coloring.len())
            .map(|node| (coloring.color(node), node as u32))
            .collect();
        result.sort_unstable();
        result
    }
}

//-----------------------------------------------------------------------------

#[cfg(test)]
mod tests
{
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

    fn signature_of<T: Topology>(graph: &T) -> (u64, u64) {
        Coloring::new(graph, &Options::default()).signature()
    }

    fn gfa_text(text: &str) -> IndexedGraph {
        let graph: GraphStr = algorithms::parse_gfa(BufReader::new(text.as_bytes())).unwrap();
        IndexedGraph::from(&graph)
    }

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
}

//-----------------------------------------------------------------------------
