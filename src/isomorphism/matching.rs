//! Finding the node mapping.
//!
//! The search starts from *anchors*: color classes that contain exactly one node in each graph.
//! Such a node can only map to its counterpart, so the pair is forced. From a matched pair, the
//! neighbors on each side are grouped by color, and a group that contains exactly one unmatched
//! node in each graph is forced as well. Propagating these deductions matches an entire connected
//! component in linear time, as long as the colors are specific enough.
//!
//! Every forced deduction preserves extendability: if the partial mapping can be extended to an
//! isomorphism, so can the result of the deduction. A conflict reached without making any choices
//! is therefore a proof that the graphs are not isomorphic.
//!
//! Where the colors are not specific enough, the search individualizes one node: it tries each
//! candidate in turn, propagates, and backtracks on a conflict. Candidates are taken from the group
//! at a site next to an already matched pair whenever possible, because such groups are small.
//!
//! The relative orientation is never guessed during propagation. If node `x` is reached through
//! side `sx` in the first graph and node `y` through side `sy` in the second, then side `sx` of `x`
//! corresponds to side `sy` of `y`, which determines whether the node is flipped. This is why
//! mapping nodes to reverse complements costs almost nothing: an edge determines the relative
//! orientation of its endpoints, so the orientations of a whole connected component follow from its
//! first matched pair.

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
struct Group {
    here: Vec<(usize, NodeSide)>,
    there: Vec<(usize, NodeSide)>,
}

// A point the search can backtrack to.
#[derive(Copy, Clone, Debug)]
struct Mark {
    trail: usize,
    pending: usize,
    head: usize,
}

//-----------------------------------------------------------------------------

/// Builds a mapping between the nodes of two graphs.
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
    // Nodes of the first graph whose neighbors have not been matched yet.
    queue: Vec<u32>,
    // Sites `2 * node + side` in the first graph where several neighbors share a color.
    pending: Vec<u32>,
    // The first site in `pending` that may still be unresolved.
    head: usize,
    // Nodes assigned since the start, for undoing.
    trail: Vec<u32>,
    // Members of each class, in CSR format.
    first_members: (Vec<u32>, Vec<u32>),
    second_members: (Vec<u32>, Vec<u32>),
    // Classes whose members are isolated nodes with the same sequence, and are therefore
    // interchangeable.
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
        let first_members = class_members(classes.first_classes(), classes.len());
        let second_members = class_members(classes.second_classes(), classes.len());
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
    //
    // A symmetric node is skipped, because its relative orientation would be a choice rather than a
    // deduction.
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
            if self.assign(node, image, orientation).is_err() {
                return Err(());
            }
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
            let (node, side) = decode_site(self.pending[self.head]);
            let Some((image, orientation)) = self.image_of(node) else {
                self.head += 1;
                continue;
            };
            match self.site_group(node, side, image, map_side(side, orientation)) {
                Err(()) => return Choice::Conflict,
                Ok(None) => { self.head += 1; },
                Ok(Some(group)) => {
                    // Matching the site may have forced other pairs. Propagate those first: `undo`
                    // clears the queue, so a node left in it would never have its edges checked.
                    if !self.queue.is_empty() {
                        return Choice::Progress;
                    }
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
                    Some(_) => self.pending.push(encode_site(node as usize, side)),
                }
            }
        }

        Ok(())
    }

    // Matches the neighbors of one side of a matched pair, assigning every forced pair.
    //
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

    // Assigns a node of the first graph to a node of the second graph.
    fn assign(&mut self, node: usize, image: usize, orientation: Orientation) -> Result<(), ()> {
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
        // The colors are hashes, so the sequences themselves have to be compared. This also makes
        // the interchangeability of isolated nodes an exact property rather than a hash claim.
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
        &members[offsets[class as usize] as usize..offsets[class as usize + 1] as usize]
    }
}

//-----------------------------------------------------------------------------

// Encodes a site as `2 * node + side`.
fn encode_site(node: usize, side: NodeSide) -> u32 {
    (2 * node + (side as usize)) as u32
}

// Decodes a site.
fn decode_site(encoded: u32) -> (usize, NodeSide) {
    let encoded = encoded as usize;
    let side = if encoded & 1 == 0 { NodeSide::Left } else { NodeSide::Right };
    (encoded / 2, side)
}

// Returns the members of each class in CSR format.
fn class_members(class_of: &[u32], class_count: usize) -> (Vec<u32>, Vec<u32>) {
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
//
// The members of such a class can be matched in any order, because any permutation of them is an
// automorphism. Without this, a graph with many identical isolated nodes would turn every one of
// them into a choice point.
fn interchangeable_classes<A: Topology, B: Topology>(
    first: &A, second: &B, classes: &Classes,
    first_members: &(Vec<u32>, Vec<u32>), second_members: &(Vec<u32>, Vec<u32>)
) -> Vec<bool> {
    let mut result = vec![false; classes.len()];

    for (class, flag) in result.iter_mut().enumerate() {
        let here = &first_members.1[
            first_members.0[class] as usize..first_members.0[class + 1] as usize
        ];
        let there = &second_members.1[
            second_members.0[class] as usize..second_members.0[class + 1] as usize
        ];
        if here.len() < 2 {
            continue;
        }

        let node = here[0] as usize;
        let isolated = first.degree(node, NodeSide::Left) == 0
            && first.degree(node, NodeSide::Right) == 0;
        if !isolated {
            continue;
        }

        // Every member must have the same sequence, up to reverse complement. The colors alone
        // would not be enough, as they are hashes.
        let sequence = first.sequence(node).to_vec();
        let same = |other: &[u8]| -> bool {
            other == sequence || hashing::equals_reverse_complement(&sequence, other)
        };
        *flag = here.iter().all(|&n| same(&first.sequence(n as usize)))
            && there.iter().all(|&n| same(&second.sequence(n as usize)));
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
