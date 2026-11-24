//! One test per English example of `verify/spec-en.md`, named after the rule.
//! A counter-example is checked by asserting the exact answers of the example: they
//! differ from the counter-example at the first answer that matters.

use super::{HandlerId, LoopCore, SourceId, Turn};

const SERVE_0: Turn = Turn::Serve(SourceId(0));
const SERVE_1: Turn = Turn::Serve(SourceId(1));
const SERVE_2: Turn = Turn::Serve(SourceId(2));
const RUN_0: Turn = Turn::Deadline(HandlerId(0));
const RUN_1: Turn = Turn::Deadline(HandlerId(1));
const DONE: Turn = Turn::PassDone;

fn src(index: usize) -> SourceId {
    SourceId(index)
}

fn hdl(index: usize) -> HandlerId {
    HandlerId(index)
}

/// Asks for `count` answers; the application does nothing in between.
fn answers(core: &mut LoopCore<u64>, count: usize) -> Vec<Turn> {
    let mut result = Vec::new();
    for _ in 0..count {
        result.push(core.next_turn());
    }
    result
}

#[test]
fn s1_new_loop_with_nothing_ready_is_pass_done() {
    let mut core = LoopCore::<u64>::new(&[1, 1], 1);
    core.begin_pass(0);
    assert_eq!(core.next_turn(), DONE);
}

#[test]
fn s1_before_first_begin_pass_answers_pass_done_and_changes_nothing() {
    let mut core = LoopCore::<u64>::new(&[1], 1);
    core.set_ready(src(0), true);
    core.set_deadline(hdl(0), Some(0));
    assert_eq!(answers(&mut core, 2), [DONE, DONE]);
    assert!(core.has_ready_source());
    assert_eq!(core.next_deadline(), Some(0));
    core.begin_pass(0);
    assert_eq!(answers(&mut core, 3), [SERVE_0, RUN_0, DONE]);
}

#[test]
fn s2_budget_two_made_ready_each_time() {
    let mut core = LoopCore::<u64>::new(&[2], 0);
    core.begin_pass(0);
    core.set_ready(src(0), true);
    let mut result = Vec::new();
    for _ in 0..3 {
        result.push(core.next_turn());
        core.set_ready(src(0), true);
    }
    assert_eq!(result, [SERVE_0, SERVE_0, DONE]);
}

#[test]
fn s2_budget_zero_ready_is_pass_done() {
    let mut core = LoopCore::<u64>::new(&[0], 0);
    core.set_ready(src(0), true);
    core.begin_pass(0);
    assert_eq!(core.next_turn(), DONE);
}

#[test]
fn s3_order_and_shape() {
    let mut core = LoopCore::<u64>::new(&[1, 1], 1);
    core.set_ready(src(0), true);
    core.set_ready(src(1), true);
    core.set_deadline(hdl(0), Some(0));
    core.begin_pass(0);
    assert_eq!(answers(&mut core, 5), [SERVE_0, SERVE_1, RUN_0, DONE, DONE]);
}

#[test]
fn s4_pause_between_answers_counts_at_once() {
    let mut core = LoopCore::<u64>::new(&[1, 1], 0);
    core.set_ready(src(0), true);
    core.set_ready(src(1), true);
    core.begin_pass(0);
    assert_eq!(core.next_turn(), SERVE_0);
    core.set_paused(src(1), true);
    assert_eq!(core.next_turn(), DONE);
}

#[test]
fn s4_pause_before_first_answer_is_not_served() {
    let mut core = LoopCore::<u64>::new(&[1, 1], 0);
    core.set_ready(src(1), true);
    core.begin_pass(0);
    core.set_paused(src(1), true);
    assert_eq!(core.next_turn(), DONE);
}

#[test]
fn s5_paused_source_is_never_served_and_keeps_ready() {
    let mut core = LoopCore::<u64>::new(&[1], 0);
    core.set_ready(src(0), true);
    core.set_paused(src(0), true);
    core.begin_pass(0);
    assert_eq!(core.next_turn(), DONE);
    core.set_paused(src(0), false);
    core.begin_pass(0);
    assert_eq!(answers(&mut core, 2), [SERVE_0, DONE]);
}

