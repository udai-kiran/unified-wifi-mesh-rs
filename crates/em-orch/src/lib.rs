// SPDX-License-Identifier: Apache-2.0

//! Synchronous command orchestration (design notes 0012 D10 and 0013 D3).
//!
//! The caller applies submission effects before admission. During a pass each
//! hook's effects are applied before the next hook. All times come from the caller.

use std::collections::HashMap;
use std::marker::PhantomData;
use std::time::Instant;

use em_cmd::{ActiveOp, ChainId, Command, CommandType, CommandView, NodeKey, OrchOp, OrchState};
use em_types::FreqBand;

/// A role change or a request to the role core.
#[derive(Debug, Copy, PartialEq, Eq)]
pub enum RoleEffect<S: Copy> {
    /// Create a node during command pre-processing.
    CreateNode {
        /// Node identity.
        key: NodeKey,
        /// Radio band, or no band for an AL node.
        band: Option<FreqBand>,
        /// Local Multi-AP profile.
        profile: u8,
        /// Initial role-specific state.
        state: S,
    },
    /// Set a node's role-specific state.
    SetState(NodeKey, S),
    /// Ask the core to send one autoconfiguration renew for this radio.
    Renew(NodeKey),
    /// Ask the core to restart this node's protocol deadline.
    RestartProto(NodeKey),
}

// Deriving Clone for this generic enum calls Clone on its fields, which the
// workspace forbids even though all fields are Copy.
#[expect(
    clippy::expl_impl_clone_on_copy,
    reason = "copy without field clone calls"
)]
impl<S: Copy> Clone for RoleEffect<S> {
    fn clone(&self) -> Self {
        *self
    }
}

enum TransientAction<S: Copy> {
    Effect(RoleEffect<S>),
    ResetStartTime,
    Cancel(CommandType),
    CancelScoped(CommandType, Vec<NodeKey>),
}

/// Ordered effects and orchestrator requests from a transient hook.
///
/// A reset immediately changes [`Self::elapsed`] to zero, matching the order
/// in `src/orch/em_orch_agent.cpp:52-65`. The orchestrator applies the reset
/// and cancel requests before processing the next candidate.
pub struct TransientOut<S: Copy> {
    actions: Vec<TransientAction<S>>,
    kind: CommandType,
    elapsed: u64,
}

impl<S: Copy> TransientOut<S> {
    fn new(kind: CommandType, elapsed: u64) -> Self {
        Self {
            actions: Vec::new(),
            kind,
            elapsed,
        }
    }

    /// Adds a role effect in hook order.
    pub fn effect(&mut self, effect: RoleEffect<S>) {
        self.actions.push(TransientAction::Effect(effect));
    }

    /// Requests a new start time and resets the type's elapsed value to zero.
    pub fn reset_start_time(&mut self) {
        self.elapsed = 0;
        self.actions.push(TransientAction::ResetStartTime);
    }

    /// Returns the elapsed value after any reset requested by this hook.
    #[must_use]
    pub const fn elapsed(&self) -> u64 {
        self.elapsed
    }

    /// Requests cancellation of pending and active stages of this type.
    pub fn cancel(&mut self, kind: CommandType) {
        self.actions.push(TransientAction::Cancel(kind));
    }

    /// Requests cancellation limited to these candidates, resetting type elapsed time.
    pub fn cancel_scoped(&mut self, kind: CommandType, candidates: Vec<NodeKey>) {
        if self.kind == kind {
            self.elapsed = 0;
        }
        self.actions
            .push(TransientAction::CancelScoped(kind, candidates));
    }
}

/// Role-specific rules invoked by the orchestrator.
///
/// Hooks are synchronous. Each hook's effects are applied before another hook
/// sees the node table (0013 D3).
pub trait OrchRole {
    /// The role's node table.
    type Nodes;
    /// The role-specific node state.
    type State: Copy;
    /// Ordered requests for the role core to drain after the pass.
    type Out;

    /// Applies a node effect immediately or records a request in `out`.
    fn apply(
        &mut self,
        nodes: &mut Self::Nodes,
        effect: RoleEffect<Self::State>,
        out: &mut Self::Out,
    );

    /// Pre-processes a plan stage, returning whether to admit it.
    ///
    /// Upstream calls this for every stage before candidate construction
    /// (`src/orch/em_orch.cpp:42-69`).
    fn pre_process(
        &mut self,
        cmd: &Command,
        op: OrchOp,
        out: &mut Vec<RoleEffect<Self::State>>,
    ) -> bool;

