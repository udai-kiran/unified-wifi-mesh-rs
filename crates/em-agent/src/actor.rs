// SPDX-License-Identifier: Apache-2.0

//! The tokio role actor of the agent (0012 D9): moves inputs into [`AgentCore`]
//! and its frames out to `em-transport`. It also passes the platform's confirmations
//! in and the configurations of accepted M2s out to the [`Platform`]. All protocol
//! logic is in the core.

use std::fmt;
use std::future::{self, Future};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use em_transport::{
    LinkState, OutboundSdu, Role, Sdu, Transport, TransportConfig, TransportError, TransportTask,
    spawn,
};
use em_types::MacAddr;
use tokio::sync::mpsc::Permit;
use tokio::sync::watch;
use tokio::task::JoinError;

use crate::pending::PendingFrames;
use crate::platform::{Confirmation, Platform};
use crate::{AgentCore, AlMacChanged, Frame};

/// How long to wait for the transport task after a stop, before aborting it (0011 A7).
const TRANSPORT_STOP_WAIT: Duration = Duration::from_secs(1);

/// What the agent actor needs besides the platform.
#[derive(Debug)]
pub struct AgentConfig {
    /// AL-SAP control socket path.
    pub control_path: PathBuf,
    /// AL-SAP data socket path.
    pub data_path: PathBuf,
    /// The Multi-AP profile to advertise, 2 or 3 (0012 D4).
    pub profile: u8,
}

/// Why the actor ended other than by a stop request. The process exits 1.
#[derive(Debug)]
pub enum ActorError {
    /// A registration returned a different AL MAC (0012 D9).
    AlMacChanged(AlMacChanged),
    /// The transport task ended with an error, for example a rejected registration.
    Transport(TransportError),
    /// The transport task ended, or its channels closed, without an error.
    TransportStopped,
    /// The transport task panicked or was cancelled.
    TransportFailed(JoinError),
}

impl fmt::Display for ActorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlMacChanged(error) => write!(f, "{error}"),
            Self::Transport(error) => write!(f, "transport ended: {error}"),
            Self::TransportStopped => write!(f, "transport ended without an error"),
            Self::TransportFailed(error) => write!(f, "transport task failed: {error}"),
        }
    }
}

impl std::error::Error for ActorError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::AlMacChanged(error) => Some(error),
            Self::Transport(error) => Some(error),
            Self::TransportStopped => None,
            Self::TransportFailed(error) => Some(error),
        }
    }
}

fn now() -> Instant {
    tokio::time::Instant::now().into_std()
}

/// The actor's state outside the transport's channels.
struct Actor<P: Platform> {
    core: AgentCore,
    platform: P,
    link: watch::Receiver<LinkState>,
    /// The link state the core has been told about.
    seen: LinkState,
    /// Recorded at the first registration (0012 D9).
    al_mac: Option<MacAddr>,
    pending: PendingFrames,
}

impl<P: Platform> Actor<P> {
    /// Tells the core about a link change. Called before every handler, because
    /// the `link` arm of the `select!` can lose the race against a received SDU
    /// of the new session (0011 A5).
    ///
    /// Returns the link state it copied. The caller stamps its frames with that
    /// same value and does not read the link again, so the core and the stamp
    /// agree on one session (0012 D9).
    fn sync_link(&mut self) -> Result<LinkState, ActorError> {
        let current = *self.link.borrow_and_update();
        if current == self.seen {
            return Ok(current);
        }
        self.seen = current;
        if let Some(al_mac) = current.al_mac {
            self.core
                .registered(al_mac, now())
                .map_err(ActorError::AlMacChanged)?;
            if self.al_mac.is_none() {
                tracing::info!(session = current.session, %al_mac, "registered");
            } else {
                tracing::info!(session = current.session, %al_mac, "new session, same AL MAC");
            }
            self.al_mac = Some(al_mac);
        } else {
            self.core.unregistered();
            tracing::info!(session = current.session, "link lost, waiting to register");
        }
        Ok(current)
    }

