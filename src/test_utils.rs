//! Shared helpers for tests.
//!
//! The generators are seeded explicitly, so that a failing test can be reproduced. Every assertion
//! built on them should include the seed in its message.

use crate::isomorphism::NodeMapping;
use crate::isomorphism::translation::Translation;
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

/// Builds a copy of the graph with every node chopped into fragments of at most `max_len` bp.
///
/// The fragments of a node are chained left to right in the forward orientation, so the result
/// represents the same pangenome. It is the standard way in which two graphs can differ without
/// being different graphs.
pub fn chop<T: Topology>(source: &T, max_len: usize) -> IndexedGraph {
    assert!(max_len > 0, "The fragment length must be positive");
    let mut result = IndexedGraph::new();

    // The first and last fragment of each node, which inherit its left and right sides.
    let mut ends: Vec<(usize, usize)> = Vec::with_capacity(source.nodes());
    for node in 0..source.nodes() {
        let sequence = source.sequence(node);
        let mut first = None;
        let mut previous = None;
        let mut offset = 0;
        // A node with an empty sequence still needs one fragment.
        loop {
            let end = (offset + max_len).min(sequence.len());
            let fragment = result.add_node(
                format!("{}_{}", node, offset).as_bytes(), &sequence[offset..end]
            );
            if first.is_none() {
                first = Some(fragment);
            }
            if let Some(previous) = previous {
                result.add_edge(previous, Orientation::Forward, fragment, Orientation::Forward);
            }
            previous = Some(fragment);
            offset = end;
            if offset >= sequence.len() {
                break;
            }
        }
        ends.push((first.unwrap(), previous.unwrap()));
    }

    // The left side of a node belongs to its first fragment, and the right side to its last.
    let fragment_of = |node: usize, side: NodeSide| -> usize {
        match side {
            NodeSide::Left => ends[node].0,
            NodeSide::Right => ends[node].1,
        }
    };
    for node in 0..source.nodes() {
        for side in [NodeSide::Left, NodeSide::Right] {
            for (neighbor, neighbor_side) in source.neighbors(node, side) {
                result.add_edge(
                    fragment_of(node, side), support::exit_orientation(side),
                    fragment_of(neighbor, neighbor_side), support::entry_orientation(neighbor_side)
                );
            }
        }
    }
    result.finalize();

    result
}

/// Returns a subgraph of the given graph.
///
/// Each node is kept with probability `node_prob`, and each edge between two kept nodes with
/// probability `edge_prob`. Node names and sequences are copied verbatim, unlike in [`permute`]
/// and [`chop`], so the result is a subgraph in the sense of [`crate::topology::is_subgraph`].
pub fn random_subgraph<T: Topology>(
    source: &T, node_prob: f64, edge_prob: f64, rng: &mut impl Rng
) -> IndexedGraph {
    let mut result = IndexedGraph::new();
    let mut index = vec![usize::MAX; source.nodes()];
    for (node, target) in index.iter_mut().enumerate() {
        if rng.random_bool(node_prob) {
            *target = result.add_node(&source.node_name(node), &source.sequence(node));
        }
    }

    for node in 0..source.nodes() {
        if index[node] == usize::MAX {
            continue;
        }
        for side in [NodeSide::Left, NodeSide::Right] {
            for (neighbor, neighbor_side) in source.neighbors(node, side) {
                if index[neighbor] == usize::MAX {
                    continue;
                }
                // Consider each edge once, as `IndexedGraph::from_topology` does. Otherwise the
                // two endpoints would make independent decisions about keeping it.
                let from = support::encode_node_side(node, side);
                let to = support::encode_node_side(neighbor, neighbor_side);
                if from <= to && rng.random_bool(edge_prob) {
                    result.add_edge(
                        index[node], support::exit_orientation(side),
                        index[neighbor], support::entry_orientation(neighbor_side)
                    );
                }
            }
        }
    }
    result.finalize();

    result
}

//-----------------------------------------------------------------------------

/// Returns the mapping induced by a permutation and a vector of flips.
pub fn mapping_of(permutation: &[usize], flips: &[bool]) -> NodeMapping {
    let encoded: Vec<u32> = permutation.iter().zip(flips.iter())
        .map(|(&image, &flip)| (2 * image + (flip as usize)) as u32)
        .collect();
    NodeMapping::from_encoded(encoded)
}

/// Checks that the translation is a correspondence between the two graphs.
///
/// This spells out the sequence of each walk and counts the visits to each node, which is a
/// different formulation from `verify_translation`.
pub fn is_translation<A: Topology, B: Topology>(
    first: &A, second: &B, translation: &Translation
) -> Result<(), String> {
    let mut first_visits = vec![0; first.nodes()];
    let mut second_visits = vec![0; second.nodes()];

    for index in 0..translation.len() {
        // The two walks must spell the same sequence.
        let here = spell(first, translation.first_walk(index), &mut first_visits)?;
        let there = spell(second, translation.second_walk(index), &mut second_visits)?;
        if here != there {
            return Err(format!(
                "The walks of pair {} spell different sequences: {} and {}",
                index, String::from_utf8_lossy(&here), String::from_utf8_lossy(&there)
            ));
        }
    }

    // Every node of both graphs must be visited exactly once.
    for (node, &visits) in first_visits.iter().enumerate() {
        if visits != 1 {
            return Err(format!("Node {} of the first graph is visited {} times", node, visits));
        }
    }
    for (node, &visits) in second_visits.iter().enumerate() {
        if visits != 1 {
            return Err(format!("Node {} of the second graph is visited {} times", node, visits));
        }
    }

    Ok(())
}

// Returns the sequence spelled by the walk, checking that it is a path and counting the visits.
fn spell<T: Topology>(
    graph: &T, walk: impl Iterator<Item = (usize, Orientation)>, visits: &mut [usize]
) -> Result<Vec<u8>, String> {
    let mut result: Vec<u8> = Vec::new();
    let mut previous: Option<(usize, Orientation)> = None;

    for (node, orientation) in walk {
        if node >= visits.len() {
            return Err(format!("The walk visits node {}, which does not exist", node));
        }
        visits[node] += 1;
        if let Some((from, from_orientation)) = previous {
            let exists = graph.neighbors(from, support::exit_side(from_orientation))
                .any(|(next, side)| next == node && side == support::entry_side(orientation));
            if !exists {
                return Err(format!("There is no edge from node {} to node {}", from, node));
            }
        }
        let sequence = graph.sequence(node);
        match orientation {
            Orientation::Forward => result.extend_from_slice(&sequence),
            Orientation::Reverse => result.extend(hashing::reverse_complement(&sequence)),
        }
        previous = Some((node, orientation));
    }

    Ok(result)
}

//-----------------------------------------------------------------------------

/// Checks that the mapping is an isomorphism between the two graphs.
///
/// This is written independently of `isomorphism::verify`, so that a bug in the production
/// verification cannot hide itself.
pub fn is_isomorphism<A: Topology, B: Topology>(
    first: &A, second: &B, mapping: &NodeMapping
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