    /// Builds candidates in node-table order after pre-processing effects apply.
    ///
    /// See `src/orch/em_orch.cpp:142-154` and
    /// `src/orch/em_orch_agent.cpp:320-345`.
    fn build_candidates(&self, cmd: &Command, op: OrchOp, nodes: &Self::Nodes) -> Vec<NodeKey>;

    /// Returns whether a pending node may begin executing.
    fn ready_for_exec(
        &self,
        cmd: &Command,
        node: NodeKey,
        nodes: &Self::Nodes,
        out: &mut Vec<RoleEffect<Self::State>>,
    ) -> bool;

    /// Returns whether an executing node has finished.
    fn ready_for_fini(
        &self,
        cmd: &Command,
        node: NodeKey,
        nodes: &Self::Nodes,
        out: &mut Vec<RoleEffect<Self::State>>,
    ) -> bool;

    /// Emits role effects when execution starts (`src/em/em.cpp:86-114`).
    fn on_start(&self, cmd: &Command, node: NodeKey, out: &mut Vec<RoleEffect<Self::State>>);

    /// Handles an unready candidate during a timer tick.
    ///
    /// `elapsed` is the maximum elapsed whole seconds for this command type.
    /// After requesting a reset, use [`TransientOut::elapsed`] to read the
    /// value that the upstream hook checks after its reset.
    fn transient(
        &self,
        cmd: &Command,
        node: NodeKey,
        nodes: &Self::Nodes,
        elapsed: u64,
        out: &mut TransientOut<Self::State>,
    );

    /// Emits role effects for each active candidate of a cancelled type.
    ///
    /// See `src/orch/em_orch.cpp:291-301`.
    fn pre_process_cancel(
        &self,
        cmd: &Command,
        node: NodeKey,
        out: &mut Vec<RoleEffect<Self::State>>,
    );
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum StageState {
    Deferred,
    Pending,
    Active,
    Done,
}

struct Stage {
    op: OrchOp,
    state: StageState,
    candidates: Vec<NodeKey>,
    start: Option<Instant>,
}

struct Chain {
    command: Command,
    stages: Vec<Stage>,
    admitted: bool,
}

#[derive(Clone, Copy)]
struct StageId {
    chain: ChainId,
    stage: usize,
}

#[derive(Clone, Copy)]
struct NodeStatus {
    state: OrchState,
    active: Option<ActiveOp>,
}

#[derive(Default)]
struct Stats {
    count: usize,
    elapsed: u64,
}

/// Owns command chains, queues, per-type timers, and node orchestration state.
pub struct Orchestrator<R: OrchRole> {
    next_chain: u64,
    chains: HashMap<ChainId, Chain>,
    pending: Vec<StageId>,
    active: Vec<StageId>,
    nodes: HashMap<NodeKey, NodeStatus>,
    stats: HashMap<CommandType, Stats>,
    role: PhantomData<fn() -> R>,
}

impl<R: OrchRole> Default for Orchestrator<R> {
    fn default() -> Self {
        Self {
            next_chain: 0,
            chains: HashMap::new(),
            pending: Vec::new(),
            active: Vec::new(),
            nodes: HashMap::new(),
            stats: HashMap::new(),
            role: PhantomData,
        }
    }
}

impl<R: OrchRole> Orchestrator<R> {
    /// Creates an empty orchestrator.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Pre-processes every plan stage and returns its effects for the caller.
    ///
    /// The caller must apply these effects before calling [`Self::admit`].
    /// Upstream pre-processes each stage at submit (`src/orch/em_orch.cpp:42-69`);
    /// timing starts only on activation (`src/orch/em_orch.cpp:532-540`).
    ///
    /// # Panics
    ///
    /// Panics if the orchestrator exhausts its `u64` chain IDs.
    pub fn submit(
        &mut self,
        role: &mut R,
        _nodes: &mut R::Nodes,
        command: Command,
        _now: Instant,
    ) -> (ChainId, Vec<RoleEffect<R::State>>) {
        let id = ChainId(self.next_chain);
        self.next_chain = self.next_chain.checked_add(1).expect("chain IDs exhausted");
        let mut effects = Vec::new();
        let mut stages = Vec::new();
        for plan_stage in command.plan() {
            let submit = role.pre_process(&command, plan_stage.op, &mut effects);
            stages.push(Stage {
                op: plan_stage.op,
                state: if submit {
                    StageState::Deferred
                } else {
                    StageState::Done
                },
                candidates: Vec::new(),
                start: None,
            });
        }
        self.chains.insert(
            id,
            Chain {
                command,
                stages,
                admitted: false,
            },
        );
        tracing::debug!(chain = id.0, "command pre-processed");
        (id, effects)
    }

