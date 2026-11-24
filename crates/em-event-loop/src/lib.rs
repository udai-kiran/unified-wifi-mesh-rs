//! Turn-by-turn scheduler core in the style of libae (note 0016 U13, U15, U28; note 0017).
//!
//! This crate owns one thing: answering "what should be done next?". It knows sources
//! (something that can have work), handlers (something that can have one timer), ready and
//! pause flags, budgets, deadlines and a generic time type. It never does the work itself.
//!
//! It must not own any `EasyMesh` type, any I/O, any logging, any queue size, drop rule or
//! timer value (all application policy). The async driver [`run`] runs passes against an
//! [`Io`] and an [`App`] (note 0016 D2, U14).
//!
//! One pass: [`LoopCore::begin_pass`] fixes the time `now`, then the application calls
//! [`LoopCore::next_turn`] until it answers [`Turn::PassDone`]. Sources are served in index
//! order, each at most its budget; then every due deadline runs once, in handler order.
//! Serving a source clears its ready flag; running a deadline clears it. Ids are indexes: an
//! id that does not exist is a programming bug and panics.
//!
//! The behaviour is fixed by the English spec `verify/spec-en.md` (rules S1 to S17) and the
//! Lean step spec `verify/lean/StepSpec.lean`. The code is written in the style of note 0016
//! erratum E8 (index loops, small `bool` helpers, guarded counters) so that Aeneas can extract
//! it for the proofs.

#[cfg(test)]
mod model_tests;
#[cfg(test)]
mod spec_examples_tests;

/// Index of a source, in the order given to [`LoopCore::new`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceId(pub usize);

/// Index of a handler.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HandlerId(pub usize);

/// What the application must do next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Turn {
    /// Serve this source; its ready flag is now cleared.
    Serve(SourceId),
    /// Run the deadline of this handler; its deadline is now cleared.
    Deadline(HandlerId),
    /// Nothing more in this pass.
    PassDone,
}

struct SourceState {
    budget: usize,
    /// Turns given to this source in the current pass.
    used: usize,
    ready: bool,
    paused: bool,
}

struct HandlerState<T> {
    deadline: Option<T>,
    paused: bool,
}

enum Phase<T> {
    /// Before the first `begin_pass`, and after `PassDone`.
    Done,
    /// Looking at sources from `position` (the last source served, 0 at the start).
    Serving { now: T, position: usize },
    /// Looking at handlers from `next`.
    Deadlines { now: T, next: usize },
}

/// The scheduler core. `T` is the time type.
pub struct LoopCore<T: Ord + Copy> {
    sources: Vec<SourceState>,
    handlers: Vec<HandlerState<T>>,
    phase: Phase<T>,
}

impl<T: Ord + Copy> LoopCore<T> {
    /// Creates a loop with one source per entry of `budgets` (the most times it may be
    /// served in one pass) and `handler_count` handlers. Sources start not ready and not
    /// paused; handlers start with no deadline and not paused. No pass has begun, so
    /// `next_turn` answers `PassDone` until `begin_pass`.
    #[must_use]
    pub fn new(budgets: &[usize], handler_count: usize) -> Self {
        // Index loops, not iterators: they extract to Lean without hand-written models.
        let mut sources = Vec::new();
        let mut index = 0;
        while index < budgets.len() {
            sources.push(SourceState {
                budget: budgets[index],
                used: 0,
                ready: false,
                paused: false,
            });
            index += 1;
        }
        let mut handlers = Vec::new();
        let mut count = 0;
        while count < handler_count {
            handlers.push(HandlerState {
                deadline: None,
                paused: false,
            });
            count += 1;
        }
        LoopCore {
            sources,
            handlers,
            phase: Phase::Done,
        }
    }

    /// Sets the ready flag of a source.
    ///
    /// # Panics
    /// If the source does not exist.
    pub fn set_ready(&mut self, source: SourceId, ready: bool) {
        self.sources[source.0].ready = ready;
    }

    /// Sets the pause flag of a source. Pausing keeps the ready flag.
    ///
    /// # Panics
    /// If the source does not exist.
    pub fn set_paused(&mut self, source: SourceId, paused: bool) {
        self.sources[source.0].paused = paused;
    }

    /// Sets the deadline of a handler; `None` clears it.
    ///
    /// # Panics
    /// If the handler does not exist.
    pub fn set_deadline(&mut self, handler: HandlerId, deadline: Option<T>) {
        self.handlers[handler.0].deadline = deadline;
    }

    /// Sets the pause flag of a handler's timer. Pausing keeps the deadline.
    ///
    /// # Panics
    /// If the handler does not exist.
    pub fn set_deadline_paused(&mut self, handler: HandlerId, paused: bool) {
        self.handlers[handler.0].paused = paused;
    }