    fn handle_received(&mut self, sdu: &Sdu) -> Result<(), ActorError> {
        let session = self.sync_link()?.session;
        let frames = self.core.received(sdu.remote, &sdu.cmdu, now());
        self.queue(session, frames);
        self.apply_platform_requests();
        Ok(())
    }

    /// The platform confirmed every radio's configuration (0013 D4).
    fn handle_confirmation(&mut self, confirmation: Confirmation) -> Result<(), ActorError> {
        self.sync_link()?;
        self.core.platform_confirmed(confirmation.bss, now());
        Ok(())
    }

    /// Hands the configurations of accepted M2s to the platform.
    fn apply_platform_requests(&mut self) {
        for request in self.core.take_platform_requests() {
            self.platform
                .apply_radio_config(request.radio, &request.bss);
        }
    }

    fn handle_deadline(&mut self) -> Result<(), ActorError> {
        let session = self.sync_link()?.session;
        let frames = self.core.tick(now());
        self.queue(session, frames);
        Ok(())
    }

    /// Stamps each frame with the session of the synced link state and the
    /// recorded AL MAC (0012 D9). A frame of a dead session is dropped and
    /// counted by the transport.
    fn queue(&mut self, session: u64, frames: Vec<Frame>) {
        if frames.is_empty() {
            return;
        }
        let Some(local) = self.al_mac else {
            tracing::warn!("frames built before the first registration were dropped");
            return;
        };
        for frame in frames {
            self.pending.push(OutboundSdu {
                session,
                sdu: Sdu {
                    local,
                    remote: frame.remote,
                    cmdu: frame.cmdu,
                },
            });
        }
    }

    fn send_next(&mut self, permit: Permit<'_, OutboundSdu>) {
        if let Some(sdu) = self.pending.pop() {
            permit.send(sdu);
        }
    }
}

async fn sleep_until_deadline(deadline: Option<tokio::time::Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => future::pending().await,
    }
}

fn transport_result(result: Result<Result<(), TransportError>, JoinError>) -> ActorError {
    match result {
        Ok(Ok(())) => ActorError::TransportStopped,
        Ok(Err(error)) => ActorError::Transport(error),
        Err(error) => ActorError::TransportFailed(error),
    }
}

/// Finds out why the transport ended: its channels closed, so its task is
/// finishing. Waits for it at most 1 s, then aborts it.
async fn transport_ended(task: &mut TransportTask) -> ActorError {
    let Ok(result) = tokio::time::timeout(TRANSPORT_STOP_WAIT, &mut *task).await else {
        task.abort();
        return ActorError::TransportStopped;
    };
    transport_result(result)
}

/// Runs the agent until `stop` completes (returns `Ok`) or the transport ends
/// or the AL MAC changes (returns the error; the process exits 1).
///
/// A stop drops the transport's senders and receivers, waits at most 1 s for its
/// task and then aborts it (0011 A7, 0012 D9).
///
/// # Errors
///
/// [`ActorError`] when the actor ends without a stop request.
pub async fn run(
    config: AgentConfig,
    mut platform: impl Platform,
    stop: impl Future<Output = ()>,
) -> Result<(), ActorError> {
    let Transport {
        sdus,
        mut received,
        link,
        counters,
        mut task,
    } = spawn(TransportConfig::new(
        config.control_path,
        config.data_path,
        Role::Agent,
    ));
    let mut confirmations = platform.confirmations();
    let mut actor = Actor {
        core: AgentCore::new(config.profile, platform.radios()),
        platform,
        link,
        seen: LinkState::default(),
        al_mac: None,
        pending: PendingFrames::default(),
    };
    tokio::pin!(stop);

    loop {
        let deadline = actor
            .core
            .next_deadline()
            .map(tokio::time::Instant::from_std);
        let has_pending = !actor.pending.is_empty();
        tokio::select! {
            () = &mut stop => break,
            result = &mut task => return Err(transport_result(result)),
            sdu = received.recv() => match sdu {
                Some(sdu) => actor.handle_received(&sdu)?,
                None => return Err(transport_ended(&mut task).await),
            },
            changed = actor.link.changed() => match changed {
                Ok(()) => {
                    actor.sync_link()?;
                }
                Err(_) => return Err(transport_ended(&mut task).await),
            },
            Some(confirmation) = confirmations.recv() => actor.handle_confirmation(confirmation)?,
            () = sleep_until_deadline(deadline) => actor.handle_deadline()?,
            permit = sdus.reserve(), if has_pending => match permit {
                Ok(permit) => actor.send_next(permit),
                Err(_) => return Err(transport_ended(&mut task).await),
            },
        }
    }

    // The permit future of the last pass borrowed `sdus`, so the channels are
    // dropped here and not in the `select!` arm.
    drop(actor);
    drop(received);
    drop(sdus);
    drop(counters);
    stop_transport(&mut task).await;
    Ok(())
}