#[test]
fn s5_paused_handler_never_runs_and_keeps_deadline() {
    let mut core = LoopCore::<u64>::new(&[], 1);
    core.set_deadline(hdl(0), Some(3));
    core.set_deadline_paused(hdl(0), true);
    core.begin_pass(5);
    assert_eq!(core.next_turn(), DONE);
    core.set_deadline_paused(hdl(0), false);
    core.begin_pass(5);
    assert_eq!(core.next_turn(), RUN_0);
}

#[test]
fn s6_sources_are_not_skipped() {
    let mut core = LoopCore::<u64>::new(&[1, 1, 1], 0);
    for index in 0..3 {
        core.set_ready(src(index), true);
    }
    core.begin_pass(0);
    assert_eq!(answers(&mut core, 4), [SERVE_0, SERVE_1, SERVE_2, DONE]);
}

#[test]
fn s6_source_behind_position_waits_for_next_pass() {
    let mut core = LoopCore::<u64>::new(&[1, 1], 0);
    core.set_ready(src(0), true);
    core.set_ready(src(1), true);
    core.begin_pass(0);
    assert_eq!(core.next_turn(), SERVE_0);
    assert_eq!(core.next_turn(), SERVE_1);
    core.set_ready(src(0), true);
    assert_eq!(core.next_turn(), DONE);
    core.begin_pass(0);
    assert_eq!(answers(&mut core, 2), [SERVE_0, DONE]);
}

#[test]
fn s7_serving_clears_ready_and_nothing_else() {
    let mut core = LoopCore::<u64>::new(&[3, 3], 0);
    core.set_ready(src(0), true);
    core.set_ready(src(1), true);
    core.begin_pass(0);
    assert_eq!(core.next_turn(), SERVE_0);
    // Source 1 is still ready (S14 as well): has_ready_source is true.
    assert!(core.has_ready_source());
    assert_eq!(answers(&mut core, 2), [SERVE_1, DONE]);
}

#[test]
fn s7_not_made_ready_again_is_served_once() {
    let mut core = LoopCore::<u64>::new(&[3], 0);
    core.set_ready(src(0), true);
    core.begin_pass(0);
    assert_eq!(answers(&mut core, 2), [SERVE_0, DONE]);
}

#[test]
fn s7_made_ready_after_each_answer_is_served_three_times() {
    let mut core = LoopCore::<u64>::new(&[3], 0);
    core.set_ready(src(0), true);
    core.begin_pass(0);
    let mut result = Vec::new();
    for _ in 0..4 {
        result.push(core.next_turn());
        core.set_ready(src(0), true);
    }
    assert_eq!(result, [SERVE_0, SERVE_0, SERVE_0, DONE]);
}

#[test]
fn s8_two_due_handlers_run_in_order() {
    let mut core = LoopCore::<u64>::new(&[], 2);
    core.set_deadline(hdl(0), Some(0));
    core.set_deadline(hdl(1), Some(0));
    core.begin_pass(0);
    assert_eq!(answers(&mut core, 3), [RUN_0, RUN_1, DONE]);
}

#[test]
fn s8_only_handler_1_due_does_not_run_handler_0_twice() {
    let mut core = LoopCore::<u64>::new(&[], 2);
    core.set_deadline(hdl(0), Some(0));
    core.begin_pass(0);
    assert_eq!(core.next_turn(), RUN_0);
    core.set_deadline(hdl(0), Some(0));
    core.set_deadline(hdl(1), Some(0));
    assert_eq!(answers(&mut core, 2), [RUN_1, DONE]);
}

#[test]
fn s9_running_a_deadline_clears_it() {
    let mut core = LoopCore::<u64>::new(&[], 1);
    core.set_deadline(hdl(0), Some(5));
    core.begin_pass(5);
    assert_eq!(answers(&mut core, 2), [RUN_0, DONE]);
    assert_eq!(core.next_deadline(), None);
}

#[test]
fn s9_deadline_set_again_waits_for_next_pass() {
    let mut core = LoopCore::<u64>::new(&[], 1);
    core.set_deadline(hdl(0), Some(5));
    core.begin_pass(5);
    assert_eq!(core.next_turn(), RUN_0);
    core.set_deadline(hdl(0), Some(5));
    assert_eq!(core.next_turn(), DONE);
    core.begin_pass(5);
    assert_eq!(core.next_turn(), RUN_0);
}

