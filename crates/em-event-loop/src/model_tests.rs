//! Seeded model-based test: random calls on random configurations, compared with a small,
//! straightforward reference model of the spec (it may use iterators; the real code may not).

use super::{HandlerId, LoopCore, Phase, SourceId, Turn};

/// xorshift64: a tiny deterministic generator, so no dependency is needed.
struct Prng(u64);

impl Prng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    /// A number in `0..limit`; `limit` must be above 0.
    fn below(&mut self, limit: usize) -> usize {
        let limit = u64::try_from(limit).unwrap();
        usize::try_from(self.next() % limit).unwrap()
    }

    fn flag(&mut self) -> bool {
        self.below(2) == 1
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ModelPhase {
    Done,
    Serving { now: u64, position: usize },
    Deadlines { now: u64, next: usize },
}

/// The spec, written for readability. `used` is kept per source as in the step spec.
struct Model {
    budgets: Vec<usize>,
    used: Vec<usize>,
    ready: Vec<bool>,
    paused: Vec<bool>,
    deadlines: Vec<Option<u64>>,
    deadline_paused: Vec<bool>,
    phase: ModelPhase,
}

impl Model {
    fn new(budgets: &[usize], handler_count: usize) -> Self {
        Model {
            budgets: budgets.to_vec(),
            used: vec![0; budgets.len()],
            ready: vec![false; budgets.len()],
            paused: vec![false; budgets.len()],
            deadlines: vec![None; handler_count],
            deadline_paused: vec![false; handler_count],
            phase: ModelPhase::Done,
        }
    }

    fn eligible(&self, index: usize) -> bool {
        self.ready[index] && !self.paused[index] && self.used[index] < self.budgets[index]
    }

    fn due(&self, index: usize, now: u64) -> bool {
        !self.deadline_paused[index] && self.deadlines[index].is_some_and(|time| time <= now)
    }

    fn begin_pass(&mut self, now: u64) {
        self.used = vec![0; self.budgets.len()];
        self.phase = ModelPhase::Serving { now, position: 0 };
    }

    fn scan_handlers(&mut self, now: u64, start: usize) -> Turn {
        let found = (start..self.deadlines.len()).find(|&index| self.due(index, now));
        if let Some(index) = found {
            self.deadlines[index] = None;
            self.phase = ModelPhase::Deadlines {
                now,
                next: index + 1,
            };
            Turn::Deadline(HandlerId(index))
        } else {
            self.phase = ModelPhase::Done;
            Turn::PassDone
        }
    }

    fn next_turn(&mut self) -> Turn {
        match self.phase {
            ModelPhase::Done => Turn::PassDone,
            ModelPhase::Serving { now, position } => {
                let found = (position..self.budgets.len()).find(|&index| self.eligible(index));
                if let Some(index) = found {
                    self.ready[index] = false;
                    self.used[index] += 1;
                    self.phase = ModelPhase::Serving {
                        now,
                        position: index,
                    };
                    Turn::Serve(SourceId(index))
                } else {
                    self.scan_handlers(now, 0)
                }
            }
            ModelPhase::Deadlines { now, next } => self.scan_handlers(now, next),
        }
    }

    fn next_deadline(&self) -> Option<u64> {
        self.deadlines
            .iter()
            .zip(&self.deadline_paused)
            .filter(|(_, paused)| !**paused)
            .filter_map(|(deadline, _)| *deadline)
            .min()
    }

    fn has_ready_source(&self) -> bool {
        (0..self.budgets.len())
            .any(|index| self.ready[index] && !self.paused[index] && 0 < self.budgets[index])
    }
}

fn assert_same_state(core: &LoopCore<u64>, model: &Model, context: &str) {
    assert_eq!(core.sources.len(), model.budgets.len(), "{context}");
    assert_eq!(core.handlers.len(), model.deadlines.len(), "{context}");
    for (index, source) in core.sources.iter().enumerate() {
        assert_eq!(
            source.budget, model.budgets[index],
            "{context}: budget {index}"
        );
        assert_eq!(source.ready, model.ready[index], "{context}: ready {index}");
        assert_eq!(
            source.paused, model.paused[index],
            "{context}: paused {index}"
        );
        assert_eq!(source.used, model.used[index], "{context}: used {index}");
    }
    for (index, handler) in core.handlers.iter().enumerate() {
        assert_eq!(
            handler.deadline, model.deadlines[index],
            "{context}: deadline {index}"
        );
        assert_eq!(
            handler.paused, model.deadline_paused[index],
            "{context}: handler pause {index}"
        );
    }
    let phase = match core.phase {
        Phase::Done => ModelPhase::Done,
        Phase::Serving { now, position } => ModelPhase::Serving { now, position },
        Phase::Deadlines { now, next } => ModelPhase::Deadlines { now, next },
    };
    assert_eq!(phase, model.phase, "{context}: phase");
}

/// Shape rules of S3: source numbers never go down, handler numbers strictly go up, no
/// "serve" after a "run deadline", and nothing after "pass done".
struct ShapeCheck {
    last_source: Option<usize>,
    last_handler: Option<usize>,
    finished: bool,
}

impl ShapeCheck {
    fn new() -> Self {
        ShapeCheck {
            last_source: None,
            last_handler: None,
            finished: false,
        }
    }

    fn check(&mut self, turn: Turn) {
        match turn {
            Turn::Serve(SourceId(index)) => {
                assert!(!self.finished && self.last_handler.is_none());
                assert!(self.last_source.is_none_or(|last| last <= index));
                self.last_source = Some(index);
            }
            Turn::Deadline(HandlerId(index)) => {
                assert!(!self.finished);
                assert!(self.last_handler.is_none_or(|last| last < index));
                self.last_handler = Some(index);
            }
            Turn::PassDone => self.finished = true,
        }
    }
}

fn run_seed(seed: u64, steps: usize) {
    let mut random = Prng(seed);
    let mut core = LoopCore::<u64>::new(&[], 0);
    let mut model = Model::new(&[], 0);
    let mut shape = ShapeCheck::new();
    let mut answers_this_pass = 0;
    let mut bound = 0;
    for step in 0..steps {
        let context = format!("seed {seed} step {step}");
        // Reconfigure now and then, so many shapes are covered.
        if step % 400 == 0 {
            let budgets: Vec<usize> = (0..random.below(5)).map(|_| random.below(4)).collect();
            let handler_count = random.below(5);
            core = LoopCore::new(&budgets, handler_count);
            model = Model::new(&budgets, handler_count);
            assert_same_state(&core, &model, &context);
            shape = ShapeCheck::new();
            bound = 0;
        }
        let source = random.below(model.budgets.len().max(1));
        let handler = random.below(model.deadlines.len().max(1));
        let has_sources = !model.budgets.is_empty();
        let has_handlers = !model.deadlines.is_empty();
        // Small times, so deadlines often equal `now`.
        let time = u64::try_from(random.below(8)).unwrap();
        match random.below(12) {
            0 => {
                core.begin_pass(time);
                model.begin_pass(time);
                shape = ShapeCheck::new();
                answers_this_pass = 0;
                bound = model.budgets.iter().sum::<usize>() + model.deadlines.len();
            }
            1..=4 => {
                let got = core.next_turn();
                assert_eq!(got, model.next_turn(), "{context}: next_turn");
                shape.check(got);
                if got != Turn::PassDone {
                    answers_this_pass += 1;
                    assert!(answers_this_pass <= bound, "{context}: S11 bound");
                }
            }
            5 if has_sources => {
                let ready = random.flag();
                core.set_ready(SourceId(source), ready);
                model.ready[source] = ready;
            }
            6 if has_sources => {
                let paused = random.flag();
                core.set_paused(SourceId(source), paused);
                model.paused[source] = paused;
            }
            7 | 8 if has_handlers => {
                let deadline = if random.below(4) == 0 {
                    None
                } else {
                    Some(time)
                };
                core.set_deadline(HandlerId(handler), deadline);
                model.deadlines[handler] = deadline;
            }
            9 if has_handlers => {
                let paused = random.flag();
                core.set_deadline_paused(HandlerId(handler), paused);
                model.deadline_paused[handler] = paused;
            }
            10 => assert_eq!(core.next_deadline(), model.next_deadline(), "{context}"),
            11 => assert_eq!(
                core.has_ready_source(),
                model.has_ready_source(),
                "{context}"
            ),
            _ => {}
        }
        assert_same_state(&core, &model, &context);
    }
}

#[test]
fn model_based_random_calls_match_the_reference_model() {
    for seed in [
        1,
        2,
        3,
        5,
        8,
        13,
        21,
        34,
        55,
        89,
        0x9E37_79B9_7F4A_7C15,
        0xDEAD_BEEF,
        42,
        4242,
        123_456_789,
        987_654_321,
        7,
        77,
        777,
        7777,
    ] {
        run_seed(seed, 4000);
    }
}