/// Waits for the transport task after its channels were dropped, then aborts it.
async fn stop_transport(task: &mut TransportTask) {
    match tokio::time::timeout(TRANSPORT_STOP_WAIT, &mut *task).await {
        Ok(Ok(Ok(()))) => tracing::info!("transport stopped"),
        Ok(Ok(Err(error))) => {
            tracing::warn!(%error, "transport ended with an error while stopping");
        }
        Ok(Err(error)) => tracing::warn!(%error, "transport task failed while stopping"),
        Err(_) => {
            tracing::warn!("transport did not stop within 1 s, aborting it");
            task.abort();
            // An aborted task ends at once with a cancelled error.
            let _ = task.await;
        }
    }
}

#[cfg(test)]
mod tests {
    use em_cmd::RadioSpec;
    use em_transport::LinkState;
    use em_types::{FreqBand, MacAddr};
    use tokio::sync::watch;

    use super::Actor;
    use crate::pending::PendingFrames;
    use crate::platform::FakePlatform;
    use crate::{AgentCore, PROTO_IDLE};

    const AL_MAC: MacAddr = MacAddr::new([2, 0, 0, 0, 0, 2]);

    fn registered_link(session: u64) -> LinkState {
        LinkState {
            session,
            al_mac: Some(AL_MAC),
        }
    }

    fn actor_with(link: watch::Receiver<LinkState>) -> Actor<FakePlatform> {
        let radios = vec![RadioSpec {
            ruid: MacAddr::new([2, 0, 0, 0, 1, 1]),
            band: FreqBand::BAND_5_GHZ,
        }];
        Actor {
            core: AgentCore::new(2, Vec::from(radios.as_slice())),
            platform: FakePlatform::new(radios),
            link,
            seen: LinkState::default(),
            al_mac: None,
            pending: PendingFrames::default(),
        }
    }

    #[test]
    fn sync_link_returns_the_state_it_copied() {
        let (tx, rx) = watch::channel(LinkState::default());
        let mut actor = actor_with(rx);
        tx.send(registered_link(4)).unwrap();
        assert_eq!(actor.sync_link().unwrap(), registered_link(4));
        // Without a change it returns the same value again.
        assert_eq!(actor.sync_link().unwrap(), registered_link(4));
    }

    /// The frames of a handler carry the session of the state that `sync_link`
    /// copied and registered with the core. A second read of the link could differ
    /// from the first, so the frames would carry a session the core was never told
    /// about. That race cannot be built deterministically; this test checks that the
    /// stamp comes from the synced value.
    #[tokio::test(start_paused = true)]
    async fn frames_are_stamped_with_the_synced_session() {
        let (tx, rx) = watch::channel(LinkState::default());
        let mut actor = actor_with(rx);
        tx.send(registered_link(7)).unwrap();
        // Registering submits dev_init; its radio node sends a Search when the
        // protocol timer runs out.
        actor.sync_link().unwrap();
        tokio::time::advance(PROTO_IDLE).await;
        actor.handle_deadline().unwrap();
        let stamped = actor.pending.pop().expect("a frame was queued");
        assert_eq!(stamped.session, 7);
        assert_eq!(stamped.sdu.local, AL_MAC);
    }
}
