//! Color refinement.
//!
//! The nodes are colored by hashing isomorphism-invariant data, and the colors are then refined by
//! one-dimensional Weisfeiler-Leman refinement over node sides. Corresponding nodes in isomorphic
//! graphs always get the same color, so a difference in the multiset of colors proves that the
//! graphs are not isomorphic.
//!
//! The refinement works on node sides rather than nodes. Each side gets a color, and the color of a
//! node is derived from the colors of its two sides. Because a node may map to the reverse
//! complement of another node, the two sides of a node are interchangeable a priori. The node color
//! is therefore built from the *unordered* pair of side colors, and the side the smaller color
//! belongs to becomes the head side.
//!
//! A node whose two sides have the same color is *symmetric*: the refinement cannot tell its sides
//! apart. Such a node carries a free choice of relative orientation. Because an edge determines the
//! relative orientation of its endpoints, this costs at most one binary choice per connected
//! component, and only when no node in the component is asymmetric.

use crate::topology::Topology;

use super::{Mismatch, Options};
use super::hashing::{self, combine};

use gbz::NodeSide;
use gbz::support;

#[cfg(test)]
mod tests;

//-----------------------------------------------------------------------------

/// An isomorphism-invariant coloring of the nodes of a graph.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Coloring {
    // Color of each node.
    colors: Vec<u64>,
    // `true` if the right side is the head side of the node.
    // These could be bitvectors, which would matter for very large graphs.
    head_is_right: Vec<bool>,
    // `true` if the two sides of the node cannot be told apart.
    symmetric: Vec<bool>,
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
            head_is_right: vec![false; nodes],
            symmetric: vec![false; nodes],
            rounds: 0,
            classes: 0,
        };

        let (left, right) = initial_colors(graph);
        result.update(left, right);

        let mut scratch = Vec::new();
        result.classes = count_classes(&result.colors, &mut scratch);
        for _ in 0..options.refinement_rounds {
            // The coloring cannot be refined further once every node has a distinct color.
            if result.classes >= nodes {
                break;
            }
            let (left, right) = result.refined_colors(graph);
            let mut candidate = result.clone();
            candidate.update(left, right);
            let classes = count_classes(&candidate.colors, &mut scratch);
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
        self.symmetric[node]
    }

    /// Returns the head side of the given node.
    pub(crate) fn head_side(&self, node: usize) -> NodeSide {
        if self.head_is_right[node] { NodeSide::Right } else { NodeSide::Left }
    }

    /// Returns the color of the given side of the given node.
    ///
    /// Both sides of a symmetric node get the same color.
    pub(crate) fn side_color(&self, node: usize, side: NodeSide) -> u64 {
        let bit = if self.symmetric[node] { 0 } else { (side != self.head_side(node)) as u64 };
        combine(self.colors[node], bit)
    }

    /// Returns an order-independent signature of the multiset of colors.
    ///
    /// Isomorphic graphs always get the same signature. The converse does not hold, both because
    /// the refinement cannot tell all graphs apart and because the signature is a hash.
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
            self.head_is_right[node] = right < left;
            self.symmetric[node] = left == right;
        }
    }

    // Computes the side colors for the next round.
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
}

//-----------------------------------------------------------------------------

// Computes the side colors for the first round.
fn initial_colors<T: Topology>(graph: &T) -> (Vec<u64>, Vec<u64>) {
    let nodes = graph.nodes();
    let mut left = vec![0; nodes];
    let mut right = vec![0; nodes];

    for node in 0..nodes {
        let sequence = graph.sequence(node);
        let key = hashing::hash_canonical_sequence(&sequence);
        let palindrome = hashing::is_palindrome(&sequence);
        let reference = hashing::reference_orientation(&sequence);
        // The side the sequence starts from gets bit 0. For a palindrome, the sequence cannot tell
        // the sides apart, so both get bit 0.
        let head = support::entry_side(reference);

        // A self-loop joining the two sides of the node is a property of the node, while a
        // self-loop at a single side is a property of that side. Flipping a node swaps its sides,
        // so the two must be kept separate.
        let (through_loop, side_loops) = loop_flags(graph, node);
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

// Returns whether the node has a self-loop joining its two sides, and whether each side has a
// self-loop of its own.
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

//-----------------------------------------------------------------------------

/// Color classes shared by two colorings.
///
/// A class is a color that occurs in both graphs. The classes are numbered so that the same number
/// means the same color in both graphs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Classes {
    // Class of each node in the first graph.
    in_first: Vec<u32>,
    // Class of each node in the second graph.
    in_second: Vec<u32>,
    // Number of nodes in each class, which is the same in both graphs.
    sizes: Vec<u32>,
}

impl Classes {
    /// Builds the shared color classes of two colorings.
    ///
    /// Returns [`Mismatch::Colors`] if the graphs have different multisets of colors, which proves
    /// that they are not isomorphic.
    pub(crate) fn new(first: &Coloring, second: &Coloring) -> Result<Self, Mismatch> {
        let sorted_first = sorted_colors(first);
        let sorted_second = sorted_colors(second);

        let mut result = Classes {
            in_first: vec![0; first.len()],
            in_second: vec![0; second.len()],
            sizes: Vec::new(),
        };

        // Merge-join the runs of equal colors. Any color that occurs in one graph only, or that
        // occurs a different number of times in the two graphs, is a mismatch.
        let (mut i, mut j) = (0, 0);
        while i < sorted_first.len() || j < sorted_second.len() {
            if i >= sorted_first.len() || j >= sorted_second.len() {
                return Err(Mismatch::Colors);
            }
            let color = sorted_first[i].0;
            if color != sorted_second[j].0 {
                return Err(Mismatch::Colors);
            }
            let end_first = run_end(&sorted_first, i);
            let end_second = run_end(&sorted_second, j);
            if end_first - i != end_second - j {
                return Err(Mismatch::Colors);
            }

            let class = result.sizes.len() as u32;
            result.sizes.push((end_first - i) as u32);
            for &(_, node) in sorted_first[i..end_first].iter() {
                result.in_first[node as usize] = class;
            }
            for &(_, node) in sorted_second[j..end_second].iter() {
                result.in_second[node as usize] = class;
            }
            i = end_first;
            j = end_second;
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
}

// Returns the (color, node) pairs of the coloring in sorted order.
fn sorted_colors(coloring: &Coloring) -> Vec<(u64, u32)> {
    let mut result: Vec<(u64, u32)> = (0..coloring.len())
        .map(|node| (coloring.color(node), node as u32))
        .collect();
    result.sort_unstable();
    result
}

// Returns the end of the run of equal colors starting at the given offset.
fn run_end(sorted: &[(u64, u32)], start: usize) -> usize {
    let color = sorted[start].0;
    let mut end = start + 1;
    while end < sorted.len() && sorted[end].0 == color {
        end += 1;
    }
    end
}

//-----------------------------------------------------------------------------