#[test]
fn s10_time_is_fixed_per_pass() {
    let mut core = LoopCore::<u64>::new(&[], 1);
    core.set_deadline(hdl(0), Some(6));
    core.begin_pass(5);
    assert_eq!(core.next_turn(), DONE);
    core.begin_pass(6);
    assert_eq!(core.next_turn(), RUN_0);
}

#[test]
fn s11_everything_made_ready_and_due_again_after_each_answer() {
    let mut core = LoopCore::<u64>::new(&[2, 1], 1);
    core.begin_pass(0);
    let mut result = Vec::new();
    for _ in 0..5 {
        core.set_ready(src(0), true);
        core.set_ready(src(1), true);
        core.set_deadline(hdl(0), Some(0));
        result.push(core.next_turn());
    }
    assert_eq!(result, [SERVE_0, SERVE_0, SERVE_1, RUN_0, DONE]);
}

#[test]
fn s11_pass_ends_within_sum_of_budgets_plus_handlers() {
    let budget_sets: [&[usize]; 5] = [&[], &[0], &[1, 1, 1], &[2, 0, 3], &[5]];
    for budgets in budget_sets {
        for handler_count in 0..4 {
            let mut core = LoopCore::<u64>::new(budgets, handler_count);
            core.begin_pass(0);
            let bound = budgets.iter().sum::<usize>() + handler_count;
            let mut count = 0;
            loop {
                for index in 0..budgets.len() {
                    core.set_ready(src(index), true);
                }
                for index in 0..handler_count {
                    core.set_deadline(hdl(index), Some(0));
                }
                if core.next_turn() == DONE {
                    break;
                }
                count += 1;
                assert!(count <= bound, "more than {bound} answers before PassDone");
            }
            assert_eq!(count, bound);
        }
    }
}

#[test]
fn s12_next_deadline_is_earliest_of_unpaused() {
    let mut core = LoopCore::<u64>::new(&[], 3);
    core.set_deadline(hdl(0), Some(7));
    core.set_deadline(hdl(1), Some(3));
    core.set_deadline(hdl(2), Some(1));
    core.set_deadline_paused(hdl(2), true);
    assert_eq!(core.next_deadline(), Some(3));
}

#[test]
fn s12_next_deadline_none_without_deadlines_and_changes_nothing() {
    let mut core = LoopCore::<u64>::new(&[], 2);
    assert_eq!(core.next_deadline(), None);
    core.set_deadline(hdl(0), Some(4));
    core.set_deadline_paused(hdl(0), true);
    assert_eq!(core.next_deadline(), None);
    core.begin_pass(10);
    assert_eq!(core.next_deadline(), None);
    assert_eq!(core.next_turn(), DONE);
    core.set_deadline_paused(hdl(0), false);
    assert_eq!(core.next_deadline(), Some(4));
    assert_eq!(core.next_deadline(), Some(4));
}

#[test]
fn s13_has_ready_source() {
    let mut core = LoopCore::<u64>::new(&[1, 0], 0);
    assert!(!core.has_ready_source());
    core.set_ready(src(0), true);
    assert!(core.has_ready_source());
    core.set_paused(src(0), true);
    assert!(!core.has_ready_source());
    core.set_paused(src(0), false);
    core.set_ready(src(0), false);
    core.set_ready(src(1), true);
    assert!(!core.has_ready_source(), "budget 0 does not count");
}

#[test]
fn s13_budget_used_in_the_pass_does_not_matter() {
    let mut core = LoopCore::<u64>::new(&[1], 0);
    core.set_ready(src(0), true);
    core.begin_pass(0);
    assert_eq!(core.next_turn(), SERVE_0);
    core.set_ready(src(0), true);
    assert!(core.has_ready_source());
    assert_eq!(core.next_turn(), DONE);
    assert!(core.has_ready_source());
}

