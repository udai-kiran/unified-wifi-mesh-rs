// SPDX-License-Identifier: Apache-2.0

//! Records of the `unassoc` group (see the module docs of `records`).

use crate::bounded::BoundedVec;
#[cfg(test)]
use crate::mapping::Map;
use em_types::MacAddr;

record! {
    /// Unassociated STAs to query on one channel.
    ///
    /// C `em_unassoc_query_channel_t` (`inc/em_base.h:620`).
    pub struct UnassocQueryChannel = "em_unassoc_query_channel_t", owned {
        /// C `channel`: `uint8_t`.
        pub channel: u8 = Map::field("channel"),
        /// C `sta_list`: `mac_address_t[EM_MAX_STA_PER_CHANNEL]` with count `num_sta`.
        pub sta_list: BoundedVec<MacAddr, 64> = Map::counted("num_sta", "sta_list"),
    }
}

record! {
    /// Unassociated STA query of one operating class.
    ///
    /// C `em_unassoc_query_opclass_t` (`inc/em_base.h:633`).
    pub struct UnassocQueryOpclass = "em_unassoc_query_opclass_t", owned {
        /// C `op_class`: `uint8_t`.
        pub op_class: u8 = Map::field("op_class"),
        /// C `channel_list`: `em_unassoc_query_channel_t[EM_MAX_CHANNELS_PER_OPCLASS]` with count `num_channels`.
        pub channel_list: BoundedVec<UnassocQueryChannel, 16> = Map::counted("num_channels", "channel_list"),
    }
}

record! {
    /// Unassociated STA query over several operating classes.
    ///
    /// C `em_unassoc_query_list_t` (`inc/em_base.h:645`).
    pub struct UnassocQueryList = "em_unassoc_query_list_t", owned {
        /// C `opclass_list`: `em_unassoc_query_opclass_t[EM_MAX_OPCLASS]` with count `num_opclass`.
        pub opclass_list: BoundedVec<UnassocQueryOpclass, 64> = Map::counted("num_opclass", "opclass_list"),
    }
}

record! {
    /// Link metrics of one unassociated STA.
    ///
    /// C `em_unassoc_sta_metric_entry_t` (`inc/em_base.h:665`).
    pub struct UnassocStaMetricEntry = "em_unassoc_sta_metric_entry_t", copy {
        /// C `sta_mac`: `mac_address_t`.
        pub sta_mac: MacAddr = Map::field("sta_mac"),
        /// C `channel`: `unsigned char`.
        pub channel: u8 = Map::field("channel"),
        /// C `op_class`: `unsigned char`.
        pub op_class: u8 = Map::field("op_class"),
        /// C `rcpi`: `unsigned char`.
        pub rcpi: u8 = Map::field("rcpi"),
        /// C `time_delta`: `unsigned int`.
        pub time_delta: u32 = Map::field("time_delta"),
    }
}

record! {
    /// Unassociated STA link metrics response.
    ///
    /// C `em_unassoc_sta_metrics_rsp_t` (`inc/em_base.h:673`).
    pub struct UnassocStaMetricsRsp = "em_unassoc_sta_metrics_rsp_t", owned {
        /// C `entry`: `em_unassoc_sta_metric_entry_t[EM_MAX_UNASSOC_STA]` with count `num_entries`.
        pub entry: BoundedVec<UnassocStaMetricEntry, 64> = Map::counted("num_entries", "entry"),
    }
}
