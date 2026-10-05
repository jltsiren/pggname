//! Matching nodes based on color classes.
//!
//! The search starts from anchors: color classes that contain exactly one node in each graph.
//! These nodes must be necessarily matched (forced).
//! From a matched pair, the neighbors on each side are grouped by color.
//! A group that contains exactly one unmatched node in each graph is forced as well.
//! Once all forced deductions have been made, we make a choice and propagate its consequences.
//! If this leads to a conflict, we backtrack and try a different choice.

use crate::topology::Topology;

use super::{NodeMapping, Options, hashing, map_side};
use super::coloring::{Classes, Coloring};

use gbz::{NodeSide, Orientation};

//-----------------------------------------------------------------------------

// Sentinel for an unassigned node.
const NONE: u32 = u32::MAX;

/// The outcome of the matching.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// Every node was assigned.
    Complete,
    /// No mapping exists, and the search covered every possibility.
    Conflict,
    /// The search budget ran out before the question could be settled.
    Exhausted,
}

// A group of candidates at a site, or a fresh component to seed.
enum Choice {
    // Assign `node` to one of the candidates. The orientation follows from the sides.
    Site { node: usize, side: NodeSide, candidates: Vec<(usize, NodeSide)> },
    // Assign `node` to one of the candidates, trying both orientations if it is symmetric.
    Seed { node: usize, candidates: Vec<usize> },
    // A forced assignment was made; the caller should try again.
    Progress,
    // Nothing left to decide.
    Done,
    // The partial mapping cannot be extended.
    Conflict,
}

// A group of unmatched candidates at a site.
// We are currently considering any matching between the two sets.
struct Group {
    here: Vec<(usize, NodeSide)>,
    there: Vec<(usize, NodeSide)>,
}

// A point the search can backtrack to.
#[derive(Copy, Clone, Debug)]
struct Mark {
    // The position in the trail to backtrack to.
    trail: usize,
    // The number of pending sites at the time of the mark.
    pending: usize,
    // The first site in `pending` that may still be unresolved.
    head: usize,
}

//-----------------------------------------------------------------------------

/// Builder for a mapping between the nodes of two graphs.
pub(crate) struct Matcher<'a, A: Topology, B: Topology> {
    first: &'a A,
    second: &'a B,
    first_coloring: &'a Coloring,
    second_coloring: &'a Coloring,
    classes: &'a Classes,

    // Image of each node of the first graph, encoded as `2 * node + flip`, or `NONE`.
    image: Vec<u32>,
    // Which nodes of the second graph have been taken.
    taken: Vec<bool>,
    // Stack of assigned nodes of the first graph whose neighbors have not been examined yet.
    queue: Vec<u32>,
    // Sites `2 * node + side` for nodes coming from `queue` where several neighbors share a color.
    pending: Vec<u32>,
    // The first site in `pending` that may still be unresolved.
    head: usize,
    // Nodes of the first graph assigned since the start, for undoing.
    trail: Vec<u32>,
    // Members of each class, as (offsets, members).
    // Class `i` contains nodes `members[offsets[i]..offsets[i + 1]]`.
    first_members: (Vec<u32>, Vec<u32>),
    second_members: (Vec<u32>, Vec<u32>),
    // Classes whose members are isolated nodes with the same sequence, and are therefore interchangeable.
    interchangeable: Vec<bool>,

    assigned: usize,
    choices: usize,
    budget: usize,
    // The first unassigned node, as a starting point for the scan.
    scan: usize,
}

impl<'a, A: Topology, B: Topology> Matcher<'a, A, B> {
    /// Creates a matcher for the two graphs.
    pub(crate) fn new(
        first: &'a A, second: &'a B,
        first_coloring: &'a Coloring, second_coloring: &'a Coloring,
        classes: &'a Classes, options: &'a Options
    ) -> Self {
        let first_members = class_offsets_and_members(classes.first_classes(), classes.len());
        let second_members = class_offsets_and_members(classes.second_classes(), classes.len());
        let interchangeable = interchangeable_classes(
            first, second, classes, &first_members, &second_members
        );
        Matcher {
            first, second, first_coloring, second_coloring, classes,
            image: vec![NONE; first.nodes()],
            taken: vec![false; second.nodes()],
            queue: Vec::new(),
            pending: Vec::new(),
            head: 0,
            trail: Vec::new(),
            first_members, second_members, interchangeable,
            assigned: 0,
            choices: 0,
            budget: options.search_budget,
            scan: 0,
        }
    }

