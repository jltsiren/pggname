//! Shared helpers for tests.
//!
//! The generators are seeded explicitly, so that a failing test can be reproduced. Every assertion
//! built on them should include the seed in its message.

use crate::isomorphism::NodeMapping;
use crate::isomorphism::hashing;
use crate::topology::{IndexedGraph, Topology};

use gbz::{NodeSide, Orientation};
use gbz::support;

use rand::Rng;

//-----------------------------------------------------------------------------

/// Seeds used by the randomized tests.
pub const SEEDS: &[u64] = &[0x5eed, 0x1234_5678, 0xdead_beef, 0x0f0f_0f0f, 1];

/// Returns a random orientation.
pub fn random_orientation(rng: &mut impl Rng) -> Orientation {
    if rng.random_bool(0.5) { Orientation::Forward } else { Orientation::Reverse }
}

/// Returns a random sequence of the given length over `ACGT`.
pub fn random_sequence(len: usize, rng: &mut impl Rng) -> Vec<u8> {
    (0..len).map(|_| b"ACGT"[rng.random_range(0..4)]).collect()
}

/// Returns a random graph with the given numbers of nodes and edges.
///
/// The sequences are short, so duplicates are common. This matters: if every sequence is distinct,
/// the initial coloring solves every instance and neither refinement nor search is ever exercised.
pub fn random_graph(nodes: usize, edges: usize, rng: &mut impl Rng) -> IndexedGraph {
    let mut result = IndexedGraph::new();
    for node in 0..nodes {
        let len = rng.random_range(1..4);
        result.add_node((node + 1).to_string().as_bytes(), &random_sequence(len, rng));
    }
    for _ in 0..edges {
        let from = rng.random_range(0..nodes);
        let to = rng.random_range(0..nodes);
        result.add_edge(from, random_orientation(rng), to, random_orientation(rng));
    }
    result.finalize();
    result
}

/// Returns a random graph where every node has a distinct sequence and the graph is a path.
///
/// Such a graph is rigid: it has no automorphisms, so the isomorphism is unique.
pub fn rigid_graph(nodes: usize) -> IndexedGraph {
    let mut result = IndexedGraph::new();
    for node in 0..nodes {
        // Distinct sequences that are not reverse complements of each other.
        let sequence = format!("A{}C", "G".repeat(node + 1));
        result.add_node((node + 1).to_string().as_bytes(), sequence.as_bytes());
    }
    for node in 1..nodes {
        result.add_edge(node - 1, Orientation::Forward, node, Orientation::Forward);
    }
    result.finalize();
    result
}

/// Returns a random permutation of `0..len`.
pub fn random_permutation(len: usize, rng: &mut impl Rng) -> Vec<usize> {
    let mut result: Vec<usize> = (0..len).collect();
    for i in (1..len).rev() {
        result.swap(i, rng.random_range(0..=i));
    }
    result
}

/// Returns a random vector of flips.
pub fn random_flips(len: usize, rng: &mut impl Rng) -> Vec<bool> {
    (0..len).map(|_| rng.random_bool(0.5)).collect()
}

/// Returns a vector of no flips.
pub fn no_flips(len: usize) -> Vec<bool> {
    vec![false; len]
}

//-----------------------------------------------------------------------------

