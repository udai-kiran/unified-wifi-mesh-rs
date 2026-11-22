// SPDX-License-Identifier: Apache-2.0

//! What the agent asks of the platform (0012 D4, 0001 D7 rule 3).
//!
//! In S1 the platform is a fake that reports the radios given on the command line.
//! A `OneWifi` adapter can implement [`Platform`] later without touching the actor.

use em_cmd::RadioSpec;

/// The platform the agent runs on.
pub trait Platform {
    /// The radios of this device, one node each.
    fn radios(&self) -> Vec<RadioSpec>;
}

/// A platform whose radios are fixed at start, from `--radio` arguments.
#[derive(Debug)]
pub struct FakePlatform {
    radios: Vec<RadioSpec>,
}

impl FakePlatform {
    /// A platform with exactly these radios.
    #[must_use]
    pub const fn new(radios: Vec<RadioSpec>) -> Self {
        Self { radios }
    }
}

impl Platform for FakePlatform {
    fn radios(&self) -> Vec<RadioSpec> {
        Vec::from(self.radios.as_slice())
    }
}
