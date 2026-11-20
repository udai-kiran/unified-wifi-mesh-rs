// SPDX-License-Identifier: Apache-2.0

//! Operations that need both a node and its network: the part of `dm_easy_mesh_t` that reads
//! `m_network` next to the node's own members.

use crate::enums::ScannerType;
use crate::model::access::NodeCtx;
use crate::model::error::Put;
use crate::records::ScanResult;
use em_types::{InterfaceName, InterfaceNameError, MacAddr};

impl NodeCtx<'_> {
    /// `set_ctrl_al_interface_mac` (`dm_easy_mesh.cpp:2165-2169`): the node's AL MAC and the
    /// network's controller MAC both become `mac`. Used on the controller's own node.
    pub fn set_ctrl_al_interface_mac(&mut self, mac: MacAddr) {
        self.node.set_agent_al_interface_mac(mac);
        self.network.set_controller_id(mac);
    }

    /// `set_ctrl_al_interface_name` (`dm_easy_mesh.cpp:2171-2175`): the node's AL interface name
    /// and the network's controller interface name both become `name`. Upstream copies at most
    /// 31 bytes into its 32-byte `em_interface_name_t` (`inc/em_base.h:410`) and truncates the
    /// rest. The port rejects a name that is not a valid interface name (`InterfaceName`, at most
    /// 15 bytes).
    ///
    /// # Errors
    ///
    /// [`InterfaceNameError`] if `name` is not a valid interface name; nothing changes.
    pub fn set_ctrl_al_interface_name(&mut self, name: &str) -> Result<(), InterfaceNameError> {
        let device_name = InterfaceName::try_from(name)?;
        let controller_name = InterfaceName::try_from(name)?;
        self.node.device_mut().intf.name = Some(device_name);
        self.network.info_mut().ctrl_id.name = Some(controller_name);
        Ok(())
    }

    /// `update_scan_results` (`dm_easy_mesh.cpp:3599-3614`): stores a decoded scan result,
    /// replacing the one with the same key.
    ///
    /// Before the lookup the result's id is overridden, as upstream does: the network id (the
    /// agent's network, the id of this context's network; upstream hard-codes
    /// `GLOBAL_NET_ID` = `"OneWifiMesh"`, `inc/dm_easy_mesh.h:45`, which is the agent's network
    /// id), the `dev_mac` (the node's AL MAC) and the `scanner_type` (`radio`). The null
    /// dereference of a null result (`:3601`) cannot occur.
    pub fn update_scan_results(&mut self, mut scan: Box<ScanResult>) -> Put {
        scan.id.net_id = self.network.net().as_text();
        scan.id.dev_mac = self.node.agent_al_interface_mac();
        scan.id.scanner_type = ScannerType::RADIO;
        self.node.put_scan_result_record(scan)
    }
}
