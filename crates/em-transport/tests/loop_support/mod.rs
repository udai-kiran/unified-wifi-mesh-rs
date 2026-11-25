// SPDX-License-Identifier: Apache-2.0

//! A test role for the transport on the event loop (0016 D2, D3b): the loop `App` that owns a
//! `TransportHandler`, and `start_loop`, which runs `em_event_loop::run` in a tokio task.
//!
//! The role has the same four ends as the old actor's `Transport` (`sdus`, `received`, `link`,
//! `counters`) and a `task` that ends with the actor's result. Dropping `sdus` is the stop signal.
//! The loop's sources, in the order of 0016 D3b: the data-socket read (64 frames), the
//! registration verdict, the stop signal, the application channel, the received queue to the
//! role, the data-socket write.

use std::future::Future;
use std::time::{Duration, Instant};

use em_event_loop::{App, HandlerId, LoopCore, SourceId, Turn, run};
use em_transport::{
    IoAction, IoEvent, LinkState, OutboundSdu, Role, Sdu, TransportConfig, TransportCounters,
    TransportError, TransportHandler, TransportIo, TransportPolicy, TransportSources,
    TransportTask,
};
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::{mpsc, watch};

use crate::support::{FakeService, bounded};

const READ: SourceId = SourceId(0);
const REGISTRATION: SourceId = SourceId(1);
const STOP: SourceId = SourceId(2);
const APP: SourceId = SourceId(3);
const RECEIVED: SourceId = SourceId(4);
const WRITE: SourceId = SourceId(5);
const TIMER: HandlerId = HandlerId(0);

/// Source budgets, in the order of the ids above.
const BUDGETS: [usize; 6] = [64, 1, 1, 1, 1, 1];

/// Room in the channel from the test to the role.
const APP_CHANNEL_CAPACITY: usize = 64;

/// Room in the channel from the role to the test, unless a test asks for another.
pub const RECEIVED_CAPACITY: usize = 64;

/// How long `drain_received` waits for one more SDU, in virtual time under `start_paused`.
const DRAIN_IDLE: Duration = Duration::from_millis(50);

/// What the test sends to the role over the application channel.
#[derive(Debug, PartialEq, Eq)]
pub enum AppInput {
    /// An SDU for the service.
    Send(OutboundSdu),
    /// Nothing to send: it only makes the loop run a pass. The role forwards a received SDU only
    /// while the channel to the test has room, and nothing else tells the loop that the test has
    /// made room.
    Wake,
}

/// The test's end of the application channel. Dropping it is the stop signal.
#[derive(Debug)]
pub struct SduSender(mpsc::Sender<AppInput>);

impl SduSender {
    /// Sends `outbound`, waiting for room. Panics if the loop has ended.
    pub async fn send(&self, outbound: OutboundSdu) {
        let sent = self.0.send(AppInput::Send(outbound)).await;
        assert!(sent.is_ok(), "the loop has ended, the SDU was not sent");
    }

    /// Sends `outbound` if there is room at once.
    pub fn try_send(&self, outbound: OutboundSdu) -> Result<(), TrySendError<AppInput>> {
        self.0.try_send(AppInput::Send(outbound))
    }

    /// True once the loop has ended.
    pub fn is_closed(&self) -> bool {
        self.0.is_closed()
    }

    /// Makes the loop run a pass. Not needed (and not sent) while the channel is full: the loop
    /// then has queued input to serve anyway.
    fn wake(&self) {
        let _ = self.try_wake();
    }

    /// Sends `AppInput::Wake` if there is room at once. It keeps the channel busy without
    /// adding an SDU to the outbound buffer.
    pub fn try_wake(&self) -> Result<(), TrySendError<AppInput>> {
        self.0.try_send(AppInput::Wake)
    }
}

