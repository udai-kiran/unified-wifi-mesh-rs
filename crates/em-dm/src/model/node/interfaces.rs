// SPDX-License-Identifier: Apache-2.0

//! Host interface list and platform preferences of a node.

use super::{MAX_INTERFACES, MAX_PREFERENCES, NodeModel};
use crate::model::error::{ModelError, Table};
use crate::records::InterfacePreference;
use em_types::{Interface, MediaType};

impl NodeModel {
    /// `m_preference[0..m_num_preferences]`.
    #[must_use]
    pub fn preferences(&self) -> &[InterfacePreference] {
        self.preferences.as_slice()
    }

    /// Appends a platform preference (the JSON decode writes them,
    /// `dm_easy_mesh.cpp:919-946`, without a capacity check: 0007 defect 1).
    ///
    /// # Errors
    ///
    /// [`ModelError::Full`] at 5 preferences; nothing changes.
    pub fn push_preference(&mut self, preference: InterfacePreference) -> Result<(), ModelError> {
        self.preferences
            .push(preference)
            .map_err(|_| ModelError::Full {
                table: Table::Preference,
                capacity: MAX_PREFERENCES,
            })
    }

    /// `m_interfaces[0..m_num_interfaces]`, as the host adapter discovered them.
    #[must_use]
    pub fn interfaces(&self) -> &[Interface] {
        self.interfaces.as_slice()
    }

    /// `get_interface_by_index` (`inc/dm_easy_mesh.h:1067`): `None` at and beyond the count.
    #[must_use]
    pub fn interface(&self, index: usize) -> Option<&Interface> {
        self.interfaces.as_slice().get(index)
    }

    /// Appends a discovered host interface (the host adapter replaces `get_interfaces_list`,
    /// 0007 D1). Upstream checks the capacity only after the first write
    /// (`dm_easy_mesh.cpp:2245-2261`, 0007 defect 1); the port checks first.
    ///
    /// # Errors
    ///
    /// [`ModelError::Full`] at 8 interfaces; nothing changes.
    pub fn push_interface(&mut self, interface: Interface) -> Result<(), ModelError> {
        self.interfaces
            .push(interface)
            .map_err(|_| ModelError::Full {
                table: Table::Interface,
                capacity: MAX_INTERFACES,
            })
    }

    /// `get_prioritized_interface` (`dm_easy_mesh.cpp:2177-2217`): the first stored interface
    /// that suits the preference for `platform`.
    ///
    /// The first preference whose platform starts with `platform` is used (upstream:
    /// `strncmp(platform, preference.platform, strlen(platform))`, so the argument is a prefix of
    /// the stored platform). An Ethernet preference picks the first interface whose name contains
    /// `eth`, `ens` or `erouter`; an 802.11b/g 2.4 GHz preference picks the first name containing
    /// `wlan`; any other media picks nothing. The upstream pick is by `strstr` over the name,
    /// kept here as a substring test.
    #[must_use]
    pub fn prioritized_interface(&self, platform: &[u8]) -> Option<&Interface> {
        let preference = self
            .preferences
            .iter()
            .find(|preference| preference.platform.as_bytes().starts_with(platform))?;
        let wanted: &[&str] = if preference.media == MediaType::IEEE_802_3AB {
            &["eth", "ens", "erouter"]
        } else if preference.media == MediaType::IEEE_802_11B_2_4_GHZ {
            &["wlan"]
        } else {
            return None;
        };
        self.interfaces.iter().find(|interface| {
            interface.name.as_ref().is_some_and(|name| {
                wanted
                    .iter()
                    .any(|fragment| name.as_str().contains(fragment))
            })
        })
    }
}