    /// Builds candidates and queues stages after submission effects are applied.
    ///
    /// Returns false and destroys the chain when no stage has candidates, as
    /// in `src/orch/em_orch.cpp:142-154`. An unknown chain returns false.
    /// Re-admitting an already-admitted chain returns true without building or
    /// queueing candidates again.
    pub fn admit(&mut self, role: &mut R, nodes: &mut R::Nodes, id: ChainId) -> bool {
        let Some(chain) = self.chains.get(&id) else {
            return false;
        };
        if chain.admitted {
            return true;
        }
        let candidates: Vec<_> = chain
            .stages
            .iter()
            .enumerate()
            .filter(|(_, stage)| stage.state == StageState::Deferred)
            .map(|(index, stage)| {
                (
                    index,
                    role.build_candidates(&chain.command, stage.op, nodes),
                )
            })
            .collect();
        let Some(chain) = self.chains.get_mut(&id) else {
            return false;
        };
        chain.admitted = true;
        let kind = chain.command.kind();
        let mut admitted = false;
        for (index, candidates) in candidates {
            let stage = &mut chain.stages[index];
            if candidates.is_empty() {
                stage.state = StageState::Done;
            } else {
                stage.candidates = candidates;
                stage.state = StageState::Pending;
                self.pending.push(StageId {
                    chain: id,
                    stage: index,
                });
                self.stats.entry(kind).or_default().count += 1;
                admitted = true;
            }
        }
        if admitted {
            tracing::debug!(chain = id.0, ?kind, "command admitted");
        } else {
            self.chains.remove(&id);
            tracing::debug!(chain = id.0, "command has no candidates");
        }
        admitted
    }

    /// Runs one timer pass, applying effects and recording ordered requests in `out`.
    ///
    /// Like `src/orch/em_orch.cpp:316-345,521-572`, the pass activates at most
    /// one eligible stage, choosing the newest eligible pending stage. It then
    /// visits active stages newest first and their candidates from last to
    /// first. A newly finished candidate is destroyed only on a subsequent pass.
    ///
    /// Elapsed time is `(now - start).as_secs()` (saturating at zero for a
    /// backwards input) and is maximized per type. This intentionally differs
    /// from upstream's subtraction of `tv_sec` in `src/orch/em_orch.cpp:72-88`:
    /// it can reach the next whole second up to one second later (0012 D2).
    pub fn tick(&mut self, role: &mut R, nodes: &mut R::Nodes, now: Instant, out: &mut R::Out) {
        self.pass(role, nodes, now, true, out);
    }

    /// Runs activation and readiness on an event without updating timers or
    /// calling transient hooks (0012 D10, 0002 D4).
    pub fn on_event(&mut self, role: &mut R, nodes: &mut R::Nodes, now: Instant, out: &mut R::Out) {
        self.pass(role, nodes, now, false, out);
    }

    /// Returns whether any admitted stage of this type remains pending or active.
    ///
    /// Upstream tracks admitted stages per type (`src/orch/em_orch.cpp:93-127,142-154`).
    #[must_use]
    pub fn is_type_in_progress(&self, kind: CommandType) -> bool {
        self.stats.contains_key(&kind)
    }

    fn pass(
        &mut self,
        role: &mut R,
        nodes: &mut R::Nodes,
        now: Instant,
        timer: bool,
        out: &mut R::Out,
    ) {
        self.activate(now);
        for index in (0..self.active.len()).rev() {
            let id = self.active[index];
            let count = self.chains[&id.chain].stages[id.stage].candidates.len();
            let mut done = true;
            for candidate_index in (0..count).rev() {
                let node = self.chains[&id.chain].stages[id.stage].candidates[candidate_index];
                let state = self.orch_state(node);
                match state {
                    OrchState::Pending => {
                        done = false;
                        let mut effects = Vec::new();
                        let ready = role.ready_for_exec(
                            &self.chains[&id.chain].command,
                            node,
                            nodes,
                            &mut effects,
                        );
                        Self::apply_effects(role, nodes, effects, out);
                        if ready {
                            let kind = self.chains[&id.chain].command.kind();
                            self.nodes.insert(
                                node,
                                NodeStatus {
                                    state: OrchState::Progress,
                                    active: Some(ActiveOp {
                                        chain: id.chain,
                                        stage: id.stage,
                                        kind,
                                    }),
                                },
                            );
                            let mut effects = Vec::new();
                            role.on_start(&self.chains[&id.chain].command, node, &mut effects);
                            Self::apply_effects(role, nodes, effects, out);
                        } else if timer {
                            self.run_transient(role, nodes, id, node, now, out);
                        }
                    }
                    OrchState::Progress => {
                        done = false;
                        let mut effects = Vec::new();
                        let ready = role.ready_for_fini(
                            &self.chains[&id.chain].command,
                            node,
                            nodes,
                            &mut effects,
                        );
                        Self::apply_effects(role, nodes, effects, out);
                        if ready {
                            self.nodes.get_mut(&node).expect("active node exists").state =
                                OrchState::Fini;
                        } else if timer {
                            self.run_transient(role, nodes, id, node, now, out);
                        }
                    }
                    OrchState::Fini => {}
                    OrchState::None | OrchState::Idle => {
                        done = false;
                    }
                }
            }
            if done {
                self.active.remove(index);
                self.finish_stage(id, true);
            }
        }
    }

