// SPDX-License-Identifier: Apache-2.0

//! A scripted fake AL-SAP service over UNIX sockets in a temporary directory.
//!
//! A trimmed copy of `em-transport`'s `tests/support` (0011 D6): that module is not
//! public, and the role crates need only registration and the data socket.

use std::future::Future;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use em_types::MacAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};

/// Registration result "success" (0011, "The AL-SAP protocol").
pub const SUCCESS: u8 = 0x01;
/// Registration result "not supported": the service rejects the client.
pub const NOT_SUPPORTED: u8 = 0x02;

/// How long a test waits for anything, in virtual time under `start_paused`.
const LIMIT: Duration = Duration::from_secs(60);

/// Waits for `future`, failing the test with `what` if it takes longer than [`LIMIT`],
/// so that a broken actor fails a test instead of hanging it.
pub async fn bounded<T>(what: &str, future: impl Future<Output = T>) -> T {
    tokio::time::timeout(LIMIT, future)
        .await
        .unwrap_or_else(|_| panic!("timed out waiting for: {what}"))
}

/// A temporary directory that is removed on drop. Std only, no `tempfile`.
pub struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let name = format!(
            "em-role-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let path = std::env::temp_dir().join(name);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Two UNIX listeners in a temporary directory, standing in for the service.
pub struct FakeService {
    dir: TempDir,
    control: UnixListener,
    data: UnixListener,
}

impl FakeService {
    pub fn new() -> Self {
        let dir = TempDir::new();
        let control = UnixListener::bind(dir.0.join("control")).unwrap();
        let data = UnixListener::bind(dir.0.join("data")).unwrap();
        Self { dir, control, data }
    }

    pub fn control_path(&self) -> PathBuf {
        self.dir.0.join("control")
    }

    pub fn data_path(&self) -> PathBuf {
        self.dir.0.join("data")
    }

    /// Accepts the client's control and data connections, in the client's order,
    /// and reads its registration request.
    pub async fn accept(&self) -> ServiceSession {
        let (mut control, _) = bounded("control connection", self.control.accept())
            .await
            .unwrap();
        let (data, _) = bounded("data connection", self.data.accept())
            .await
            .unwrap();
        // The request content is not checked here; `em-transport` tests cover it.
        read_frame(&mut control).await;
        ServiceSession { control, data }
    }

    /// Accepts a client and answers its registration with `al_mac` and `result`.
    pub async fn accept_and_respond(&self, al_mac: MacAddr, result: u8) -> ServiceSession {
        let mut session = self.accept().await;
        session.respond(al_mac, result).await;
        session
    }
}

/// One accepted client session.
pub struct ServiceSession {
    control: UnixStream,
    data: UnixStream,
}

impl ServiceSession {
    async fn respond(&mut self, al_mac: MacAddr, result: u8) {
        let mut frame = vec![0, 0, 0, 7];
        frame.extend_from_slice(&al_mac.octets());
        frame.push(result);
        bounded("control write", self.control.write_all(&frame))
            .await
            .unwrap();
    }

    /// Reads one SDU the client sent.
    pub async fn read_sdu(&mut self) -> WireSdu {
        let frame = read_frame(&mut self.data).await;
        WireSdu {
            local: MacAddr::new(frame[4..10].try_into().unwrap()),
            remote: MacAddr::new(frame[10..16].try_into().unwrap()),
            cmdu: frame[19..].to_vec(),
        }
    }

    /// Delivers an unfragmented SDU to the client: MAC A is the client's AL MAC,
    /// MAC B the sender.
    pub async fn write_sdu(&mut self, local: MacAddr, remote: MacAddr, cmdu: &[u8]) {
        let length = u32::try_from(15 + cmdu.len()).unwrap();
        let mut frame = length.to_be_bytes().to_vec();
        frame.extend_from_slice(&local.octets());
        frame.extend_from_slice(&remote.octets());
        frame.extend_from_slice(&[0, 1, 0]);
        frame.extend_from_slice(cmdu);
        bounded("data write", self.data.write_all(&frame))
            .await
            .unwrap();
    }
}

/// An SDU as the service reads it from the data socket.
pub struct WireSdu {
    pub local: MacAddr,
    pub remote: MacAddr,
    pub cmdu: Vec<u8>,
}

/// Reads one frame, with its length prefix.
async fn read_frame(stream: &mut UnixStream) -> Vec<u8> {
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