    /// Returns the number of choices made.
    pub(crate) fn choices(&self) -> usize {
        self.choices
    }

    /// Matches the nodes of the two graphs.
    pub(crate) fn run(&mut self) -> Outcome {
        if self.seed_anchors().is_err() {
            return Outcome::Conflict;
        }
        self.search()
    }

    /// Returns the mapping, which must be complete.
    pub(crate) fn into_mapping(self) -> NodeMapping {
        NodeMapping::from_encoded(self.image)
    }

    // Assigns every class that contains exactly one node in each graph.
    // Symmetric nodes are skipped, as their orientation requires a choice.
    fn seed_anchors(&mut self) -> Result<(), ()> {
        for class in 0..self.classes.len() {
            if self.classes.size(class as u32) != 1 {
                continue;
            }
            let node = self.members(true, class as u32)[0] as usize;
            let image = self.members(false, class as u32)[0] as usize;
            if self.first_coloring.is_symmetric(node) {
                continue;
            }
            let flip = self.first_coloring.head_side(node) != self.second_coloring.head_side(image);
            let orientation = if flip { Orientation::Reverse } else { Orientation::Forward };
            self.assign(node, image, orientation)?;
        }

        Ok(())
    }

    // Propagates forced deductions and individualizes a node when none remain.
    fn search(&mut self) -> Outcome {
        loop {
            if self.propagate().is_err() {
                return Outcome::Conflict;
            }
            match self.next_choice() {
                Choice::Conflict => return Outcome::Conflict,
                Choice::Progress => continue,
                Choice::Done => {
                    return if self.assigned == self.first.nodes() {
                        Outcome::Complete
                    } else {
                        Outcome::Conflict
                    };
                },
                // Branch on multiple oriented candidate images for a node.
                Choice::Site { node, side, candidates } => {
                    let options: Vec<(usize, Orientation)> = candidates.into_iter()
                        .map(|(target, target_side)| {
                            let orientation = if side == target_side {
                                Orientation::Forward
                            } else {
                                Orientation::Reverse
                            };
                            (target, orientation)
                        })
                        .collect();
                    return self.branch(node, options);
                },
                // Branch on multiple candidate images for a node, in both orientations if the node is symmetric.
                Choice::Seed { node, candidates } => {
                    let options = self.seed_options(node, &candidates);
                    return self.branch(node, options);
                },
            }
        }
    }

    // Tries each option in turn, backtracking on a conflict.
    fn branch(&mut self, node: usize, options: Vec<(usize, Orientation)>) -> Outcome {
        for (target, orientation) in options {
            if self.budget == 0 {
                return Outcome::Exhausted;
            }
            self.budget -= 1;
            self.choices += 1;

            let mark = self.mark();
            if self.assign(node, target, orientation).is_ok() {
                // Recursive call. We assume that our budget is not high enough to worry about stack overflows.
                match self.search() {
                    Outcome::Complete => return Outcome::Complete,
                    // Once the budget is gone, the remaining options cannot be ruled out.
                    Outcome::Exhausted => { self.undo(mark); return Outcome::Exhausted; },
                    Outcome::Conflict => {},
                }
            }
            self.undo(mark);
        }

        Outcome::Conflict
    }

    // Returns the orientations worth trying when seeding a node.
    fn seed_options(&self, node: usize, candidates: &[usize]) -> Vec<(usize, Orientation)> {
        let mut result = Vec::new();
        for &target in candidates.iter() {
            if self.first_coloring.is_symmetric(node) {
                // The refinement cannot tell the sides of the node apart, so both orientations are
                // worth trying. This is the only place where a flip is ever guessed.
                result.push((target, Orientation::Forward));
                result.push((target, Orientation::Reverse));
            } else {
                let flip = self.first_coloring.head_side(node) != self.second_coloring.head_side(target);
                let orientation = if flip { Orientation::Reverse } else { Orientation::Forward };
                result.push((target, orientation));
            }
        }
        result
    }

