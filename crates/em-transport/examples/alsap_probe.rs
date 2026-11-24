// SPDX-License-Identifier: Apache-2.0

//! Test probe for the AL-SAP client (note 0011 D5). Test tooling, not a product binary.
//!
//! ```text
//! alsap_probe --role agent|controller --control <path> --data <path> --fifo <path>
//! ```
//!
//! Registers, then stays connected. Every event is one line on stdout, flushed. The
//! `REGISTERED` and `DISCONNECTED` lines come from the transport's link state, a latest
//! value (note 0011 A5): leaving a registered state, to no AL MAC or to a new session,
//! prints `DISCONNECTED`; reaching a new session with an AL MAC prints `REGISTERED`.
//! Commands use the current link state, not the printed one, so each SDU carries the
//! session it was built for (A6).
//!
//! ```text
//! REGISTERED <al_mac>
//! RECEIVED local=<mac> remote=<mac> type=0x<4 hex> cmdu=<hex>
//! DISCONNECTED
//! SENT <remote> type=0x<4 hex> mid=<n>
//! ```
//!
//! Commands come from the FIFO, one per line (the probe creates it if missing and opens it
//! again at end of file):
//!
//! ```text
//! send <remote_mac> ap-cap-query   unicast AP Capability Query (0x8001)
//! send multicast search            AP-Autoconfiguration Search to 01:80:c2:00:00:13
//! ```

use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use em_msg::bodies::{
    AlMacAddress, AutoconfigFreqBand, FrequencyBand, Role as SearchedRoleCode, RoleValue,
    SearchedRole,
};
use em_msg::{AL_SAP_BUDGET, BuildError, CmduHeader, MsgBuilder, default_relay};
use em_transport::{LinkState, OutboundSdu, Role, Sdu, Transport, TransportConfig, spawn};
use em_types::{FreqBand, MacAddr, MsgType};
use tokio::sync::mpsc;

struct Args {
    role: Role,
    control: PathBuf,
    data: PathBuf,
    fifo: PathBuf,
}

fn parse_args() -> Result<Args, String> {
    let mut role = None;
    let mut control = None;
    let mut data = None;
    let mut fifo = None;
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        let value = args.next().ok_or_else(|| format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--role" => {
                role = Some(match value.as_str() {
                    "agent" => Role::Agent,
                    "controller" => Role::Controller,
                    other => return Err(format!("unknown role {other:?}")),
                });
            }
            "--control" => control = Some(PathBuf::from(value)),
            "--data" => data = Some(PathBuf::from(value)),
            "--fifo" => fifo = Some(PathBuf::from(value)),
            other => return Err(format!("unknown option {other:?}")),
        }
    }
    Ok(Args {
        role: role.ok_or("--role is required")?,
        control: control.ok_or("--control is required")?,
        data: data.ok_or("--data is required")?,
        fifo: fifo.ok_or("--fifo is required")?,
    })
}

/// Prints one line and flushes, so that a script reading a pipe sees it at once.
fn emit(line: &str) {
    let mut stdout = std::io::stdout().lock();
    // A closed stdout means nobody is watching; the probe has nothing left to do.
    if writeln!(stdout, "{line}")
        .and_then(|()| stdout.flush())
        .is_err()
    {
        std::process::exit(1);
    }
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        // Writing to a String cannot fail.
        let _ = write!(text, "{byte:02x}");
    }
    text
}

