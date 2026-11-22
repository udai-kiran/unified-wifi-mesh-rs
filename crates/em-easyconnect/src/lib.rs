// SPDX-License-Identifier: Apache-2.0

//! Easy Connect boundary needed by S1 controller discovery (note 0012 D10).

use em_types::MacAddr;

/// Reports whether the AL device is actively onboarding through Easy Connect.
///
/// S1 has no DPP state machine, so this is always false. This is the boundary
/// for the enrollee check in `src/em/config/em_configuration.cpp:6695-6710`;
/// a later slice will supply the real onboarding state.
#[must_use]
pub const fn onboarding_in_progress(_al_mac: MacAddr) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn s1_has_no_active_onboarding() {
        assert!(!onboarding_in_progress(MacAddr::ZERO));
        assert!(!onboarding_in_progress(MacAddr::BROADCAST));
    }
}