    // Finds the next decision to make, making forced assignments along the way.
    fn next_choice(&mut self) -> Choice {
        // Prefer a site next to an already matched pair: such groups are small.
        while self.head < self.pending.len() {
            let (node, side) = crate::decode_node_side(self.pending[self.head]);
            let Some((image, orientation)) = self.image_of(node) else {
                self.head += 1;
                continue;
            };
            match self.site_group(node, side, image, map_side(side, orientation)) {
                Err(()) => return Choice::Conflict,
                Ok(None) => { self.head += 1; },
                Ok(Some(group)) => {
                    // Matching the site may have forced other pairs. Propagate those first.
                    if !self.queue.is_empty() {
                        return Choice::Progress;
                    }
                    // Try matching the first node in the first graph to any candidate in the second graph.
                    return Choice::Site { node: group.here[0].0, side: group.here[0].1, candidates: group.there };
                },
            }
        }

        // Otherwise seed a new component.
        while self.scan < self.first.nodes() && self.image[self.scan] != NONE {
            self.scan += 1;
        }
        if self.scan >= self.first.nodes() {
            return Choice::Done;
        }

        let node = self.scan;
        let class = self.classes.in_first(node);
        let candidates: Vec<usize> = self.members(false, class).iter()
            .map(|&target| target as usize)
            .filter(|&target| !self.taken[target])
            .collect();
        if candidates.is_empty() {
            return Choice::Conflict;
        }

        // Isolated nodes with the same sequence are interchangeable, so any free candidate will do.
        if self.interchangeable[class as usize] {
            let orientation = self.forced_orientation(node, candidates[0]);
            return match orientation {
                Some(orientation) => {
                    if self.assign(node, candidates[0], orientation).is_err() {
                        Choice::Conflict
                    } else {
                        Choice::Progress
                    }
                },
                None => Choice::Conflict,
            };
        }

        if !self.queue.is_empty() {
            return Choice::Progress;
        }
        Choice::Seed { node, candidates }
    }

    // Returns the orientation in which the sequences match, if there is exactly one.
    fn forced_orientation(&self, node: usize, target: usize) -> Option<Orientation> {
        let sequence = self.first.sequence(node);
        let image = self.second.sequence(target);
        if sequence == image {
            Some(Orientation::Forward)
        } else if hashing::equals_reverse_complement(&sequence, &image) {
            Some(Orientation::Reverse)
        } else {
            None
        }
    }

    // Propagates forced deductions from the queue.
    fn propagate(&mut self) -> Result<(), ()> {
        while let Some(node) = self.queue.pop() {
            let (image, orientation) = self.image_of(node as usize).unwrap();
            for side in [NodeSide::Left, NodeSide::Right] {
                let image_side = map_side(side, orientation);
                match self.site_group(node as usize, side, image, image_side)? {
                    None => {},
                    // Re-derive the group later; it may have shrunk by then.
                    Some(_) => self.pending.push(crate::encode_node_side(node as usize, side)),
                }
            }
        }

        Ok(())
    }

