// SPDX-License-Identifier: Apache-2.0

//! The ten enums owned by `em-dm`.
//!
//! Each is an open newtype over the widest wire integer that feeds it (or, for stored-only enums,
//! the narrowest integer that holds every inventory value; the golden test keeps a per-enum table
//! of these widths with upstream cites): unknown codes are accepted and round-trip unchanged.
//! Sentinels (`*_max`, `*_unknown`, `*_none`, reserved) are not named consts; the golden test
//! lists each with its reason. The three enums shared with `em-types` (`FreqBand`, `MediaType`, `ProfileType`) live there.

open_enum! {
    /// HAL 802.11 variant bit mask. Stored in the device and radio-capability records. Its bit values reach `0x200`, so the type is 16 bits wide, not 8.
    ///
    /// C `wifi_ieee80211Variant_t` (`include/wifi_hal_generic.h:327` in rdkb-halif-wifi @ 1deddfc). Open newtype: unknown values round-trip.
    pub struct Ieee80211Variant(u16) = "wifi_ieee80211Variant_t";
    named: [
        A = 1 => "WIFI_80211_VARIANT_A",
        B = 2 => "WIFI_80211_VARIANT_B",
        G = 4 => "WIFI_80211_VARIANT_G",
        N = 8 => "WIFI_80211_VARIANT_N",
        H = 16 => "WIFI_80211_VARIANT_H",
        AC = 32 => "WIFI_80211_VARIANT_AC",
        AD = 64 => "WIFI_80211_VARIANT_AD",
        AX = 128 => "WIFI_80211_VARIANT_AX",
        BE = 256 => "WIFI_80211_VARIANT_BE",
        BN = 512 => "WIFI_80211_VARIANT_BN",
    ];
    sentinels: [
    ];
}

open_enum! {
    /// Extended-device product class. Dual-use: stored, and read from a wire byte (`em_configuration.cpp:1324`).
    ///
    /// C `emex_product_class_t` (`inc/em_base.h:2546`). Open newtype: unknown values round-trip.
    pub struct ProductClass(u8) = "emex_product_class_t";
    named: [
        GW = 128 => "emex_product_class_gw",
    ];
    sentinels: [
    ];
}

open_enum! {
    /// Extended-device role. Dual-use: stored, and read from a wire byte (`em_configuration.cpp:1327`).
    ///
    /// C `emex_device_role_t` (`inc/em_base.h:2550`). Open newtype: unknown values round-trip.
    pub struct DeviceRole(u8) = "emex_device_role_t";
    named: [
        CONTROLLER = 128 => "emex_device_role_controller",
    ];
    sentinels: [
    ];
}

open_enum! {
    /// Haul type. Dual-use: stored, read from the `unsigned short haultype` of the packed
    /// `em_ap_vendor_operational_bss_t` (`em_configuration.cpp:1638`, `inc/em_base.h:1617`), and
    /// emitted as a wire byte (`em_configuration.cpp:5924`). The widest wire field is 16 bits, so
    /// the type is 16 bits wide, not 8.
    ///
    /// C `em_haul_type_t` (`inc/em_base.h:2488`). Open newtype: unknown values round-trip.
    pub struct HaulType(u16) = "em_haul_type_t";
    named: [
        FRONTHAUL = 0 => "em_haul_type_fronthaul",
        BACKHAUL = 1 => "em_haul_type_backhaul",
        IOT = 2 => "em_haul_type_iot",
        CONFIGURATOR = 3 => "em_haul_type_configurator",
        HOTSPOT = 4 => "em_haul_type_hotspot",
    ];
    sentinels: [
        5 => "em_haul_type_max", "sentinel: upper-bound marker";
    ];
}

open_enum! {
    /// Virtual AP mode. Dual-use: stored, and read from the `unsigned short vap_mode` of the
    /// packed `em_ap_vendor_operational_bss_t` (`em_configuration.cpp:1639`,
    /// `inc/em_base.h:1617`). The wire field is 16 bits, so the type is 16 bits wide, not 8.
    ///
    /// C `em_vap_mode_t` (`inc/em_base.h:2497`). Open newtype: unknown values round-trip.
    pub struct VapMode(u16) = "em_vap_mode_t";
    named: [
        AP = 0 => "em_vap_mode_ap",
        STA = 1 => "em_vap_mode_sta",
    ];
    sentinels: [
    ];
}

open_enum! {
    /// Kind of an operating-class row. Stored only.
    ///
    /// C `em_op_class_type_t` (`inc/em_base.h:2665`). Open newtype: unknown values round-trip.
    pub struct OpClassType(u8) = "em_op_class_type_t";
    named: [
        CURRENT = 1 => "em_op_class_type_current",
        CAPABILITY = 2 => "em_op_class_type_capability",
        CAC_AVAILABLE = 3 => "em_op_class_type_cac_available",
        CAC_NON_OCC = 4 => "em_op_class_type_cac_non_occ",
        CAC_ACTIVE = 5 => "em_op_class_type_cac_active",
        PREFERENCE = 6 => "em_op_class_type_preference",
        ANTICIPATED = 7 => "em_op_class_type_anticipated",
        SCAN_PARAM = 8 => "em_op_class_type_scan_param",
        SELECTION = 9 => "em_op_class_type_selection",
    ];
    sentinels: [
        0 => "em_op_class_type_none", "sentinel: none marker";
    ];
}