/// Builds a copy of the graph with the nodes permuted and some of them reverse complemented.
///
/// Node `i` of the source becomes node `permutation[i]` of the result. If `flips[i]` is set, the
/// sequence is reverse complemented and the two sides of the node are swapped, which changes the
/// side of every edge endpoint at that node.
pub fn permute<T: Topology>(source: &T, permutation: &[usize], flips: &[bool]) -> IndexedGraph {
    let nodes = source.nodes();
    let mut inverse = vec![0; nodes];
    for (node, &image) in permutation.iter().enumerate() {
        inverse[image] = node;
    }

    let mut result = IndexedGraph::new();
    for (node, &original) in inverse.iter().enumerate() {
        let sequence = source.sequence(original);
        let sequence = if flips[original] {
            hashing::reverse_complement(&sequence)
        } else {
            sequence.to_vec()
        };
        result.add_node(format!("x{}", node).as_bytes(), &sequence);
    }

    for node in 0..nodes {
        for side in [NodeSide::Left, NodeSide::Right] {
            let from_side = if flips[node] { side.flip() } else { side };
            for (neighbor, neighbor_side) in source.neighbors(node, side) {
                let to_side = if flips[neighbor] { neighbor_side.flip() } else { neighbor_side };
                result.add_edge(
                    permutation[node], support::exit_orientation(from_side),
                    permutation[neighbor], support::entry_orientation(to_side)
                );
            }
        }
    }
    result.finalize();

    result
}

/// Returns the mapping induced by a permutation and a vector of flips.
pub fn mapping_of(permutation: &[usize], flips: &[bool]) -> NodeMapping {
    let encoded: Vec<u32> = permutation.iter().zip(flips.iter())
        .map(|(&image, &flip)| (2 * image + (flip as usize)) as u32)
        .collect();
    NodeMapping::from_encoded(encoded)
}

//-----------------------------------------------------------------------------

/// Checks that the mapping is an isomorphism between the two graphs.
///
/// This is written independently of `isomorphism::verify`, so that a bug in the production
/// verification cannot hide itself.
pub fn is_isomorphism<A: Topology, B: Topology>(
    first: &A, second: &B, mapping: &NodeMapping, allow_flips: bool
) -> Result<(), String> {
    if first.nodes() != second.nodes() {
        return Err(format!("Node counts {} and {} differ", first.nodes(), second.nodes()));
    }
    if mapping.len() != first.nodes() {
        return Err(format!("The mapping covers {} of {} nodes", mapping.len(), first.nodes()));
    }

    // The mapping must be a bijection.
    let mut seen = vec![false; second.nodes()];
    for (source, destination, _) in mapping.iter() {
        if destination >= second.nodes() {
            return Err(format!("Node {} maps outside the graph", source));
        }
        if seen[destination] {
            return Err(format!("Two nodes map to node {}", destination));
        }
        seen[destination] = true;
    }

    for (source, destination, orientation) in mapping.iter() {
        if orientation == Orientation::Reverse && !allow_flips {
            return Err(format!("Node {} is flipped, but flips are not allowed", source));
        }

        let sequence = first.sequence(source).to_vec();
        let expected = match orientation {
            Orientation::Forward => sequence,
            Orientation::Reverse => hashing::reverse_complement(&sequence),
        };
        if expected != second.sequence(destination).to_vec() {
            return Err(format!("Wrong sequence for the image of node {}", source));
        }

        for side in [NodeSide::Left, NodeSide::Right] {
            let image_side = if orientation == Orientation::Reverse { side.flip() } else { side };
            // Compare the neighbors as sets, and the degrees separately, so that a missing
            // neighbor cannot be hidden by a duplicate.
            if first.degree(source, side) != second.degree(destination, image_side) {
                return Err(format!("Wrong degree for the image of node {}, side {:?}", source, side));
            }
            let mut expected: Vec<(usize, NodeSide)> = first.neighbors(source, side)
                .map(|(neighbor, neighbor_side)| {
                    let (image, image_orientation) = mapping.get(neighbor);
                    let image_side = if image_orientation == Orientation::Reverse {
                        neighbor_side.flip()
                    } else {
                        neighbor_side
                    };
                    (image, image_side)
                })
                .collect();
            let mut found: Vec<(usize, NodeSide)> = second.neighbors(destination, image_side).collect();
            expected.sort_unstable();
            found.sort_unstable();
            if expected != found {
                return Err(format!("Wrong neighbors for the image of node {}, side {:?}", source, side));
            }
        }
    }

    Ok(())
}

//-----------------------------------------------------------------------------