    // Matches the neighbors of one side of a matched pair, assigning every forced pair.
    // Returns the first group that remains ambiguous, or an error if the mapping is broken.
    fn site_group(
        &mut self, node: usize, side: NodeSide, image: usize, image_side: NodeSide
    ) -> Result<Option<Group>, ()> {
        let mut here = self.neighbors(true, node, side);
        let mut there = self.neighbors(false, image, image_side);
        if here.len() != there.len() {
            return Err(());
        }
        here.sort_unstable();
        there.sort_unstable();

        let mut start = 0;
        while start < here.len() {
            let color = here[start].0;
            if there[start].0 != color {
                return Err(());
            }
            let end = run_end(&here, start);
            if run_end(&there, start) != end {
                return Err(());
            }

            // Remove the neighbors that are already matched. A group with several members may
            // still be forced once the matched ones are taken out.
            let mut free_here: Vec<(usize, NodeSide)> = Vec::new();
            let mut consumed = vec![false; end - start];
            for &(_, neighbor, neighbor_side) in here[start..end].iter() {
                match self.image_of(neighbor) {
                    Some((target, _)) => {
                        match there[start..end].iter().position(|&(_, n, _)| n == target) {
                            Some(offset) if !consumed[offset] => consumed[offset] = true,
                            // The image is not among the candidates, so the mapping is broken.
                            _ => return Err(()),
                        }
                    },
                    None => free_here.push((neighbor, neighbor_side)),
                }
            }
            let free_there: Vec<(usize, NodeSide)> = there[start..end].iter().enumerate()
                .filter(|(offset, _)| !consumed[*offset])
                .map(|(_, &(_, neighbor, neighbor_side))| (neighbor, neighbor_side))
                .collect();
            if free_here.len() != free_there.len() {
                return Err(());
            }

            if free_here.len() == 1 {
                let (neighbor, neighbor_side) = free_here[0];
                let (target, target_side) = free_there[0];
                // The relative orientation follows from the sides the edge arrives at.
                let orientation = if neighbor_side == target_side {
                    Orientation::Forward
                } else {
                    Orientation::Reverse
                };
                self.assign(neighbor, target, orientation)?;
            } else if free_here.len() > 1 {
                return Ok(Some(Group { here: free_here, there: free_there }));
            }

            start = end;
        }

        Ok(None)
    }

    // Assigns a node of the first graph to a node of the second graph in the given orientation.
    // Checks that the assignment does not conflict with earlier assignments.
    // Checks that the sequences and degrees match.
    fn assign(&mut self, node: usize, image: usize, orientation: Orientation) -> Result<(), ()> {
        // FIXME: support::encode_node
        let encoded = (2 * image + (orientation as usize)) as u32;

        if self.image[node] != NONE {
            // A second deduction must agree, including on the relative orientation.
            return if self.image[node] == encoded { Ok(()) } else { Err(()) };
        }
        if self.taken[image] {
            return Err(());
        }
        if self.classes.in_first(node) != self.classes.in_second(image) {
            return Err(());
        }
        for side in [NodeSide::Left, NodeSide::Right] {
            if self.first.degree(node, side) != self.second.degree(image, map_side(side, orientation)) {
                return Err(());
            }
        }
        // The sequences must match in the given orientation.
        let matches = {
            let sequence = self.first.sequence(node);
            let target = self.second.sequence(image);
            match orientation {
                Orientation::Forward => sequence == target,
                Orientation::Reverse => hashing::equals_reverse_complement(&sequence, &target),
            }
        };
        if !matches {
            return Err(());
        }

        self.image[node] = encoded;
        self.taken[image] = true;
        self.assigned += 1;
        self.queue.push(node as u32);
        self.trail.push(node as u32);

        Ok(())
    }

    // Returns a point the search can backtrack to.
    fn mark(&self) -> Mark {
        Mark { trail: self.trail.len(), pending: self.pending.len(), head: self.head }
    }

    // Undoes every assignment made since the mark.
    fn undo(&mut self, mark: Mark) {
        while self.trail.len() > mark.trail {
            let node = self.trail.pop().unwrap() as usize;
            let (image, _) = self.image_of(node).unwrap();
            self.taken[image] = false;
            self.image[node] = NONE;
            self.assigned -= 1;
            self.scan = self.scan.min(node);
        }
        self.pending.truncate(mark.pending);
        self.head = mark.head;
        self.queue.clear();
    }

    // Returns the image of the node and the relative orientation, if it has been assigned.
    fn image_of(&self, node: usize) -> Option<(usize, Orientation)> {
        let encoded = self.image[node];
        if encoded == NONE {
            return None;
        }
        // FIXME: support::decode_node
        let encoded = encoded as usize;
        let orientation = if encoded & 1 == 0 { Orientation::Forward } else { Orientation::Reverse };
        Some((encoded / 2, orientation))
    }

