//! What a search spends on its static evaluations: how many accumulator updates it makes, how many of
//! them any evaluation ever reads, and how many evaluations a TT entry for the same position could
//! have spared. Measures what lazy accumulator updates and an evaluation kept in the TT would save:
//! on amanda's generation-1 network (2026-10-07), 8.9% of updates were never read and 10.7% of
//! evaluations were of positions the TT held, a few percent of her search at most.
//!
//! Per thread, so a search never shares its counters; `take` reads the calling thread's and resets
//! them. Only built under the `node-stats` feature: the engine never carries it.
//!
//! An update writes the accumulator slot of one ply. It is **needed** when an evaluation reads that
//! slot, or reads a later slot on the same path, which a lazy update would have built from it.
//! `evaluated` marks the evaluated ply's slot and those before it, stopping at the first slot
//! already marked: every slot below a marked one was marked with it, and a slot written since was
//! unmarked by its write.

use std::cell::RefCell;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NodeStats {
    /// Accumulator updates made, one per child entered.
    pub updates: u64,
    /// Updates some evaluation read, directly or through a later ply.
    pub needed_updates: u64,
    /// Static evaluations, by any evaluator.
    pub evaluations: u64,
    /// Static evaluations of a position the TT held an entry for when it was evaluated.
    pub evaluations_with_tt_entry: u64,
    /// Quiescence nodes, of the search's node count: what separates a tree's cheap leaves from its
    /// interior nodes, when two evaluations grow differently made trees.
    pub quiescence_nodes: u64,
    /// Main-search nodes whose move picker went past the TT move to generate the other moves: where
    /// a move head, tried right after the TT move, would have to score them. Pruned nodes and
    /// TT-move cutoffs never get there, and quiescence has no picker.
    pub ordered_nodes: u64,
    /// Beta cutoffs in the main search by the TT move.
    pub tt_cutoffs: u64,
    /// Beta cutoffs by the first legal move after the TT move: the move a move head would replace.
    pub first_cutoffs: u64,
    /// Beta cutoffs by a later move, and the sum of their places after the TT move (2 for the
    /// second legal move after it, and so on), for their mean.
    pub later_cutoffs: u64,
    pub later_places: u64,
    /// Of `first_cutoffs` and `later_cutoffs`, those by a quiet move: where a move hint tried after
    /// the good captures, as killers are, could still help.
    pub first_quiet_cutoffs: u64,
    pub later_quiet_cutoffs: u64,
    /// Nodes searched in the moves tried in vain before a later cutoff, after the TT move: what a
    /// perfect move hint would have saved at those nodes. Each node counted at most once.
    pub wasted_nodes: u64,
    /// Of `wasted_nodes`, those before a later cutoff by a quiet move.
    pub wasted_before_quiet: u64,
    /// `ordered_nodes` and `wasted_nodes` by the remaining depth of the node they were counted at,
    /// the last entry holding every deeper one: where a move hint costs, and where it could save.
    pub ordered_by_depth: [u64; DEPTHS],
    pub wasted_by_depth: [u64; DEPTHS],
}

/// Remaining depths `0..DEPTHS` in the per-depth counts; deeper nodes land in the last.
pub const DEPTHS: usize = 12;

fn depth_index(remaining: u32) -> usize {
    return (remaining as usize).min(DEPTHS - 1);
}

struct State {
    stats: NodeStats,
    /// Per accumulator slot (one per ply), whether the update that last wrote it is needed.
    needed: Vec<bool>,
}

thread_local! {
    static STATE: RefCell<State> = const { RefCell::new(State { stats: NodeStats { updates: 0, needed_updates: 0, evaluations: 0, evaluations_with_tt_entry: 0, quiescence_nodes: 0, ordered_nodes: 0, tt_cutoffs: 0, first_cutoffs: 0, later_cutoffs: 0, later_places: 0, first_quiet_cutoffs: 0, later_quiet_cutoffs: 0, wasted_nodes: 0, wasted_before_quiet: 0, ordered_by_depth: [0; DEPTHS], wasted_by_depth: [0; DEPTHS] }, needed: Vec::new() }) };
}

/// The wasted nodes recorded so far.
pub fn wasted_nodes() -> u64 {
    return STATE.with(|state| state.borrow().stats.wasted_nodes);
}