    /// The earliest deadline among handlers that have one and are not paused.
    #[must_use]
    pub fn next_deadline(&self) -> Option<T> {
        let mut earliest: Option<T> = None;
        let mut index = 0;
        while index < self.handlers.len() {
            let handler = &self.handlers[index];
            if !handler.paused {
                if let Some(deadline) = handler.deadline {
                    if is_earlier_or_unset(earliest, deadline) {
                        earliest = Some(deadline);
                    }
                }
            }
            index += 1;
        }
        earliest
    }

    /// Whether some source is ready, not paused and has a budget above 0. Budget used in
    /// the current pass does not matter.
    #[must_use]
    pub fn has_ready_source(&self) -> bool {
        let mut index = 0;
        while index < self.sources.len() {
            if self.source_has_work(index) {
                return true;
            }
            index += 1;
        }
        false
    }

    /// Starts a pass at time `now`, abandoning any pass in progress. Position and budgets
    /// start again; ready flags, pause flags and deadlines are kept.
    pub fn begin_pass(&mut self, now: T) {
        let mut index = 0;
        while index < self.sources.len() {
            self.sources[index].used = 0;
            index += 1;
        }
        self.phase = Phase::Serving { now, position: 0 };
    }

    /// The next answer of the current pass. Every condition is checked at this call.
    pub fn next_turn(&mut self) -> Turn {
        match self.phase {
            Phase::Done => Turn::PassDone,
            Phase::Serving { now, position } => {
                let source = self.first_eligible_source(position);
                if source < self.sources.len() {
                    self.sources[source].ready = false;
                    // Eligible means `used < budget`, so this cannot overflow.
                    self.sources[source].used += 1;
                    self.phase = Phase::Serving {
                        now,
                        position: source,
                    };
                    Turn::Serve(SourceId(source))
                } else {
                    self.turn_from_handler(now, 0)
                }
            }
            Phase::Deadlines { now, next } => self.turn_from_handler(now, next),
        }
    }

    /// Ready, not paused, and budget left in this pass.
    fn source_is_eligible(&self, index: usize) -> bool {
        let source = &self.sources[index];
        source.ready && !source.paused && source.used < source.budget
    }

    /// Ready, not paused, and a budget above 0.
    fn source_has_work(&self, index: usize) -> bool {
        let source = &self.sources[index];
        source.ready && !source.paused && 0 < source.budget
    }

    /// Has a deadline, is not paused, and the deadline is not later than `now`.
    fn handler_is_due(&self, index: usize, now: T) -> bool {
        let handler = &self.handlers[index];
        !handler.paused && deadline_is_due(handler.deadline, now)
    }

    /// First eligible source from `start`, or the number of sources if there is none.
    fn first_eligible_source(&self, start: usize) -> usize {
        let mut index = start;
        while index < self.sources.len() {
            if self.source_is_eligible(index) {
                return index;
            }
            index += 1;
        }
        self.sources.len()
    }

    /// First due handler from `start`, or the number of handlers if there is none.
    fn first_due_handler(&self, start: usize, now: T) -> usize {
        let mut index = start;
        while index < self.handlers.len() {
            if self.handler_is_due(index, now) {
                return index;
            }
            index += 1;
        }
        self.handlers.len()
    }

    /// Runs the first due handler from `start`, or finishes the pass.
    fn turn_from_handler(&mut self, now: T, start: usize) -> Turn {
        let handler = self.first_due_handler(start, now);
        if handler < self.handlers.len() {
            self.handlers[handler].deadline = None;
            // `handler < len`, so `handler + 1` cannot overflow.
            self.phase = Phase::Deadlines {
                now,
                next: handler + 1,
            };
            Turn::Deadline(HandlerId(handler))
        } else {
            self.phase = Phase::Done;
            Turn::PassDone
        }
    }
}

/// True if `earliest` is unset or not earlier than `deadline`.
fn is_earlier_or_unset<T: Ord + Copy>(earliest: Option<T>, deadline: T) -> bool {
    match earliest {
        None => true,
        Some(best) => deadline <= best,
    }
}

/// True if there is a deadline and it is not later than `now`.
fn deadline_is_due<T: Ord + Copy>(deadline: Option<T>, now: T) -> bool {
    match deadline {
        None => false,
        Some(time) => time <= now,
    }
}

// The driver sits after `LoopCore` so that the frozen extraction's source positions do not move
// (note 0017 A2 (c)); `verify/extract.sh` excludes it.
mod driver;
#[cfg(test)]
mod driver_tests;

pub use driver::{App, Io, run};