/// The ends of a started role.
pub struct LoopClient {
    /// SDUs to send. Dropping it is the stop signal.
    pub sdus: SduSender,
    /// SDUs received from the service. Read through `next_received` and `drain_received`, which
    /// wake the loop after a read; a test that reads this end by itself must do the same.
    pub received: mpsc::Receiver<Sdu>,
    /// The latest link state, published after every turn.
    pub link: watch::Receiver<LinkState>,
    /// The latest counters, published after every turn.
    pub counters: watch::Receiver<TransportCounters>,
    /// The loop's task: ends with the actor's result.
    pub task: TransportTask,
}

impl LoopClient {
    /// Waits for the next received SDU, then wakes the loop so that it forwards the next one.
    pub async fn next_received(&mut self) -> Sdu {
        let sdu = bounded("a received SDU", self.received.recv())
            .await
            .expect("the received channel closed");
        self.sdus.wake();
        sdu
    }

    /// Takes every SDU the loop forwards until none comes for a while: the SDUs in the channel
    /// and then, one per pass, those in the received queue.
    pub async fn drain_received(&mut self) -> Vec<Sdu> {
        let mut all = Vec::new();
        while let Ok(Some(sdu)) = tokio::time::timeout(DRAIN_IDLE, self.received.recv()).await {
            self.sdus.wake();
            all.push(sdu);
        }
        all
    }
}

/// The role: owns the handler and the test's ends of the channels.
struct TestApp {
    handler: TransportHandler,
    /// The input the `Io` received from the application channel, until the application source runs.
    arrived: Option<AppInput>,
    received: mpsc::Sender<Sdu>,
    link: watch::Sender<LinkState>,
    counters: watch::Sender<TransportCounters>,
}

impl TestApp {
    /// Refreshes what depends on the handler after an event or a turn: the received source, and
    /// the published link and counters.
    ///
    /// The received source is ready only while the channel to the test has room. Nothing wakes
    /// the loop when the test makes room, so the test sends `AppInput::Wake` after it reads
    /// (`LoopClient::next_received` does).
    fn refresh(&self, core: &mut LoopCore<Instant>) {
        let can_forward = !self.received.is_closed() && 0 < self.received.capacity();
        core.set_ready(RECEIVED, self.handler.received_ready() && can_forward);

        let link = self.handler.link();
        self.link.send_if_modified(|published| {
            let changed = *published != link;
            *published = link;
            changed
        });
        let counters = self.handler.counters();
        self.counters.send_if_modified(|published| {
            let changed = *published != counters;
            *published = counters;
            changed
        });
    }
}

impl App<Instant> for TestApp {
    type Event = IoEvent<AppInput>;
    type Action = IoAction;

    fn on_event(&mut self, event: Self::Event, core: &mut LoopCore<Instant>) {
        match event {
            IoEvent::Transport(event) => self.handler.on_event(event, core),
            IoEvent::App(outbound) => {
                self.arrived = Some(outbound);
                core.set_ready(APP, true);
            }
            IoEvent::AppClosed => core.set_ready(STOP, true),
        }
        self.refresh(core);
    }

    fn serve(
        &mut self,
        turn: Turn,
        now: Instant,
        core: &mut LoopCore<Instant>,
        actions: &mut Vec<IoAction>,
    ) {
        if !self.handler.serve(turn, now, core, actions) {
            match turn {
                Turn::Serve(STOP) => self.handler.stop(now, core, actions),
                Turn::Serve(APP) => {
                    if let Some(AppInput::Send(outbound)) = self.arrived.take() {
                        self.handler.send(now, outbound, core);
                    }
                    // The next receive is armed only now, so the application channel feeds the
                    // loop one SDU per turn.
                    actions.push(IoAction::RecvApp);
                }
                Turn::Serve(RECEIVED) => {
                    if let Ok(permit) = self.received.try_reserve() {
                        if let Some(sdu) = self.handler.take_received() {
                            permit.send(sdu);
                        }
                    }
                }
                Turn::Serve(_) | Turn::Deadline(_) | Turn::PassDone => {}
            }
        }
        self.refresh(core);
    }

