// SPDX-License-Identifier: Apache-2.0

//! Command line of `em-ctrl` (0012 D11): parsed with `std` only.

use std::path::PathBuf;

/// Upstream's controller control socket (`em_ctrl.cpp:1797-1798`).
const DEFAULT_CONTROL: &str = "/tmp/al_em_ctrl_control_socket";
/// Upstream's controller data socket.
const DEFAULT_DATA: &str = "/tmp/al_em_ctrl_data_socket";

/// The usage text printed with a bad argument.
pub const USAGE: &str = "usage: em-ctrl [--control <path>] [--data <path>]\n\
  --control  AL-SAP control socket (default /tmp/al_em_ctrl_control_socket)\n\
  --data     AL-SAP data socket (default /tmp/al_em_ctrl_data_socket)";

/// The parsed command line.
#[derive(Debug, PartialEq, Eq)]
pub struct Args {
    /// AL-SAP control socket path.
    pub control: PathBuf,
    /// AL-SAP data socket path.
    pub data: PathBuf,
}

/// Parses the arguments after the program name.
///
/// # Errors
///
/// A message naming the first bad argument.
pub fn parse(arguments: impl IntoIterator<Item = String>) -> Result<Args, String> {
    let mut control = PathBuf::from(DEFAULT_CONTROL);
    let mut data = PathBuf::from(DEFAULT_DATA);
    let mut arguments = arguments.into_iter();
    while let Some(flag) = arguments.next() {
        let value = arguments
            .next()
            .ok_or_else(|| format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--control" => control = PathBuf::from(value),
            "--data" => data = PathBuf::from(value),
            other => return Err(format!("unknown option {other:?}")),
        }
    }
    Ok(Args { control, data })
}

#[cfg(test)]
mod tests {
    use super::parse;

    fn args(words: &[&str]) -> Result<super::Args, String> {
        parse(words.iter().map(|word| (*word).to_owned()))
    }

    #[test]
    fn defaults_are_upstreams_controller_sockets() {
        let parsed = args(&[]).unwrap();
        assert_eq!(
            parsed.control.to_str(),
            Some("/tmp/al_em_ctrl_control_socket")
        );
        assert_eq!(parsed.data.to_str(), Some("/tmp/al_em_ctrl_data_socket"));
    }

    #[test]
    fn both_paths_can_be_set() {
        let parsed = args(&["--control", "/run/c", "--data", "/run/d"]).unwrap();
        assert_eq!(parsed.control.to_str(), Some("/run/c"));
        assert_eq!(parsed.data.to_str(), Some("/run/d"));
    }

    #[test]
    fn bad_arguments_are_rejected() {
        assert!(args(&["--bogus", "x"]).is_err());
        assert!(args(&["--control"]).is_err(), "missing value");
        assert!(args(&["--profile", "3"]).is_err(), "an agent option");
    }
}