fn ensure_fifo(path: &Path) -> Result<(), String> {
    if path.exists() {
        return Ok(());
    }
    let status = Command::new("mkfifo")
        .arg(path)
        .status()
        .map_err(|error| format!("cannot run mkfifo: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("mkfifo {} failed: {status}", path.display()))
    }
}

/// Reads command lines from the FIFO on a thread of its own (opening a FIFO blocks),
/// and opens it again whenever the writer closes it.
fn spawn_fifo_reader(path: PathBuf, lines: mpsc::UnboundedSender<String>) {
    std::thread::spawn(move || {
        loop {
            let file = match File::open(&path) {
                Ok(file) => file,
                Err(error) => {
                    eprintln!("cannot open {}: {error}", path.display());
                    return;
                }
            };
            for line in BufReader::new(file).lines() {
                let Ok(line) = line else {
                    break;
                };
                if lines.send(line).is_err() {
                    return;
                }
            }
        }
    });
}

enum FifoCommand {
    Unicast(MacAddr),
    MulticastSearch,
}

fn parse_command(line: &str) -> Result<FifoCommand, String> {
    let words: Vec<&str> = line.split_whitespace().collect();
    match words.as_slice() {
        ["send", "multicast", "search"] => Ok(FifoCommand::MulticastSearch),
        ["send", remote, "ap-cap-query"] => {
            let remote = remote
                .parse::<MacAddr>()
                .map_err(|error| format!("bad MAC {remote:?}: {error}"))?;
            Ok(FifoCommand::Unicast(remote))
        }
        _ => Err(format!("unknown command {line:?}")),
    }
}

fn header(msg_type: MsgType, msg_id: u16) -> CmduHeader {
    CmduHeader {
        msg_type,
        msg_id,
        frag_id: 0,
        last_fragment: true,
        relay: default_relay(msg_type),
    }
}

/// AP Capability Query with only End of Message (0x8001).
fn ap_capability_query(msg_id: u16) -> Result<Vec<u8>, BuildError> {
    MsgBuilder::new(&header(MsgType::AP_CAPABILITY_QUERY, msg_id), AL_SAP_BUDGET).finish()
}

/// AP-Autoconfiguration Search: the AL MAC, `SearchedRole` registrar and
/// `AutoconfigFreqBand` 2.4 GHz (0x00).
fn autoconfig_search(msg_id: u16, al_mac: MacAddr) -> Result<Vec<u8>, BuildError> {
    let mut builder = MsgBuilder::new(
        &header(MsgType::AP_AUTOCONFIGURATION_SEARCH, msg_id),
        AL_SAP_BUDGET,
    );
    builder
        .body(&AlMacAddress { mac: al_mac })
        .body(&SearchedRole(RoleValue {
            role: SearchedRoleCode::REGISTRAR,
        }))
        .body(&AutoconfigFreqBand(FrequencyBand {
            band: FreqBand::BAND_2_4_GHZ,
        }));
    builder.finish()
}

/// The remote address, the message type and the built CMDU for a command.
fn build_command(
    command: &FifoCommand,
    msg_id: u16,
    local: MacAddr,
) -> (MacAddr, MsgType, Result<Vec<u8>, BuildError>) {
    match command {
        FifoCommand::Unicast(remote) => (
            *remote,
            MsgType::AP_CAPABILITY_QUERY,
            ap_capability_query(msg_id),
        ),
        FifoCommand::MulticastSearch => (
            MacAddr::IEEE1905_MULTICAST,
            MsgType::AP_AUTOCONFIGURATION_SEARCH,
            autoconfig_search(msg_id, local),
        ),
    }
}

fn cmdu_type(cmdu: &[u8]) -> u16 {
    match CmduHeader::parse(cmdu) {
        Ok((header, _)) => header.msg_type.0,
        Err(_) => 0,
    }
}

fn print_received(sdu: &Sdu) {
    emit(&format!(
        "RECEIVED local={} remote={} type=0x{:04x} cmdu={}",
        sdu.local,
        sdu.remote,
        cmdu_type(&sdu.cmdu),
        hex(&sdu.cmdu)
    ));
}

/// Prints the lines for a change of link state from `previous` to `current`.
fn print_link_change(previous: LinkState, current: LinkState) {
    let was_registered = previous.al_mac.is_some();
    let new_session = current.session != previous.session;
    if was_registered && (current.al_mac.is_none() || new_session) {
        emit("DISCONNECTED");
    }
    match current.al_mac {
        Some(al_mac) if new_session => emit(&format!("REGISTERED {al_mac}")),
        _ => {}
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(args) => args,
        Err(message) => {
            eprintln!("alsap_probe: {message}");
            eprintln!(
                "usage: alsap_probe --role agent|controller --control <path> --data <path> --fifo <path>"
            );
            return ExitCode::from(2);
        }
    };
    if let Err(message) = ensure_fifo(&args.fifo) {
        eprintln!("alsap_probe: {message}");
        return ExitCode::from(2);
    }

    let (commands_sender, mut commands) = mpsc::unbounded_channel();
    spawn_fifo_reader(args.fifo, commands_sender);

    let config = TransportConfig::new(args.control, args.data, args.role);
    let Transport {
        sdus,
        mut received,
        mut link,
        counters: _counters,
        task: mut actor,
    } = spawn(config);

    let mut shown_link = LinkState::default();
    let mut next_msg_id: u16 = 1;

    loop {
        tokio::select! {
            sdu = received.recv() => {
                let Some(sdu) = sdu else {
                    break;
                };
                print_received(&sdu);
            }
            changed = link.changed() => {
                // The actor ended and dropped the watch.
                if changed.is_err() {
                    break;
                }
                let current = *link.borrow_and_update();
                print_link_change(shown_link, current);
                shown_link = current;
            }
            line = commands.recv() => {
                let Some(line) = line else {
                    eprintln!("alsap_probe: the FIFO reader stopped");
                    return ExitCode::FAILURE;
                };
                if line.trim().is_empty() {
                    continue;
                }
                // The SDU is built from the link as it is now, not from `shown_link`, which
                // lags until the loop has handled the change (note 0011 A6).
                let current_link = *link.borrow();
                let Some(local) = current_link.al_mac else {
                    eprintln!("alsap_probe: not registered, command ignored: {line}");
                    continue;
                };
                let command = match parse_command(&line) {
                    Ok(command) => command,
                    Err(message) => {
                        eprintln!("alsap_probe: {message}");
                        continue;
                    }
                };
                let msg_id = next_msg_id;
                next_msg_id = next_msg_id.wrapping_add(1);
                let (remote, msg_type, built) = build_command(&command, msg_id, local);
                let cmdu = match built {
                    Ok(cmdu) => cmdu,
                    Err(error) => {
                        eprintln!("alsap_probe: cannot build the message: {error:?}");
                        continue;
                    }
                };
                let outbound = OutboundSdu {
                    session: current_link.session,
                    sdu: Sdu { local, remote, cmdu },
                };
                if sdus.send(outbound).await.is_err() {
                    break;
                }
                emit(&format!("SENT {remote} type=0x{:04x} mid={msg_id}", msg_type.0));
            }
        }
    }

    // The received channel or the link watch closed: the actor ended, normally or with an error.
    match (&mut actor).await {
        Ok(Ok(())) => ExitCode::SUCCESS,
        Ok(Err(error)) => {
            eprintln!("alsap_probe: {error}");
            ExitCode::FAILURE
        }
        Err(error) => {
            eprintln!("alsap_probe: the transport task failed: {error}");
            ExitCode::FAILURE
        }
    }
}