    fn apply_effects(
        role: &mut R,
        nodes: &mut R::Nodes,
        effects: Vec<RoleEffect<R::State>>,
        out: &mut R::Out,
    ) {
        for effect in effects {
            role.apply(nodes, effect, out);
        }
    }

    fn activate(&mut self, now: Instant) {
        let eligible = self.pending.iter().rposition(|id| {
            let chain = &self.chains[&id.chain];
            chain.stages[..id.stage]
                .iter()
                .all(|stage| stage.state == StageState::Done)
                && chain.stages[id.stage]
                    .candidates
                    .iter()
                    .all(|node| self.orch_state(*node) == OrchState::Idle)
        });
        if let Some(index) = eligible {
            let id = self.pending.remove(index);
            let stage = &mut self.chains.get_mut(&id.chain).expect("chain exists").stages[id.stage];
            stage.state = StageState::Active;
            stage.start = Some(now);
            for &node in stage.candidates.iter().rev() {
                self.nodes.insert(
                    node,
                    NodeStatus {
                        state: OrchState::Pending,
                        active: None,
                    },
                );
            }
            self.active.push(id);
            tracing::debug!(chain = id.chain.0, stage = id.stage, "stage activated");
        }
    }

    fn run_transient(
        &mut self,
        role: &mut R,
        nodes: &mut R::Nodes,
        id: StageId,
        node: NodeKey,
        now: Instant,
        requests: &mut R::Out,
    ) {
        let chain = &self.chains[&id.chain];
        let kind = chain.command.kind();
        let start = chain.stages[id.stage]
            .start
            .expect("active stage has a start");
        let stats = self.stats.get_mut(&kind).expect("admitted stage has stats");
        stats.elapsed = stats
            .elapsed
            .max(now.saturating_duration_since(start).as_secs());
        let mut out = TransientOut::new(kind, stats.elapsed);
        role.transient(&chain.command, node, nodes, stats.elapsed, &mut out);
        for action in out.actions {
            match action {
                TransientAction::Effect(effect) => role.apply(nodes, effect, requests),
                TransientAction::ResetStartTime => {
                    self.chains.get_mut(&id.chain).expect("chain exists").stages[id.stage].start =
                        Some(now);
                    self.stats.get_mut(&kind).expect("stats exist").elapsed = 0;
                }
                TransientAction::Cancel(kind) => self.cancel(role, nodes, kind, None, requests),
                TransientAction::CancelScoped(kind, candidates) => {
                    self.cancel(role, nodes, kind, Some(&candidates), requests);
                }
            }
        }
    }

    /// Cancels a command type only for the specified candidates.
    ///
    /// Pending stages retain other candidates; matching active candidates become
    /// `Fini`. The type's elapsed time is reset (`src/orch/em_orch.cpp:175-220`).
    pub fn cancel_command(
        &mut self,
        role: &mut R,
        nodes: &mut R::Nodes,
        kind: CommandType,
        candidates: &[NodeKey],
        out: &mut R::Out,
    ) {
        self.cancel(role, nodes, kind, Some(candidates), out);
    }