/// `nodes` more nodes searched in vain before a later cutoff at a node with `remaining` plies left,
/// the cutoff by a quiet move when `quiet`.
pub fn waste(remaining: u32, nodes: u64, quiet: bool) {
    STATE.with(|state| {
        let stats = &mut state.borrow_mut().stats;
        stats.wasted_nodes += nodes;
        stats.wasted_by_depth[depth_index(remaining)] += nodes;
        if quiet {
            stats.wasted_before_quiet += nodes;
        }
    });
}

/// A beta cutoff in the main search: by the TT move, or by the `place`th legal move after it, a
/// quiet one when `quiet`.
pub fn cutoff(by_tt_move: bool, place: u64, quiet: bool) {
    STATE.with(|state| {
        let stats = &mut state.borrow_mut().stats;
        if by_tt_move {
            stats.tt_cutoffs += 1;
        } else if place <= 1 {
            stats.first_cutoffs += 1;
            if quiet {
                stats.first_quiet_cutoffs += 1;
            }
        } else {
            stats.later_cutoffs += 1;
            stats.later_places += place;
            if quiet {
                stats.later_quiet_cutoffs += 1;
            }
        }
    });
}

/// A quiescence node was entered and counted as a node.
pub fn quiescence_node() {
    STATE.with(|state| state.borrow_mut().stats.quiescence_nodes += 1);
}

/// A main-search node with `remaining` plies left whose move picker went past the TT move.
pub fn ordered_node(remaining: u32) {
    STATE.with(|state| {
        let stats = &mut state.borrow_mut().stats;
        stats.ordered_nodes += 1;
        stats.ordered_by_depth[depth_index(remaining)] += 1;
    });
}

/// An update wrote the accumulator slot `slot`.
pub fn updated(slot: usize) {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.stats.updates += 1;
        if state.needed.len() <= slot {
            state.needed.resize(slot + 1, false);
        }
        state.needed[slot] = false;
    });
}

/// A static evaluation at `ply`. `from_accumulators` when it read the accumulator slot of `ply`;
/// slot 0, the root's, is built from scratch and is not an update.
pub fn evaluated(ply: usize, with_tt_entry: bool, from_accumulators: bool) {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.stats.evaluations += 1;
        if with_tt_entry {
            state.stats.evaluations_with_tt_entry += 1;
        }
        if !from_accumulators {
            return;
        }
        if state.needed.len() <= ply {
            state.needed.resize(ply + 1, false);
        }
        for slot in (1..=ply).rev() {
            if state.needed[slot] {
                break;
            }
            state.needed[slot] = true;
            state.stats.needed_updates += 1;
        }
    });
}

/// The calling thread's counts since the last `take`, which it resets.
pub fn take() -> NodeStats {
    return STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.needed.clear();
        return std::mem::take(&mut state.stats);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_update_is_needed_when_its_ply_or_a_later_one_on_its_path_is_evaluated() {
        let _ = take();
        updated(1); // the first child
        updated(2); // its child, evaluated: both needed
        evaluated(2, false, true);
        updated(2); // a sibling, cut off before any evaluation: not needed
        updated(2); // another sibling, evaluated: needed, and ply 1 is not counted again
        evaluated(2, true, true);
        updated(1); // a new first-ply child, its subtree never evaluated: not needed
        updated(2);
        evaluated(0, false, true); // the root reads no update
        evaluated(3, false, false); // PeSTO, or past the accumulators: reads none
        quiescence_node();
        ordered_node(2);
        cutoff(true, 0, true); // the TT move: never split by kind
        cutoff(false, 1, true);
        cutoff(false, 3, false);
        cutoff(false, 5, true);
        waste(7, 40, true);
        waste(30, 2, false); // deeper than the table: the last entry
        assert_eq!(wasted_nodes(), 42);
        let mut ordered_by_depth = [0; DEPTHS];
        ordered_by_depth[2] = 1;
        let mut wasted_by_depth = [0; DEPTHS];
        wasted_by_depth[7] = 40;
        wasted_by_depth[DEPTHS - 1] = 2;
        assert_eq!(
            take(),
            NodeStats {
                updates: 6,
                needed_updates: 3,
                evaluations: 4,
                evaluations_with_tt_entry: 1,
                quiescence_nodes: 1,
                ordered_nodes: 1,
                tt_cutoffs: 1,
                first_cutoffs: 1,
                later_cutoffs: 2,
                later_places: 8,
                first_quiet_cutoffs: 1,
                later_quiet_cutoffs: 1,
                wasted_nodes: 42,
                wasted_before_quiet: 40,
                ordered_by_depth,
                wasted_by_depth
            }
        );
    }
}