    fn finished(&self) -> bool {
        self.handler.outcome().is_some()
    }
}

/// Starts the transport on the loop, in a task of the current runtime.
pub fn start_loop(service: &FakeService, role: Role) -> LoopClient {
    start_loop_with_config(&service.config(role))
}

/// Like `start_loop`, with a channel of `received_capacity` SDUs from the role to the test. A
/// test that never reads it and fills it with one SDU keeps every later SDU in the received
/// queue, so what the queue keeps does not depend on when the loop forwards.
pub fn start_loop_with_received_capacity(
    service: &FakeService,
    role: Role,
    received_capacity: usize,
) -> LoopClient {
    start_loop_with_capacity(&service.config(role), received_capacity)
}

/// Like `start_loop`, for a config the test built.
pub fn start_loop_with_config(config: &TransportConfig) -> LoopClient {
    start_loop_with_capacity(config, RECEIVED_CAPACITY)
}

/// What the test holds of a loop that has not been started yet.
struct LoopEnds {
    sdus: SduSender,
    received: mpsc::Receiver<Sdu>,
    link: watch::Receiver<LinkState>,
    counters: watch::Receiver<TransportCounters>,
}

impl LoopEnds {
    fn with_task(self, task: TransportTask) -> LoopClient {
        LoopClient {
            sdus: self.sdus,
            received: self.received,
            link: self.link,
            counters: self.counters,
            task,
        }
    }
}

/// Builds the loop as a future that ends with the transport's result, and the test's ends.
fn build_loop(
    config: &TransportConfig,
    received_capacity: usize,
) -> (
    impl Future<Output = Result<(), TransportError>> + Send + use<>,
    LoopEnds,
) {
    let mut policy = TransportPolicy::easymesh();
    policy.reconnect_delay = config.reconnect_delay;

    let (sdus, app_events) = mpsc::channel(APP_CHANNEL_CAPACITY);
    let (received_sender, received) = mpsc::channel(received_capacity);
    let (link_sender, link) = watch::channel(LinkState::default());
    let (counters_sender, counters) = watch::channel(TransportCounters::default());

    let mut io = TransportIo::new(config, app_events);
    let mut core = LoopCore::new(&BUDGETS, 1);
    let sources = TransportSources {
        read: READ,
        registration: REGISTRATION,
        write: WRITE,
        timer: TIMER,
    };
    let now = tokio::time::Instant::now().into_std();
    let handler = TransportHandler::new(policy, now, sources, &mut core);
    let mut app = TestApp {
        handler,
        arrived: None,
        received: received_sender,
        link: link_sender,
        counters: counters_sender,
    };
    // The first turn of the application source arms the first receive.
    core.set_ready(APP, true);

    let future = async move {
        run(&mut core, &mut io, &mut app).await;
        let outcome = app
            .handler
            .outcome()
            .expect("the loop ends only when the transport has finished");
        io.into_result(outcome)
    };
    let ends = LoopEnds {
        sdus: SduSender(sdus),
        received,
        link,
        counters,
    };
    (future, ends)
}

fn start_loop_with_capacity(config: &TransportConfig, received_capacity: usize) -> LoopClient {
    let (future, ends) = build_loop(config, received_capacity);
    ends.with_task(tokio::spawn(future))
}

/// Like `start_loop`, with the loop on a runtime of its own thread. The loop awaits only when no
/// source is ready, so while input keeps arriving it keeps its thread; on a thread of its own it
/// cannot keep the test's tasks from running, and the test's I/O driver is turned by the test's
/// runtime. `task` ends when the thread has.
pub fn start_loop_on_its_own_thread(service: &FakeService, role: Role) -> LoopClient {
    let (future, ends) = build_loop(&service.config(role), RECEIVED_CAPACITY);
    let thread = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a runtime for the loop thread");
        runtime.block_on(future)
    });
    let task =
        tokio::task::spawn_blocking(move || thread.join().expect("the loop thread panicked"));
    ends.with_task(task)
}
