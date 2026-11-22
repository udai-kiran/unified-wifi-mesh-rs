// SPDX-License-Identifier: Apache-2.0

//! Command line of `em-agent` (0012 D4, D11): parsed with `std` only.

use std::path::PathBuf;

use em_cmd::RadioSpec;
use em_types::{FreqBand, MacAddr};

/// Upstream's agent control socket (`em_agent.h:34-35`).
const DEFAULT_CONTROL: &str = "/tmp/al_control_socket";
/// Upstream's agent data socket.
const DEFAULT_DATA: &str = "/tmp/al_data_socket";
/// The profile of upstream's agent (`em_orch_agent.cpp:259`).
const DEFAULT_PROFILE: u8 = 3;

/// The usage text printed with a bad argument.
pub const USAGE: &str = "usage: em-agent --radio <ruid>,<band> [--radio <ruid>,<band> ...] \
[--control <path>] [--data <path>] [--profile <2|3>]\n\
  --radio    a radio: its MAC and band (0 = 2.4 GHz, 1 = 5 GHz, 2 = 60 GHz, 3 = 6 GHz); repeatable, at least one\n\
  --control  AL-SAP control socket (default /tmp/al_control_socket)\n\
  --data     AL-SAP data socket (default /tmp/al_data_socket)\n\
  --profile  Multi-AP profile to advertise, 2 or 3 (default 3)";

/// The parsed command line.
#[derive(Debug, PartialEq, Eq)]
pub struct Args {
    /// AL-SAP control socket path.
    pub control: PathBuf,
    /// AL-SAP data socket path.
    pub data: PathBuf,
    /// The radios of the fake platform.
    pub radios: Vec<RadioSpec>,
    /// The Multi-AP profile, 2 or 3.
    pub profile: u8,
}

/// Parses the arguments after the program name.
///
/// # Errors
///
/// A message naming the first bad argument.
pub fn parse(arguments: impl IntoIterator<Item = String>) -> Result<Args, String> {
    let mut control = PathBuf::from(DEFAULT_CONTROL);
    let mut data = PathBuf::from(DEFAULT_DATA);
    let mut radios: Vec<RadioSpec> = Vec::new();
    let mut profile = DEFAULT_PROFILE;
    let mut arguments = arguments.into_iter();
    while let Some(flag) = arguments.next() {
        let value = arguments
            .next()
            .ok_or_else(|| format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--control" => control = PathBuf::from(value),
            "--data" => data = PathBuf::from(value),
            "--radio" => {
                let radio = parse_radio(&value)?;
                if radios.iter().any(|known| known.ruid == radio.ruid) {
                    return Err(format!("radio {} is given twice", radio.ruid));
                }
                radios.push(radio);
            }
            "--profile" => profile = parse_profile(&value)?,
            other => return Err(format!("unknown option {other:?}")),
        }
    }
    if radios.is_empty() {
        return Err("at least one --radio is required".to_owned());
    }
    Ok(Args {
        control,
        data,
        radios,
        profile,
    })
}

fn parse_radio(value: &str) -> Result<RadioSpec, String> {
    let (ruid, band) = value
        .split_once(',')
        .ok_or_else(|| format!("--radio {value:?}: expected <ruid>,<band>"))?;
    let ruid = ruid
        .parse::<MacAddr>()
        .map_err(|error| format!("--radio {value:?}: bad ruid: {error}"))?;
    let band = match band {
        "0" => FreqBand::BAND_2_4_GHZ,
        "1" => FreqBand::BAND_5_GHZ,
        "2" => FreqBand::BAND_60_GHZ,
        "3" => FreqBand::BAND_6_GHZ,
        other => {
            return Err(format!(
                "--radio {value:?}: band {other:?} is not 0, 1, 2 or 3"
            ));
        }
    };
    Ok(RadioSpec { ruid, band })
}

fn parse_profile(value: &str) -> Result<u8, String> {
    match value {
        "2" => Ok(2),
        "3" => Ok(3),
        other => Err(format!("--profile {other:?} is not 2 or 3")),
    }
}

#[cfg(test)]
mod tests {
    use em_cmd::RadioSpec;
    use em_types::{FreqBand, MacAddr};

    use super::{Args, parse};

    fn args(words: &[&str]) -> Result<Args, String> {
        parse(words.iter().map(|word| (*word).to_owned()))
    }

    #[test]
    fn defaults_are_upstreams_agent_sockets_and_profile_3() {
        let parsed = args(&["--radio", "02:00:00:00:01:01,0"]).unwrap();
        assert_eq!(parsed.control.to_str(), Some("/tmp/al_control_socket"));
        assert_eq!(parsed.data.to_str(), Some("/tmp/al_data_socket"));
        assert_eq!(parsed.profile, 3);
        assert_eq!(
            parsed.radios,
            [RadioSpec {
                ruid: MacAddr::new([2, 0, 0, 0, 1, 1]),
                band: FreqBand::BAND_2_4_GHZ,
            }]
        );
    }

    #[test]
    fn every_option_is_read_and_radios_repeat() {
        let parsed = args(&[
            "--control",
            "/run/c",
            "--data",
            "/run/d",
            "--profile",
            "2",
            "--radio",
            "02:00:00:00:01:01,1",
            "--radio",
            "02:00:00:00:01:02,3",
        ])
        .unwrap();
        assert_eq!(parsed.control.to_str(), Some("/run/c"));
        assert_eq!(parsed.data.to_str(), Some("/run/d"));
        assert_eq!(parsed.profile, 2);
        assert_eq!(parsed.radios.len(), 2);
        assert_eq!(parsed.radios[0].band, FreqBand::BAND_5_GHZ);
        assert_eq!(parsed.radios[1].band, FreqBand::BAND_6_GHZ);
    }

    #[test]
    fn bad_arguments_are_rejected() {
        let ruid = "02:00:00:00:01:01";
        assert!(args(&[]).is_err(), "no radio");
        assert!(args(&["--radio", ruid]).is_err(), "no band");
        assert!(args(&["--radio", "02:00:00:00:01:01,4"]).is_err(), "band 4");
        assert!(args(&["--radio", "nonsense,0"]).is_err(), "bad MAC");
        assert!(
            args(&[
                "--radio",
                "02:00:00:00:01:01,0",
                "--radio",
                "02:00:00:00:01:01,1"
            ])
            .is_err(),
            "same ruid twice"
        );
        assert!(args(&["--radio", "02:00:00:00:01:01,0", "--profile", "1"]).is_err());
        assert!(args(&["--radio", "02:00:00:00:01:01,0", "--profile", "4"]).is_err());
        assert!(args(&["--radio", "02:00:00:00:01:01,0", "--bogus", "x"]).is_err());
        assert!(args(&["--radio"]).is_err(), "missing value");
    }
}
