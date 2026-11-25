// SPDX-License-Identifier: Apache-2.0

//! A scripted fake AL-SAP service for the integration tests (0011 D6).

use std::future::Future;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc as std_mpsc;
use std::thread;
use std::time::Duration;

use em_transport::{Role, Sdu, TransportConfig, TransportError};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

/// The reconnect delay the tests configure.
pub const RECONNECT_DELAY: Duration = Duration::from_secs(2);

/// How long a test waits for anything, in virtual time under `start_paused`.
const LIMIT: Duration = Duration::from_secs(60);

/// Waits for `future`, failing the test with `what` if it takes longer than [`LIMIT`].
/// A broken loop then fails a test with a message instead of hanging it.
pub async fn bounded<T>(what: &str, future: impl Future<Output = T>) -> T {
    // The timeout cannot fire if a broken loop spins without yielding. The watchdog
    // then ends the process with a message after a long real time.
    let _watchdog = Watchdog::start(what.to_owned(), Duration::from_secs(20));
    tokio::time::timeout(LIMIT, future)
        .await
        .unwrap_or_else(|_| panic!("timed out waiting for: {what}"))
}

/// Waits for the loop's task to end within a real-time `limit` and returns its result.
pub async fn join_within(
    limit: Duration,
    task: JoinHandle<Result<(), TransportError>>,
) -> Result<(), TransportError> {
    // A real-time timeout cannot fire while the loop spins without yielding.
    let _watchdog = Watchdog::start("the loop to end".to_owned(), limit + Duration::from_secs(5));
    tokio::time::timeout(limit, task)
        .await
        .unwrap_or_else(|_| panic!("the loop did not end within {limit:?}"))
        .expect("the loop task panicked")
}

/// Aborts the whole test process with a message if it is not dropped in time.
///
/// For tests where a broken loop can spin without ever yielding, which no timeout
/// on the same thread can interrupt.
pub struct Watchdog {
    disarm: std_mpsc::Sender<()>,
}

impl Watchdog {
    pub fn start(what: String, limit: Duration) -> Self {
        let (disarm, disarmed) = std_mpsc::channel::<()>();
        thread::spawn(move || {
            // A disarm message and a dropped sender both mean the test finished in time.
            if disarmed.recv_timeout(limit) == Err(std_mpsc::RecvTimeoutError::Timeout) {
                eprintln!("watchdog: {what} did not finish within {limit:?}, aborting");
                std::process::abort();
            }
        });
        Self { disarm }
    }
}

impl Drop for Watchdog {
    fn drop(&mut self) {
        let _ = self.disarm.send(());
    }
}

/// A temporary directory that is removed on drop. Std only, no `tempfile`.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new() -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let name = format!(
            "em-transport-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let path = std::env::temp_dir().join(name);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    pub fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Two UNIX listeners in a temporary directory, standing in for the service.
pub struct FakeService {
    pub dir: TempDir,
    pub control: UnixListener,
    pub data: UnixListener,
}

impl FakeService {
    pub fn new() -> Self {
        let dir = TempDir::new();
        let control = UnixListener::bind(dir.path().join("control")).unwrap();
        let data = UnixListener::bind(dir.path().join("data")).unwrap();
        Self { dir, control, data }
    }

    pub fn control_path(&self) -> PathBuf {
        self.dir.path().join("control")
    }

    pub fn data_path(&self) -> PathBuf {
        self.dir.path().join("data")
    }

    pub fn config(&self, role: Role) -> TransportConfig {
        let mut config = TransportConfig::new(self.control_path(), self.data_path(), role);
        config.reconnect_delay = RECONNECT_DELAY;
        config
    }

    /// Accepts the client's control and data connections, in the client's order,
    /// and reads its registration request.
    pub async fn accept(&self) -> ServiceSession {
        let (mut control, data) = self.accept_without_reading().await;
        let request = read_frame(&mut control).await;
        ServiceSession {
            control,
            data,
            request,
        }
    }

    /// Accepts the two connections and leaves the registration request unread.
    pub async fn accept_without_reading(&self) -> (UnixStream, UnixStream) {
        let (control, _) = bounded("control connection", self.control.accept())
            .await
            .unwrap();
        let (data, _) = bounded("data connection", self.data.accept())
            .await
            .unwrap();
        (control, data)
    }
}

/// One accepted client session.
pub struct ServiceSession {
    pub control: UnixStream,
    pub data: UnixStream,
    /// The registration request, with its length prefix.
    pub request: Vec<u8>,
}

impl ServiceSession {
    /// Sends a registration response frame.
    pub async fn respond(&mut self, al_mac: [u8; 6], result: u8) {
        let mut frame = vec![0, 0, 0, 7];
        frame.extend_from_slice(&al_mac);
        frame.push(result);
        self.write_control(&frame).await;
    }

    pub async fn write_control(&mut self, bytes: &[u8]) {
        bounded("control write", self.control.write_all(bytes))
            .await
            .unwrap();
    }

    /// Reads one frame from the data socket, with its length prefix.
    pub async fn read_data_frame(&mut self) -> Vec<u8> {
        read_frame(&mut self.data).await
    }

    pub async fn write_data(&mut self, bytes: &[u8]) {
        bounded("data write", self.data.write_all(bytes))
            .await
            .unwrap();
    }
}

/// Reads one frame, with its length prefix.
pub async fn read_frame(stream: &mut UnixStream) -> Vec<u8> {
    let mut prefix = [0u8; 4];
    bounded("frame prefix", stream.read_exact(&mut prefix))
        .await
        .unwrap();
    let length = u32::from_be_bytes(prefix) as usize;
    let mut frame = prefix.to_vec();
    frame.resize(4 + length, 0);
    bounded("frame body", stream.read_exact(&mut frame[4..]))
        .await
        .unwrap();
    frame
}

/// Reads one octet and returns the count: 0 is end of file.
pub async fn read_one(stream: &mut UnixStream) -> usize {
    let mut buffer = [0u8; 1];
    bounded("a read", stream.read(&mut buffer)).await.unwrap()
}

/// An SDU frame written out by hand from the field order in `ieee1905-rs`'s
/// `sdu_codec.rs`: prefix, MAC A, MAC B, `is_fragment`, `is_last_fragment`,
/// `fragment_id`, payload.
pub fn sdu_frame(mac_a: [u8; 6], mac_b: [u8; 6], flags: [u8; 3], payload: &[u8]) -> Vec<u8> {
    let length = u32::try_from(15 + payload.len()).unwrap();
    let mut frame = length.to_be_bytes().to_vec();
    frame.extend_from_slice(&mac_a);
    frame.extend_from_slice(&mac_b);
    frame.extend_from_slice(&flags);
    frame.extend_from_slice(payload);
    frame
}

/// Waits for the next received SDU, failing the test after a long (virtual) time.
pub async fn next_received(received: &mut mpsc::Receiver<Sdu>) -> Sdu {
    bounded("a received SDU", received.recv())
        .await
        .expect("received channel closed")
}