    fn cancel(
        &mut self,
        role: &mut R,
        nodes: &mut R::Nodes,
        kind: CommandType,
        candidates: Option<&[NodeKey]>,
        out: &mut R::Out,
    ) {
        tracing::debug!(?kind, "cancelling command type");
        // Upstream cancels both queues from newest to oldest (em_orch.cpp:279-301).
        for index in (0..self.pending.len()).rev() {
            let id = self.pending[index];
            if self.chains[&id.chain].command.kind() == kind {
                let stage =
                    &mut self.chains.get_mut(&id.chain).expect("chain exists").stages[id.stage];
                stage
                    .candidates
                    .retain(|node| candidates.is_some_and(|selected| !selected.contains(node)));
                if stage.candidates.is_empty() {
                    self.pending.remove(index);
                    self.finish_stage(id, false);
                }
            }
        }
        for index in (0..self.active.len()).rev() {
            let id = self.active[index];
            if self.chains[&id.chain].command.kind() == kind {
                let count = self.chains[&id.chain].stages[id.stage].candidates.len();
                for candidate in (0..count).rev() {
                    let node = self.chains[&id.chain].stages[id.stage].candidates[candidate];
                    if candidates.is_some_and(|selected| !selected.contains(&node)) {
                        continue;
                    }
                    let mut effects = Vec::new();
                    role.pre_process_cancel(&self.chains[&id.chain].command, node, &mut effects);
                    Self::apply_effects(role, nodes, effects, out);
                    self.nodes.get_mut(&node).expect("active node exists").state = OrchState::Fini;
                }
            }
        }
        if candidates.is_some() {
            if let Some(stats) = self.stats.get_mut(&kind) {
                stats.elapsed = 0;
            }
        }
    }

    /// Removes a radio from pending `em_config` stages and finishes it in active ones.
    ///
    /// A nonempty pending stage is retained. Active stages remain until all
    /// candidates finish. When an active candidate matched, the returned state
    /// effect must be applied by the caller before subsequent role work. The
    /// caller supplies its controller-unconfigured state; no role state is named
    /// here. The command type's elapsed value is reset in either case
    /// (`src/orch/em_orch.cpp:222-268`).
    ///
    /// # Panics
    ///
    /// Panics if an internal queue refers to a missing chain or active node.
    pub fn remove_chain_for_node(
        &mut self,
        node: NodeKey,
        unconfigured: R::State,
    ) -> Option<RoleEffect<R::State>> {
        for index in (0..self.pending.len()).rev() {
            let id = self.pending[index];
            if self.chains[&id.chain].command.kind() != CommandType::EmConfig {
                continue;
            }
            let stage = &mut self.chains.get_mut(&id.chain).expect("chain exists").stages[id.stage];
            stage.candidates.retain(|candidate| *candidate != node);
            if stage.candidates.is_empty() {
                self.pending.remove(index);
                self.finish_stage(id, false);
            }
        }
        let mut matched = false;
        for id in self.active.iter().rev() {
            let chain = &self.chains[&id.chain];
            if chain.command.kind() == CommandType::EmConfig
                && chain.stages[id.stage].candidates.contains(&node)
            {
                self.nodes.get_mut(&node).expect("active node exists").state = OrchState::Fini;
                matched = true;
            }
        }
        if let Some(stats) = self.stats.get_mut(&CommandType::EmConfig) {
            stats.elapsed = 0;
        }
        matched.then_some(RoleEffect::SetState(node, unconfigured))
    }

    fn finish_stage(&mut self, id: StageId, active: bool) {
        let chain = self.chains.get_mut(&id.chain).expect("chain exists");
        let stage = &mut chain.stages[id.stage];
        if active {
            for node in &stage.candidates {
                self.nodes.remove(node);
            }
        }
        stage.state = StageState::Done;
        let kind = chain.command.kind();
        let stats = self.stats.get_mut(&kind).expect("admitted stage has stats");
        stats.count -= 1;
        if stats.count == 0 {
            self.stats.remove(&kind);
        }
        if chain
            .stages
            .iter()
            .all(|stage| stage.state == StageState::Done)
        {
            self.chains.remove(&id.chain);
            tracing::debug!(chain = id.chain.0, "command destroyed");
        }
    }
}

impl<R: OrchRole> CommandView for Orchestrator<R> {
    fn command(&self, chain: ChainId) -> Option<&Command> {
        self.chains.get(&chain).map(|chain| &chain.command)
    }

    fn orch_state(&self, node: NodeKey) -> OrchState {
        // Upstream initializes each node idle (src/em/em.cpp:3221-3229).
        self.nodes
            .get(&node)
            .map_or(OrchState::Idle, |node| node.state)
    }

    fn active(&self, node: NodeKey) -> Option<ActiveOp> {
        self.nodes.get(&node).and_then(|node| node.active)
    }
}

#[cfg(test)]
mod tests;