    // Returns the neighbors of the given side as (side color, node, side).
    fn neighbors(&self, in_first: bool, node: usize, side: NodeSide) -> Vec<(u64, usize, NodeSide)> {
        if in_first {
            self.first.neighbors(node, side)
                .map(|(n, s)| (self.first_coloring.side_color(n, s), n, s))
                .collect()
        } else {
            self.second.neighbors(node, side)
                .map(|(n, s)| (self.second_coloring.side_color(n, s), n, s))
                .collect()
        }
    }

    // Returns the members of the given class.
    fn members(&self, in_first: bool, class: u32) -> &[u32] {
        let (offsets, members) = if in_first { &self.first_members } else { &self.second_members };
        members_of_class(offsets, members, class as usize)
    }
}

// Returns the members of the given class as a slice.
fn members_of_class<'a>(offsets: &[u32], members: &'a [u32], class: usize) -> &'a [u32] {
    &members[offsets[class] as usize..offsets[class + 1] as usize]
}

//-----------------------------------------------------------------------------

// Returns the members of each class as (offsets, members).
// Class `i` contains nodes `members[offsets[i]..offsets[i + 1]]`.
fn class_offsets_and_members(class_of: &[u32], class_count: usize) -> (Vec<u32>, Vec<u32>) {
    let mut offsets = vec![0u32; class_count + 1];
    for &class in class_of.iter() {
        offsets[class as usize + 1] += 1;
    }
    for i in 0..class_count {
        offsets[i + 1] += offsets[i];
    }

    let mut next = offsets.clone();
    let mut members = vec![0u32; class_of.len()];
    for (node, &class) in class_of.iter().enumerate() {
        members[next[class as usize] as usize] = node as u32;
        next[class as usize] += 1;
    }

    (offsets, members)
}

// Determines which classes consist of isolated nodes that all have the same sequence.
// These nodes can be matched arbitrarily.
fn interchangeable_classes<A: Topology, B: Topology>(
    first: &A, second: &B, classes: &Classes,
    first_members: &(Vec<u32>, Vec<u32>), second_members: &(Vec<u32>, Vec<u32>)
) -> Vec<bool> {
    let mut result = vec![false; classes.len()];

    for (class, flag) in result.iter_mut().enumerate() {
        let here = members_of_class(&first_members.0, &first_members.1, class as usize);
        let there = members_of_class(&second_members.0, &second_members.1, class as usize);
        if here.len() < 2 {
            continue;
        }

        // We check all nodes in the class to avoid hash collisions.
        // This is a bit redundant, but isolated nodes tend to be unique unaligned contigs.
        let expected_sequence = first.sequence(here[0] as usize);
        let sequence_matches = |seq: &[u8]| -> bool {
            seq == expected_sequence.as_ref() || hashing::equals_reverse_complement(&expected_sequence, seq)
        };
        *flag = true;
        for (&first_node, &second_node) in here.iter().zip(there.iter()) {
            if first.degree(first_node as usize, NodeSide::Left) != 0 || first.degree(first_node as usize, NodeSide::Right) != 0 {
                *flag = false;
                break;
            }
            if second.degree(second_node as usize, NodeSide::Left) != 0 || second.degree(second_node as usize, NodeSide::Right) != 0 {
                *flag = false;
                break;
            }
            if !sequence_matches(&first.sequence(first_node as usize)) {
                *flag = false;
                break;
            }
            if !sequence_matches(&second.sequence(second_node as usize)) {
                *flag = false;
                break;
            }
        }
    }

    result
}

// Returns the end of the run of equal colors starting at the given offset.
fn run_end(neighbors: &[(u64, usize, NodeSide)], start: usize) -> usize {
    let color = neighbors[start].0;
    let mut end = start + 1;
    while end < neighbors.len() && neighbors[end].0 == color {
        end += 1;
    }
    end
}

//-----------------------------------------------------------------------------