#[test]
fn s14_serving_clears_only_the_served_ready_flag() {
    let mut core = LoopCore::<u64>::new(&[1, 1], 1);
    core.set_ready(src(0), true);
    core.set_ready(src(1), true);
    core.set_paused(src(1), true);
    core.set_deadline(hdl(0), Some(9));
    core.set_deadline_paused(hdl(0), true);
    core.begin_pass(0);
    assert_eq!(core.next_turn(), SERVE_0);
    // Source 1 keeps ready and paused, the handler keeps deadline and pause.
    core.set_paused(src(1), false);
    assert!(core.has_ready_source());
    assert_eq!(core.next_turn(), SERVE_1);
    core.set_deadline_paused(hdl(0), false);
    assert_eq!(core.next_deadline(), Some(9));
}

#[test]
fn s16_begin_pass_during_a_pass_restarts_with_full_budget() {
    let mut core = LoopCore::<u64>::new(&[1], 0);
    core.set_ready(src(0), true);
    core.begin_pass(0);
    assert_eq!(core.next_turn(), SERVE_0);
    core.set_ready(src(0), true);
    core.begin_pass(1);
    assert_eq!(answers(&mut core, 2), [SERVE_0, DONE]);
}

#[test]
fn s16_begin_pass_returns_position_to_source_0_and_takes_new_time() {
    let mut core = LoopCore::<u64>::new(&[1, 1], 1);
    core.set_ready(src(0), true);
    core.set_ready(src(1), true);
    core.set_deadline(hdl(0), Some(6));
    core.begin_pass(5);
    assert_eq!(core.next_turn(), SERVE_0);
    assert_eq!(core.next_turn(), SERVE_1);
    core.set_ready(src(0), true);
    core.begin_pass(6);
    assert_eq!(answers(&mut core, 3), [SERVE_0, RUN_0, DONE]);
}

#[test]
fn s17_setters_change_only_their_target() {
    let mut core = LoopCore::<u64>::new(&[1, 1], 2);
    core.set_paused(src(0), true);
    core.set_paused(src(1), true);
    core.set_ready(src(1), true);
    core.set_paused(src(1), false);
    // Source 0 is still paused; source 1 is ready and served with its own budget only.
    core.set_ready(src(0), true);
    core.set_deadline(hdl(0), Some(1));
    core.set_deadline_paused(hdl(1), true);
    core.set_deadline(hdl(1), Some(2));
    core.begin_pass(5);
    assert_eq!(answers(&mut core, 3), [SERVE_1, RUN_0, DONE]);
    // Handler 1 kept its pause and its deadline.
    assert_eq!(core.next_deadline(), None);
    core.set_deadline_paused(hdl(1), false);
    assert_eq!(core.next_deadline(), Some(2));
    core.set_deadline(hdl(1), None);
    assert_eq!(core.next_deadline(), None);
}

#[test]
#[should_panic(expected = "index out of bounds")]
fn s15_set_ready_unknown_source_panics() {
    let mut core = LoopCore::<u64>::new(&[1], 1);
    core.set_ready(src(1), true);
}

#[test]
#[should_panic(expected = "index out of bounds")]
fn s15_set_paused_unknown_source_panics() {
    let mut core = LoopCore::<u64>::new(&[1], 1);
    core.set_paused(src(1), true);
}

#[test]
#[should_panic(expected = "index out of bounds")]
fn s15_set_deadline_unknown_handler_panics() {
    let mut core = LoopCore::<u64>::new(&[1], 1);
    core.set_deadline(hdl(1), Some(0));
}

#[test]
#[should_panic(expected = "index out of bounds")]
fn s15_set_deadline_paused_unknown_handler_panics() {
    let mut core = LoopCore::<u64>::new(&[1], 1);
    core.set_deadline_paused(hdl(1), true);
}

#[test]
fn s15_many_passes_do_not_overflow_or_panic() {
    let mut core = LoopCore::<u64>::new(&[usize::MAX, 1], 1);
    core.set_deadline(hdl(0), Some(u64::MAX));
    core.set_ready(src(0), true);
    for pass in 0..10_000 {
        core.begin_pass(u64::MAX);
        core.set_ready(src(0), true);
        assert_eq!(core.next_turn(), SERVE_0, "pass {pass}");
        core.set_deadline(hdl(0), Some(u64::MAX));
        assert_eq!(answers(&mut core, 2), [RUN_0, DONE]);
    }
}