open_enum! {
    /// Policy kind. Stored only (reached from `em_policy_t`).
    ///
    /// C `em_policy_id_type_t` (`inc/em_base.h:3760`). Open newtype: unknown values round-trip.
    pub struct PolicyIdType(u8) = "em_policy_id_type_t";
    named: [
        STEERING_LOCAL = 0 => "em_policy_id_type_steering_local",
        STEERING_BTM = 1 => "em_policy_id_type_steering_btm",
        STEERING_PARAM = 2 => "em_policy_id_type_steering_param",
        AP_METRICS_REP = 3 => "em_policy_id_type_ap_metrics_rep",
        RADIO_METRICS_REP = 4 => "em_policy_id_type_radio_metrics_rep",
        DEFAULT_8021Q_SETTINGS = 5 => "em_policy_id_type_default_8021q_settings",
        TRAFFIC_SEPARATION = 6 => "em_policy_id_type_traffic_separation",
        CHANNEL_SCAN = 7 => "em_policy_id_type_channel_scan",
        UNSUCCESS_ASSOC = 8 => "em_policy_id_type_unsuccess_assoc",
        BACKHAUL_BSS_CONFIG = 9 => "em_policy_id_type_backhaul_bss_config",
        QOS_MGT = 10 => "em_policy_id_type_qos_mgt",
        ALARM_THRESHOLD = 11 => "em_policy_id_type_alarm_threshold",
        CLIENT_FILTERS = 12 => "em_policy_id_type_client_filters",
    ];
    sentinels: [
        13 => "em_policy_id_type_unknown", "sentinel: unknown marker";
    ];
}

open_enum! {
    /// Steering policy kind. Stored only (reached from `em_policy_t`).
    ///
    /// C `em_steering_policy_type_t` (`inc/em_base.h:3785`). Open newtype: unknown values round-trip.
    pub struct SteeringPolicyType(u8) = "em_steering_policy_type_t";
    named: [
        DISALLOWED = 0 => "em_steering_policy_type_disallowed",
        RCPI_MANDATED = 1 => "em_steering_policy_type_rcpi_mandated",
        RCPI_ALLOWED = 2 => "em_steering_policy_type_rcpi_allowed",
    ];
    sentinels: [
        3 => "em_steering_policy_type_unknown", "sentinel: unknown marker";
    ];
}

open_enum! {
    /// Scanner kind of a scan result. Stored only (reached from `em_scan_result_id_t`).
    ///
    /// C `em_scanner_type_t` (`inc/em_base.h:1059`). Open newtype: unknown values round-trip.
    pub struct ScannerType(u8) = "em_scanner_type_t";
    named: [
        RADIO = 1 => "em_scanner_type_radio",
        STA = 2 => "em_scanner_type_sta",
    ];
    sentinels: [
    ];
}

open_enum! {
    /// HAL channel bandwidth bit mask. Stored only (reached from `em_neighbor_t`). The inventory values fit in 8 bits.
    ///
    /// C `wifi_channelBandwidth_t` (`include/wifi_hal_generic.h:344` in rdkb-halif-wifi @ 1deddfc). Open newtype: unknown values round-trip.
    pub struct ChannelBandwidth(u8) = "wifi_channelBandwidth_t";
    named: [
        MHZ_20 = 1 => "WIFI_CHANNELBANDWIDTH_20MHZ",
        MHZ_40 = 2 => "WIFI_CHANNELBANDWIDTH_40MHZ",
        MHZ_80 = 4 => "WIFI_CHANNELBANDWIDTH_80MHZ",
        MHZ_160 = 8 => "WIFI_CHANNELBANDWIDTH_160MHZ",
        MHZ_80_80 = 16 => "WIFI_CHANNELBANDWIDTH_80_80MHZ",
        MHZ_320 = 32 => "WIFI_CHANNELBANDWIDTH_320MHZ",
    ];
    sentinels: [
    ];
}

#[cfg(test)]
mod tests {
    use super::{HaulType, OpClassType, PolicyIdType, ProductClass, VapMode};

    #[test]
    fn unknown_codes_round_trip() {
        let unknown = HaulType::from(0xEE);
        assert_eq!(u16::from(unknown), 0xEE);
        assert_eq!(u16::from(HaulType::from(0x1234)), 0x1234);
        assert_eq!(unknown.name(), None);
        assert_eq!(OpClassType(0).name(), None, "sentinel 0 is an unnamed code");
        assert_eq!(
            PolicyIdType(13).name(),
            None,
            "sentinel 13 is an unnamed code"
        );
    }

    #[test]
    fn haul_type_and_vap_mode_hold_the_16_bit_wire_fields() {
        // `unsigned short haultype` and `vap_mode` of em_ap_vendor_operational_bss_t.
        assert_eq!(u16::from(HaulType::from(0xFFFF)), 0xFFFF);
        assert_eq!(u16::from(VapMode::from(0x0102)), 0x0102);
        assert_eq!(size_of::<HaulType>(), 2);
        assert_eq!(size_of::<VapMode>(), 2);
    }

    #[test]
    fn named_values_have_names_and_codes() {
        assert_eq!(HaulType::BACKHAUL.name(), Some("BACKHAUL"));
        assert_eq!(HaulType::BACKHAUL.0, 1);
        assert_eq!(ProductClass::GW.0, 0x80);
        assert_eq!(format!("{:?}", HaulType::IOT), "IOT(2)");
        assert_eq!(format!("{:?}", HaulType(9)), "HaulType(9)");
    }

    #[test]
    fn all_lists_every_named_value_once() {
        assert_eq!(HaulType::ALL.len(), 5);
        assert_eq!(HaulType::ALL[0], HaulType::FRONTHAUL);
    }
}
