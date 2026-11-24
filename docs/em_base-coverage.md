# `inc/em_base.h` coverage

Every typedef in upstream `inc/em_base.h` @ `ba4180b`, with the crate that owns it (see [0001](0001-em-types-scope.md) D1).
Generated from a typedef scan of the pinned header (struct/enum/union blocks, plain aliases and function-pointer typedefs), then rule-assigned. Rows marked "verify" or "confirm" are provisional: the owning crate's port confirms or reassigns them and updates the Status column.

## Summary

| Owner | Types |
|---|---|
| em-msg | 194 |
| em-dm | 53 |
| em-core | 23 |
| do not port | 18 |
| em-cmd | 18 |
| pending (0002) | 13 |
| em-agent | 9 |
| em-types | 8 |
| em-db | 7 |
| em-transport | 6 |
| em-cli | 3 |
| em-ctrl | 3 |
| em-orch | 3 |
| em-crypto | 2 |
| deferred (ieee80211, D6) | 2 |
| replaced (D7) | 2 |
| em-easyconnect | 1 |
| **total** | **365** |

## Types

| Line | Kind | Upstream type | Owner | Note | Status |
|---|---|---|---|---|---|
| 410 | alias | `em_interface_name_t` | em-types | `InterfaceName` | ported |
| 411 | alias | `em_nonce_t` | em-crypto | `rand::Nonce` | ported |
| 412 | alias | `em_dh5_key_t` | em-crypto | `dh::DhPublic` / `dh::DhPrivate` | ported |
| 413 | alias | `em_short_string_t` | do not port | fixed-size C char buffer; replaced by `String`/bounded types | n/a |
| 414 | alias | `em_long_string_t` | do not port | fixed-size C char buffer; replaced by `String`/bounded types | n/a |
| 415 | alias | `em_2xlong_string_t` | do not port | fixed-size C char buffer; replaced by `String`/bounded types | n/a |
| 416 | alias | `em_3xlong_string_t` | do not port | fixed-size C char buffer; replaced by `String`/bounded types | n/a |
| 417 | alias | `em_4xlong_string_t` | do not port | fixed-size C char buffer; replaced by `String`/bounded types | n/a |
| 418 | alias | `em_string_t` | do not port | fixed-size C char buffer; replaced by `String`/bounded types | n/a |
| 419 | alias | `em_small_string_t` | do not port | fixed-size C char buffer; replaced by `String`/bounded types | n/a |
| 420 | alias | `em_tiny_string_t` | do not port | fixed-size C char buffer; replaced by `String`/bounded types | n/a |
| 421 | alias | `em_subdoc_name_space_t` | em-agent | OneWifi webconfig subdoc | planned |
| 422 | alias | `em_subdoc_data_buff_t` | do not port | zero-length trailing array; replaced by slices/`Vec<u8>` | n/a |
| 423 | alias | `em_status_string_t` | do not port | fixed-size C char buffer; replaced by `String`/bounded types | n/a |
| 424 | alias | `em_raw_data_t` | do not port | zero-length trailing array; replaced by slices/`Vec<u8>` | n/a |
| 427 | struct | `llc_hdr_t` | em-transport | link-layer header / raw frame | planned |
| 433 | struct | `snap_hdr_t` | em-transport | link-layer header / raw frame | planned |
| 438 | struct | `em_raw_hdr_t` | em-transport | link-layer header / raw frame | planned |
| 444 | struct | `em_raw_ext_hdr_t` | em-transport | link-layer header / raw frame | planned |
| 452 | struct | `em_attrib_t` | em-msg | no Rust type; upstream defines it (`em_base.h:452`) but nothing uses it; `WscAttrIter` and `write_wsc_attr` carry the 4-octet attribute header (0009 D5) | not needed: unused at the pin; the WSC walker and writer carry the header (0009 D5, B00) |
| 458 | struct | `em_tlv_t` | em-msg | `Tlv<'a>`, with `TlvIter` | ported (em-msg) |
| 464 | struct | `em_cmdu_t` | em-msg | `CmduHeader` | ported (em-msg) |
| 475 | struct | `em_client_assoc_event_t` | em-msg | `ClientAssociationEvent` | ported (em-msg B08) |
| 482 | alias | `em_enum_type_t` | do not port | C storage alias for enums | n/a |
| 484 | enum | `em_service_type_t` | em-types | `ServiceType`; `cli` (0x02) and `none` excluded, see D3 | ported |
| 493 | enum | `em_profile_type_t` | em-types | `ProfileType`; `reserved` and `max` excluded | ported |
| 501 | enum | `em_freq_band_t` | em-types | `FreqBand`; `unknown` excluded | ported |
| 509 | enum | `em_mlo_mode_bitmap_t` | em-core | MLO mode flags `EM_MLO_MODE_STR`, `NSTR`, `EMLSR`, `EMLMR`, OR-ed and tested in `em_configuration.cpp:224-326` (AP MLD and Backhaul STA MLD configuration); no TLV carries the bitmap, so no `em-msg` body uses it | planned |
| 516 | struct | `em_mlo_policy_t` | em-dm | `MloPolicy` (`em-dm` `records/mld.rs`); stored model root, used by `dm_easy_mesh.cpp` and `em_configuration.cpp` | ported |
| 521 | struct | `em_channels_list_t` | em-msg | no Rust type; the channel list is a `&[u8]` field on each body | not needed: channel lists are slices on the bodies (0009 D2, B05) |
| 525 | struct | `em_per_op_class_channels_list_t` | em-msg | no Rust type; the channel list is a `&[u8]` field on each body | not needed: channel lists are slices on the bodies (0009 D2, B05) |
| 529 | struct | `em_op_class_t` | em-msg | `BasicOperatingClass<'a>`; used by `em_ap_radio_basic_cap_t` (sender `em.cpp:1090-1128`, receiver `em_capability.cpp:1272`) | ported (em-msg B04) |
| 536 | struct | `em_scan_cap_op_class_info_t` | em-msg | `ScanOperatingClass<'a>` | ported (em-msg B05) |
| 546 | struct | `em_cac_op_class_t` | em-msg | `CacOperatingClass<'a>` | ported (em-msg B04) |
| 552 | struct | `em_ap_radio_basic_cap_t` | em-msg | `ApRadioBasicCapabilities<'a>` | ported (em-msg B04) |
| 583 | struct | `em_unassoc_work_channel_t` | em-agent | agent work list for the unassociated-STA subdoc sent to OneWifi (`em_agent.cpp:944`); not a TLV | planned |
| 595 | struct | `em_unassoc_work_opclass_t` | em-agent | agent work list for the unassociated-STA subdoc sent to OneWifi (`em_agent.cpp:923`); not a TLV | planned |
| 607 | struct | `em_unassoc_work_list_t` | em-agent | agent work list for the unassociated-STA subdoc sent to OneWifi (`em_agent.cpp:893`, `:1137`); not a TLV | planned |
| 620 | struct | `em_unassoc_query_channel_t` | em-msg | `UnassociatedQueryChannel` | ported (em-msg B07) |
| 633 | struct | `em_unassoc_query_opclass_t` | em-msg | `UnassociatedStaLinkMetricsQuery`, with `UnassociatedQueryChannel` | ported (em-msg B07) |
| 645 | struct | `em_unassoc_query_list_t` | em-msg | no Rust type; the sender's list of operating classes | not needed: one TLV is written per operating class, so the list is not on the wire (0009 D2, B07) |
| 665 | struct | `em_unassoc_sta_metric_entry_t` | em-msg | no Rust type; an entry of the data model's response list | not needed: the wire form is `UnassociatedStaMetrics` (0009 D2, B07) |
| 673 | struct | `em_unassoc_sta_metrics_rsp_t` | em-msg | no Rust type; the data model's response list, an input to the sender | not needed: the wire form is `UnassociatedStaLinkMetricsResponse` (0009 D2, B07) |
| 679 | struct | `em_unassoc_sta_metric_t` | em-msg | `UnassociatedStaMetrics` | ported (em-msg B07) |
| 686 | struct | `em_unassoc_sta_link_metrics_rsp_t` | em-msg | `UnassociatedStaLinkMetricsResponse`, with `UnassociatedStaMetrics` | ported (em-msg B07) |
| 692 | enum | `em_tlv_requirement_t` | em-msg | `tables::Requirement` (mandatory, optional, bad) | ported (em-msg) |
| 698 | enum | `em_msg_type_t` | em-types | `MsgType` | ported |
| 773 | enum | `em_attrib_id_t` | do not port | only parameter type of `dm_easy_mesh_t::commit_config(em_attrib_id_t, ...)` (`dm_easy_mesh.cpp:346-349`), a stub that returns 0; `em-dm` records it as not ported (`dm_method_map.tsv`, `commit_config@cpp:346`) | not needed: only a stub's parameter type (0009 A11) |
| 792 | enum | `em_wsc_msg_type_t` | em-msg | `WSC_M1` and `WSC_M2` (`wsc`), with `wsc_msg_type`, which returns the raw message type octet; no other enumerator is named in upstream code | ported (em-msg B00, B12); the other values are not needed (0009 A11) |
| 811 | enum | `em_tlv_type_t` | em-types | `TlvType`; `max` excluded | ported |
| 937 | enum | `em_vendor_airties_tlv_type_t` | em-core | Airties vendor sub-TLV ids (0x0003, 0x0004, 0x0013) inside `em_vendor_specific_t`; tested in `em_configuration.cpp:1350`, `em_metrics.cpp:1103`, `:2208`, `em_capability.cpp:1689`, `:2076`, `em.cpp:1355` (0009 D5) | planned |
| 943 | enum | `em_channel_pref_reason_t` | em-msg | no Rust type; the reason is a raw 4-bit value on `PreferenceOperatingClass` | not needed: the reason octet is carried raw (0009 D2, B05) |
| 960 | struct | `em_qos_mgmt_des_t` | em-msg | `QosManagementDescriptor<'a>` | ported (em-msg B09) |
| 967 | struct | `em_agent_list_agent_t` | em-msg | `Agent` | ported (em-msg B03) |
| 973 | struct | `em_agent_list_t` | em-msg | `AgentList` | ported (em-msg B03) |
| 978 | struct | `em_anti_chan_usage_t` | em-msg | `AnticipatedChannelUsage<'a>`, with `AnticipatedUsage<'a>` | ported (em-msg B06) |
| 995 | struct | `em_dpp_bootstrap_uri_t` | em-msg | `DppBootstrappingUriNotification<'a>` | ported (em-msg B11) |
| 1002 | struct | `em_cce_indication_t` | em-msg | `DppCceIndication` | ported (em-msg B11) |
| 1006 | struct | `em_encap_dpp_t` | em-msg | `Ieee1905EncapDpp<'a>` | ported (em-msg B11) |
| 1022 | struct | `em_assoc_sts_notif_t` | em-msg | `AssociationStatusNotification`, with `AssociationStatusEntry` | ported (em-msg B08) |
| 1028 | struct | `em_prof2_error_t` | em-msg | `Profile2ErrorCode` | ported (em-msg B03) |
| 1035 | struct | `em_cac_term_t` | em-msg | `CacTermination`, with `CacTerminationRadio` | ported (em-msg B06) |
| 1043 | struct | `em_cac_req_t` | em-msg | `CacRequest`, with `CacRequestRadio` | ported (em-msg B06) |
| 1053 | struct | `em_timestamp_t` | em-msg | `Timestamp<'a>` | ported (em-msg B05) |
| 1059 | enum | `em_scanner_type_t` | em-dm | `ScannerType` (`em-dm` `enums.rs`); stored in `ScanResultId` | ported |
| 1064 | struct | `em_scan_result_id_t` | em-dm | `ScanResultId` (`em-dm` `records/scan.rs`); scan row key, built at `em_channel.cpp:2426` | ported |
| 1073 | struct | `em_neighbor_t` | em-dm | `Neighbor` (`em-dm` `records/scan.rs`); stored in `ScanResult` | ported |
| 1084 | struct | `em_scan_result_t` | em-dm | `ScanResult` (`em-dm` `records/scan.rs`); the decoded channel-scan TLV has its own wire type, `ChannelScanResult<'a>` | ported |
| 1096 | struct | `em_channel_scan_result_t` | em-msg | `ChannelScanResult<'a>` | ported (em-msg B05) |
| 1105 | struct | `em_scan_params_t` | em-cmd | scan request parameters; `em_cmd_scan_params_t` aliases it (`em_base.h:3510`, `:3520`); built from the Channel Scan Request at `em_channel.cpp:2165-2282`, `em.cpp:973` | planned |
| 1115 | struct | `em_channel_scan_req_op_class_t` | em-msg | `ScanOperatingClass<'a>` | ported (em-msg B05) |
| 1121 | struct | `em_channel_scan_req_t` | em-msg | `ChannelScanRequest<'a>`, with `ScanRequestRadio<'a>` | ported (em-msg B05) |
| 1130 | struct | `em_higher_layer_data_t` | em-msg | `HigherLayerData<'a>` | ported (em-msg B08) |
| 1135 | struct | `em_error_code_t` | em-msg | `ErrorCode` | ported (em-msg B08) |
| 1140 | struct | `em_bh_steering_resp_t` | em-msg | `BackhaulSteeringResponse` | ported (em-msg B08) |
| 1146 | struct | `em_bh_steering_req_t` | em-msg | `BackhaulSteeringRequest` | ported (em-msg B08) |
| 1153 | struct | `em_client_assoc_ctrl_req_t` | em-msg | `ClientAssociationControlRequest` | ported (em-msg B08) |
| 1161 | struct | `em_steering_btm_rprt_t` | em-msg | `SteeringBtmReport` | ported (em-msg B08) |
| 1168 | struct | `em_steering_req_t` | em-msg | `SteeringRequest`, with `SteeringMode`, `SteeringTarget` and `SteeringFlags` | ported (em-msg B08) |
| 1185 | struct | `em_80211_btm_req_reqmode_t` | em-agent | 802.11 BTM Request frame built for OneWifi in `dm_easy_mesh_agent.cpp:834`; not an EasyMesh TLV | planned |
| 1194 | struct | `em_80211_neighbor_report_t` | em-agent | 802.11 neighbor report element of the BTM Request frame built for OneWifi (`dm_easy_mesh_agent.cpp:820`); not an EasyMesh TLV | planned |
| 1206 | struct | `em_80211_btm_req_var_t` | em-agent | variable part of the BTM Request frame built for OneWifi (`dm_easy_mesh_agent.cpp:849`); not an EasyMesh TLV | planned |
| 1212 | struct | `em_profile2_steering_req_t` | em-msg | `Profile2SteeringRequest`, with `Profile2SteeringTarget` | ported (em-msg B08) |
| 1217 | struct | `em_beacon_metrics_resp_t` | em-msg | `BeaconMetricsResponse<'a>`, with `MeasurementReportElement<'a>` | ported (em-msg B07) |
| 1224 | struct | `em_beacon_ap_channel_rprt_t` | em-msg | `ApChannelReport<'a>` | ported (em-msg B07) |
| 1230 | struct | `em_beacon_element_list_t` | em-msg | no Rust type; the element IDs are a counted list | not needed: the element IDs are a borrowed slice, `element_ids` on `BeaconMetricsQuery` (0009 D2, B07) |
| 1235 | struct | `em_beacon_metrics_query_t` | em-msg | `BeaconMetricsQuery<'a>` | ported (em-msg B07) |
| 1248 | struct | `em_unassoc_sta_link_metrics_query_t` | em-msg | `UnassociatedStaLinkMetricsQuery`, with `UnassociatedQueryChannel` | ported (em-msg B07) |
| 1256 | struct | `em_assoc_sta_mac_addr_t` | em-msg | `StaMacAddress` | ported (em-msg B07) |
| 1260 | struct | `em_assoc_link_metrics_t` | em-msg | `AssociatedStaBssMetrics` | ported (em-msg B07) |
| 1268 | struct | `em_assoc_sta_link_metrics_t` | em-msg | `AssociatedStaLinkMetrics` | ported (em-msg B07) |
| 1274 | struct | `em_assoc_ext_link_metrics_t` | em-msg | `ExtendedLinkMetricsBss` | ported (em-msg B07) |
| 1282 | struct | `em_assoc_sta_ext_link_metrics_t` | em-msg | `AssociatedStaExtendedLinkMetrics` | ported (em-msg B07) |
| 1288 | struct | `em_assoc_vendor_link_metrics_t` | em-core | per-BSS entry of the Airties vendor link metrics; unused at the pin (named only in a comment, `em_base.h:1301`); tracked with `em_assoc_sta_vendor_link_metrics_t` | planned |
| 1296 | struct | `em_assoc_sta_vendor_link_metrics_t` | em-core | vendor sub-id 0x03 (`client_type`) of the Vendor Specific TLV, sent at `em_metrics.cpp:1686-1696` and read at `:167-178` (0009 D5) | planned |
| 1304 | struct | `em_ap_metrics_query_t` | em-msg | `ApMetricsQuery` | ported (em-msg B07) |
| 1309 | struct | `em_ap_radio_id_t` | em-msg | `ApRadioIdentifier` (over `MacValue`) | ported (em-msg B03) |
| 1313 | struct | `em_ap_metric_t` | em-msg | `ApMetrics` | ported (em-msg B07) |
| 1326 | struct | `em_ap_ext_metric_t` | em-msg | `ApExtendedMetrics` | ported (em-msg B07) |
| 1336 | struct | `em_radio_metric_t` | em-msg | `RadioMetrics` | ported (em-msg B07) |
| 1344 | struct | `em_assoc_sta_traffic_sts_t` | em-msg | `AssociatedStaTrafficStats` | ported (em-msg B07) |
| 1355 | struct | `em_assoc_wifi6_sta_sts_t` | em-msg | `AssociatedWifi6StaStatusReport`, with `Wifi6TidQueueSize` | ported (em-msg B07) |
| 1362 | struct | `em_client_info_t` | em-msg | `ClientInfo` | ported (em-msg B04) |
| 1367 | struct | `em_client_cap_rprt_t` | em-msg | `ClientCapabilityReport<'a>` | ported (em-msg B04) |
| 1373 | struct | `em_op_class_ch_rprt_t` | em-msg | `OperatingChannel` | ported (em-msg B05) |
| 1378 | struct | `em_op_channel_rprt_t` | em-msg | `OperatingChannelReport` | ported (em-msg B05) |
| 1384 | struct | `em_spatial_reuse_rprt_t` | em-msg | `SpatialReuseReport`, with `SpatialReuseParameters` | ported (em-msg B06) |
| 1404 | struct | `em_alarm_samples_t` | em-dm | `AlarmSamples` (`em-dm` `records/sta.rs`); inside `LinkReport` | ported |
| 1412 | struct | `em_link_report_t` | em-dm | `LinkReport` (`em-dm` `records/sta.rs`); em-core reads and writes it as vendor sub-id 0x06 (`em_metrics.cpp:944`, `:2327`) | ported |
| 1421 | enum | `em_chan_sel_resp_code_type_t` | em-msg | no Rust type; the code is a raw octet on `ChannelSelectionResponse` | not needed: the response code is carried raw (0009 D2, B05) |
| 1428 | enum | `em_prof2_error_code_reason_code_type_t` | em-msg | `Profile2ErrorReason` (in `Profile2ErrorCode`) | ported (em-msg B03) |
| 1446 | struct | `em_channel_sel_rsp_t` | em-msg | `ChannelSelectionResponse` | ported (em-msg B05) |
| 1451 | struct | `em_spatial_reuse_cfg_rsp_t` | em-msg | `SpatialReuseConfigResponse` | ported (em-msg B06) |
| 1455 | struct | `em_channel_pref_op_class_t` | em-msg | `PreferenceOperatingClass<'a>` | ported (em-msg B05) |
| 1461 | struct | `em_channel_pref_t` | em-msg | `ChannelPreference<'a>` | ported (em-msg B05) |
| 1467 | struct | `em_tx_power_limit_t` | em-msg | `TransmitPowerLimit` | ported (em-msg B05) |
| 1472 | struct | `em_spatial_reuse_req_t` | em-msg | `SpatialReuseRequest`, with `SpatialReuseParameters` | ported (em-msg B06) |
| 1490 | struct | `em_radio_op_restrict_channel_t` | em-msg | `RestrictedChannel` | ported (em-msg B05) |
| 1495 | struct | `em_radio_op_restrict_op_class_t` | em-msg | `RestrictedOperatingClass` | ported (em-msg B05) |
| 1501 | struct | `em_radio_op_restriction_t` | em-msg | `RadioOperationRestriction` | ported (em-msg B05) |
| 1508 | struct | `em_cac_comp_rprt_pair_t` | em-msg | `CacDetectedPair` | ported (em-msg B06) |
| 1513 | struct | `em_cac_comp_rprt_radio_t` | em-msg | `CacCompletionRadio` | ported (em-msg B06) |
| 1522 | struct | `em_cac_comp_rprt_t` | em-msg | `CacCompletionReport` | ported (em-msg B06) |
| 1528 | struct | `em_cac_avail_t` | em-msg | `CacAvailableChannel` | ported (em-msg B06) |
| 1534 | struct | `em_cac_non_occ_t` | em-msg | `CacNonOccupancyChannel` | ported (em-msg B06) |
| 1540 | struct | `em_cac_active_t` | em-msg | `CacActiveChannel` | ported (em-msg B06) |
| 1546 | struct | `em_cac_status_rprt_avail_t` | em-msg | no Rust type; the counted list is a `Vec` on `CacStatusReport` | not needed: the three counted lists are `Vec`s on `CacStatusReport` (0009 D2, B06) |
| 1551 | struct | `em_cac_status_rprt_non_occ_t` | em-msg | no Rust type; the counted list is a `Vec` on `CacStatusReport` | not needed: the three counted lists are `Vec`s on `CacStatusReport` (0009 D2, B06) |
| 1556 | struct | `em_cac_status_rprt_active_t` | em-msg | no Rust type; the counted list is a `Vec` on `CacStatusReport` | not needed: the three counted lists are `Vec`s on `CacStatusReport` (0009 D2, B06) |
| 1561 | struct | `em_media_spec_data_t` | em-msg | no Rust type; the bytes are `LocalInterface::media_info` | not needed: media-specific info is carried as raw bytes (0009 D2, B01) |
| 1571 | struct | `em_local_interface_t` | em-msg | `LocalInterface<'a>` | ported (em-msg B01) |
| 1576 | struct | `em_device_info_type_t` | em-msg | `DeviceInformation<'a>` | ported (em-msg B01) |
| 1582 | struct | `em_bridge_tuple_mac_entry_t` | em-msg | `BridgingTuple::macs` (`Vec<MacAddr>`) | ported (em-msg B01) |
| 1586 | struct | `em_bridge_tuple_entry_t` | em-msg | `BridgingTuple` | ported (em-msg B01) |
| 1591 | struct | `em_device_bridge_cap_t` | em-msg | `DeviceBridgingCapability` | ported (em-msg B01) |
| 1596 | struct | `em_non_1905_neigh_entry_t` | em-msg | `Non1905NeighborDeviceList::neighbors` (`Vec<MacAddr>`) | ported (em-msg B01) |
| 1600 | struct | `em_non_1905_neigh_device_list_t` | em-msg | `Non1905NeighborDeviceList` | ported (em-msg B01) |
| 1606 | struct | `em_neigh_entry_t` | em-msg | `Ieee1905Neighbor` | ported (em-msg B01) |
| 1611 | struct | `em_neigh_device_list_t` | em-msg | `Ieee1905NeighborDevice` | ported (em-msg B01) |
| 1617 | struct | `em_ap_vendor_operational_bss_t` | em-msg | `VendorBss` | ported (em-msg B03) |
| 1623 | struct | `em_ap_vendor_op_bss_radio_t` | em-msg | `VendorRadio` | ported (em-msg B03) |
| 1629 | struct | `em_ap_vendor_op_bss_t` | em-msg | `VendorOperationalBss` | ported (em-msg B03) |
| 1634 | struct | `em_ap_operational_bss_t` | em-msg | `OperationalBss<'a>` | ported (em-msg B03) |
| 1640 | struct | `em_ap_op_bss_radio_t` | em-msg | `OperationalRadio<'a>` | ported (em-msg B03) |
| 1646 | struct | `em_ap_op_bss_t` | em-msg | `ApOperationalBss<'a>` | ported (em-msg B03) |
| 1651 | struct | `em_assoc_clients_sta_t` | em-msg | `AssociatedClient` | ported (em-msg B03) |
| 1656 | struct | `em_assoc_clients_bss_t` | em-msg | `AssociatedBss` | ported (em-msg B03) |
| 1662 | struct | `em_assoc_clients_t` | em-msg | `AssociatedClients` | ported (em-msg B03) |
| 1667 | struct | `em_bssid_t` | em-msg | `Bssid` (over `MacValue`) | ported (em-msg B03) |
| 1671 | struct | `em_status_code_t` | em-msg | `StatusCode` | ported (em-msg B08) |
| 1675 | struct | `em_reason_code_t` | em-msg | `ReasonCode` | ported (em-msg B08) |
| 1679 | struct | `em_affiliated_sta_metrics_t` | em-msg | `AffiliatedStaMetrics` | ported (em-msg B10) |
| 1689 | struct | `em_bss_conf_rsp_t` | em-msg | `BssConfigurationResponse<'a>` | ported (em-msg B10) |
| 1693 | struct | `em_bss_conf_req_t` | em-msg | `BssConfigurationRequest<'a>` | ported (em-msg B10) |
| 1697 | struct | `em_8021q_settings_t` | em-msg | `Default8021QSettings` | ported (em-msg B09) |
| 1703 | struct | `em_traffic_sep_policy_ssid_t` | em-msg | no Rust type; the packed struct is not used on the wire (audit 175) | not needed: the wire form is `TrafficSeparationSsid<'a>` (0009 D2, B09) |
| 1709 | struct | `em_traffic_sep_policy_t` | em-msg | no Rust type; the packed struct is not used on the wire (audit 175) | not needed: the wire form is `TrafficSeparationPolicy<'a>` (0009 D2, B09) |
| 1714 | struct | `em_bh_akm_suite_t` | em-msg | `AkmSuite`, the backhaul list entry | ported (em-msg B11) |
| 1719 | struct | `em_fh_akm_suite_t` | em-msg | `AkmSuite`, the fronthaul list entry | ported (em-msg B11) |
| 1724 | struct | `em_fh_akm_suite_info_t` | em-msg | no Rust type; the count-prefixed fronthaul list is part of `AkmSuiteCapabilities` | not needed: the list is part of `AkmSuiteCapabilities` (0009 D2, B11) |
| 1729 | struct | `em_bh_akm_suite_info_t` | em-msg | no Rust type; the count-prefixed backhaul list is part of `AkmSuiteCapabilities` | not needed: the list is part of `AkmSuiteCapabilities` (0009 D2, B11) |
| 1734 | struct | `em_akm_suite_info_t` | em-msg | `AkmSuiteCapabilities` | ported (em-msg B11) |
| 1738 | struct | `em_bh_sta_radio_cap_t` | em-msg | `BackhaulStaRadioCapabilities` | ported (em-msg B04) |
| 1745 | struct | `em_anti_channel_pref_t` | em-msg | `AnticipatedChannelPreference<'a>`, with `AnticipatedOperatingClass<'a>` | ported (em-msg B06) |
| 1750 | struct | `em_profile_2_ap_cap_t` | em-msg | `Profile2ApCapability` | ported (em-msg B04) |
| 1761 | struct | `em_service_prio_rule_t` | em-msg | `ServicePrioritizationRule` | ported (em-msg B09) |
| 1771 | struct | `em_dscp_map_table_t` | em-msg | `DscpMappingTable` | ported (em-msg B09) |
| 1775 | struct | `em_assoc_sta_traffic_stats_t` | em-msg | `AssociatedStaTrafficStats`; sender `em_metrics.cpp:897-902` | ported (em-msg B07) |
| 1786 | struct | `em_source_info_t` | em-msg | `SourceInfo` | ported (em-msg B11) |
| 1790 | struct | `em_tunneled_msg_type_t` | em-msg | `TunneledMessageType` | ported (em-msg B11) |
| 1794 | struct | `em_tunneled_t` | em-msg | no Rust type; the struct holds a length and a pointer, so it cannot describe the wire form (audit 190) | not needed: the wire form is `Tunneled<'a>` (0009 D2, B11) |
| 1799 | struct | `em_1905_mac_addr_t` | em-msg | `MacAddress` (1905.1 MAC Address TLV, 6 octets) | ported (em-msg B01) |
| 1803 | struct | `em_vendor_specific_t` | em-msg | `VendorSpecific<'a>` | ported (em-msg B01) |
| 1808 | struct | `em_radio_capability_vendor_t` | em-core | Airties radio capability sub-format (0x0013) inside `em_vendor_specific_t`; sent at `em.cpp:1358`, read at `em_capability.cpp:1690-1696`, `:2077-2083` (0009 D5) | planned |
| 1814 | struct | `em_radio_metrics_t` | em-core | per-radio entry of the Airties device metrics sub-format (0x0004); sent at `em_metrics.cpp:2258`, read at `:760` (0009 D5) | planned |
| 1819 | struct | `em_radio_device_metrics_vendor_t` | em-core | header of the Airties device metrics sub-format (0x0004); sent at `em_metrics.cpp:2212`, read at `:744-748` (0009 D5) | planned |
| 1830 | struct | `em_link_metric_query_t` | em-msg | `LinkMetricQuery` | ported (em-msg B02) |
| 1836 | struct | `em_tx_link_metric_entry_t` | em-msg | `TransmitterLinkMetricPair` | ported (em-msg B02) |
| 1848 | struct | `em_tx_link_metric_t` | em-msg | `TransmitterLinkMetric` | ported (em-msg B02) |
| 1855 | struct | `em_rx_link_metric_entry_t` | em-msg | `ReceiverLinkMetricPair` | ported (em-msg B02) |
| 1864 | struct | `em_rx_link_metric_t` | em-msg | `ReceiverLinkMetric` | ported (em-msg B02) |
| 1871 | struct | `em_radio_vendor_t` | em-msg | `InventoryRadio<'a>` | ported (em-msg B04) |
| 1877 | struct | `em_device_inventory_t` | em-msg | `DeviceInventory<'a>` | ported (em-msg B04) |
| 1881 | struct | `em_ap_ht_cap_t` | em-msg | `ApHtCapabilities` | ported (em-msg B04) |
| 1891 | struct | `em_ap_vht_cap_t` | em-msg | `ApVhtCapabilities` | ported (em-msg B04) |
| 1906 | struct | `em_ap_he_mcs_maps_t` | em-msg | `HeMcsSet` (in `HeMcsSets`) | ported (em-msg B04) |
| 1911 | struct | `em_ap_he_cap_t` | em-msg | `ApHeCapabilities` | ported (em-msg B04) |
| 1929 | struct | `em_wifi6_cap_role_head_tlv_t` | em-msg | bit-field octet of the Wi-Fi 6 role; no Rust type | not needed: its fields are on `Wifi6Role` (0009 D2, B04) |
| 1936 | struct | `em_wifi6_cap_role_tail_tlv_t` | em-msg | bit-field octets of the Wi-Fi 6 role; no Rust type | not needed: its fields are on `Wifi6Role` (0009 D2, B04) |
| 1959 | struct | `em_wifi6_role_wire_t` | em-msg | `Wifi6Role` | ported (em-msg B04) |
| 1965 | struct | `em_radio_wifi6_cap_data_t` | em-msg | `ApWifi6Capabilities` | ported (em-msg B04) |
| 1970 | struct | `em_wifi7_freq_record_tlv_t` | em-msg | `Wifi7FrequencyRecord` | ported (em-msg B04) |
| 1976 | struct | `em_wifi7_freq_records_t` | em-msg | `Wifi7ModeCapabilities` (its record vectors) | ported (em-msg B04) |
| 1981 | struct | `em_wifi7_cap_link_info_tlv_t` | em-msg | link limits and TID mapping; no Rust type | not needed: its fields are on `Wifi7AgentCapabilities` (0009 D2, B04) |
| 1990 | struct | `em_wifi7_mlo_cap_support_tlv_t` | em-msg | per-radio RUID and mode flags; no Rust type | not needed: its fields are on `Wifi7Radio` and `Wifi7ModeCapabilities` (0009 D2, B04) |
| 2005 | struct | `em_wifi7_mlo_cap_records_t` | em-msg | `Wifi7Radio` (`ap`, `backhaul_sta`) | ported (em-msg B04) |
| 2016 | struct | `em_wifi7_agent_cap_t` | em-msg | `Wifi7Radio` | ported (em-msg B04) |
| 2021 | struct | `em_wifi7_cap_tlv_t` | em-msg | `Wifi7AgentCapabilities` | ported (em-msg B04) |
| 2027 | struct | `em_eht_operations_bss_t` | em-msg | `EhtOperationsBss` | ported (em-msg B10) |
| 2043 | struct | `em_eht_operations_radio_t` | em-msg | `EhtOperationsRadio` | ported (em-msg B10) |
| 2050 | struct | `em_eht_operations_t` | em-msg | `EhtOperations` | ported (em-msg B10) |
| 2055 | struct | `em_avail_spectrum_inquiry_req_t` | em-msg | `AvailableSpectrumInquiryRequest<'a>` (over `SpectrumInquiryObject<'a>`) | ported (em-msg B06) |
| 2059 | struct | `em_avail_spectrum_inquiry_rsp_t` | em-msg | `AvailableSpectrumInquiryResponse<'a>` (over `SpectrumInquiryObject<'a>`) | ported (em-msg B06) |
| 2063 | struct | `em_channel_scan_cap_radio_t` | em-msg | `ScanCapabilityRadio<'a>` | ported (em-msg B05) |
| 2073 | struct | `em_channel_scan_cap_t` | em-msg | `ChannelScanCapabilities<'a>` | ported (em-msg B05) |
| 2079 | enum | `em_cac_method_type_t` | em-msg | no Rust type; the method code is a raw `u8` on `CacMethod`; no upstream code uses the enumerators | not needed: the method octet is carried raw (0009 D2, B04) |
| 2086 | struct | `em_cac_cap_method_t` | em-msg | `CacMethod<'a>` | ported (em-msg B04) |
| 2093 | struct | `em_cac_cap_radio_t` | em-msg | `CacRadio<'a>` | ported (em-msg B04) |
| 2099 | struct | `em_cac_cap_t` | em-msg | `CacCapabilities<'a>` | ported (em-msg B04) |
| 2105 | struct | `em_metric_cltn_interval_t` | em-msg | `MetricCollectionInterval` | ported (em-msg B07) |
| 2109 | struct | `em_steering_policy_sta_t` | em-msg | no Rust type; the MAC list is a `Vec` on `SteeringPolicy` | not needed: the MAC lists are `Vec`s on `SteeringPolicy` (0009 D2, B09) |
| 2114 | struct | `em_steering_policy_radio_t` | em-msg | `RadioSteeringPolicy` | ported (em-msg B09) |
| 2121 | struct | `em_steering_policy_t` | em-msg | `SteeringPolicy`, with `RadioSteeringPolicy` | ported (em-msg B09) |
| 2128 | struct | `em_metric_rprt_policy_radio_t` | em-msg | `RadioMetricReportingPolicy` | ported (em-msg B09) |
| 2136 | struct | `em_metric_rprt_policy_t` | em-msg | `MetricReportingPolicy`, with `RadioMetricReportingPolicy` | ported (em-msg B09) |
| 2142 | struct | `em_channel_scan_rprt_policy_t` | em-msg | `ChannelScanReportingPolicy` | ported (em-msg B05) |
| 2147 | struct | `em_unsuccessful_assoc_policy_t` | em-msg | `UnsuccessfulAssociationPolicy` | ported (em-msg B09) |
| 2153 | struct | `em_bh_bss_config_t` | em-msg | `BackhaulBssConfiguration` | ported (em-msg B09) |
| 2160 | struct | `em_qos_mgmt_policy_t` | em-msg | no Rust type; the packed struct is not used on the wire (audit 270) | not needed: the wire form is `QosManagementPolicy` (0009 D2, B09) |
| 2168 | struct | `em_link_stats_alarm_cfg_t` | em-dm | `LinkStatsAlarmCfg` (`em-dm` `records/policy.rs`); em-core sends and reads it as vendor sub-id 0x04 (`em_policy_cfg.cpp:444`, `:917`) | ported |
| 2174 | struct | `em_client_filters_cfg_t` | em-dm | `ClientFiltersCfg` (`em-dm` `records/policy.rs`); em-core sends and reads it as vendor sub-id 0x05 (`em_policy_cfg.cpp:454`, `:929`) | ported |
| 2179 | struct | `em_vendor_policy_t` | em-cmd | member `vendor_policy` of `em_policy_cfg_params_t` (`em_base.h:2196`), which is an `em-cmd` row; filled at `em_policy_cfg.cpp:910-935` | planned |
| 2185 | struct | `em_policy_cfg_params_t` | em-cmd | command / request parameters | planned |
| 2199 | struct | `em_supported_service_t` | em-msg | `SupportedService` (over `ServiceList`) | ported (em-msg B03) |
| 2204 | struct | `em_searched_service_t` | em-msg | `SearchedService` (over `ServiceList`) | ported (em-msg B03) |
| 2209 | struct | `em_ap_capability_t` | em-msg | `ApCapability` | ported (em-msg B04) |
| 2218 | struct | `em_ap_radio_advanced_cap_t` | em-msg | `ApRadioAdvancedCapabilities` | ported (em-msg B04) |
| 2230 | enum | `em_media_type_t` | em-types | `MediaType`; `max` excluded | ported |
| 2243 | struct | `em_dpp_chirp_value_t` | em-msg | `DppChirpValue<'a>` | ported (em-msg B11) |
| 2250 | struct | `em_ctrl_cap_t` | em-msg | `ControllerCapability` | ported (em-msg B04) |
| 2256 | struct | `data_elem_attr_t` | em-msg | `WscAttr` (walked by `WscAttrIter`, written by `write_wsc_attr`); typed values in `WscAttribute` | ported (em-msg B00b, B12) |
| 2262 | enum | `data_elem_attr_id_t` | em-msg | `WscAttrId` (32 constants; other ids are kept as found) | ported (em-msg B12) |
| 2332 | enum | `vendor_ext_attr_id_t` | em-msg | `HaulTypeExtension` (sub-id 0x01); sub-ids 0x02 to 0x06 (`em_policy_cfg.cpp:432-454`, `:908-929`; `em_metrics.cpp:944`, `:1696`, `:2327`) are Vendor Specific TLV formats read and written by `em-core`; the row stays `em-msg` because `dm_type_classification.tsv` pins it there (0009 A11) | ported (em-msg B12: 0x01); 0x02 to 0x06 are `em-core`'s (0009 A11) |
| 2344 | enum | `em_state_t` | em-core | events; redefined natively, see D7 | planned |
| 2412 | enum | `em_cmd_type_t` | em-cmd | command / request parameters | planned |
| 2470 | struct | `em_queue_t` | do not port | C queue; replaced by std collections | n/a |
| 2477 | enum | `em_event_type_t` | em-core | events; redefined natively, see D7 | planned |
| 2488 | enum | `em_haul_type_t` | em-dm | data-model record | ported |
| 2497 | enum | `em_vap_mode_t` | em-dm | data-model record | ported |
| 2502 | struct | `em_frame_info_t` | em-transport | link-layer header / raw frame | planned |
| 2507 | struct | `em_interface_preference_t` | em-dm | data-model record | ported |
| 2512 | struct | `em_interface_t` | em-types | `Interface` | ported |
| 2518 | alias | `em_frame_event_t` | em-transport | link-layer header / raw frame | planned |
| 2520 | struct | `em_network_info_t` | em-dm | data-model record | ported |
| 2533 | struct | `ieee_1905_security_t` | em-dm | data-model record | ported |
| 2540 | struct | `em_device_id_t` | em-dm | data-model record | ported |
| 2546 | enum | `emex_product_class_t` | em-dm | data-model record | ported |
| 2550 | enum | `emex_device_role_t` | em-dm | data-model record | ported |
| 2559 | struct | `em_ext_device_info_t` | em-dm | data-model record | ported |
| 2570 | struct | `em_device_info_t` | em-dm | data-model record | ported |
| 2629 | struct | `em_ieee_1905_security_cap_t` | em-dm | data-model record; the TLV body is `Ieee1905LayerSecurityCapability` (em-msg B11) | ported (em-msg B11) |
| 2635 | struct | `em_ieee_1905_security_info_t` | em-dm | data-model record | ported |
| 2640 | struct | `em_ssid_2_vid_map_info_t` | em-dm | data-model record | ported |
| 2646 | struct | `em_network_ssid_info_t` | em-dm | data-model record | ported |
| 2665 | enum | `em_op_class_type_t` | em-dm | data-model record | ported |
| 2678 | struct | `em_op_class_id_t` | em-dm | data-model record | ported |
| 2684 | struct | `em_op_class_info_t` | em-dm | data-model record | ported |
| 2699 | struct | `em_cac_comp_info_t` | em-dm | data-model record | ported |
| 2708 | struct | `em_sta_info_t` | em-dm | data-model record | ported |
| 2769 | enum | `em_target_sta_map_t` | em-dm | data-model record: `StaMapTarget` (`model::node::sta`) | ported |
| 2775 | struct | `em_bss_id_t` | em-dm | data-model record | ported |
| 2783 | struct | `em_bss_info_t` | em-dm | data-model record | ported |
| 2839 | struct | `em_neighbor_info_t` | em-dm | data-model record | planned |
| 2849 | struct | `em_affiliated_ap_info_t` | em-dm | data-model record | ported |
| 2857 | struct | `em_ap_mld_info_t` | em-dm | data-model record | ported |
| 2870 | struct | `em_affiliated_bsta_info_t` | em-dm | data-model record | ported |
| 2876 | struct | `em_bsta_mld_info_t` | em-dm | data-model record | ported |
| 2889 | struct | `em_affiliated_sta_info_t` | em-dm | data-model record | ported |
| 2894 | struct | `em_assoc_sta_mld_info_t` | em-dm | data-model record | ported |
| 2905 | struct | `em_tid_to_link_map_info_t` | em-dm | data-model record | ported |
| 2918 | struct | `em_tid_to_link_info_t` | em-dm | data-model record | ported |
| 2926 | struct | `em_radio_id_t` | em-dm | data-model record | ported |
| 2932 | struct | `em_radio_info_t` | em-dm | data-model record | ported |
| 2977 | struct | `em_radio_cap_info_t` | em-dm | data-model record | ported |
| 2994 | struct | `em_bss_rprt_t` | em-msg | `BssConfigurationReportBss<'a>` | ported (em-msg B10) |
| 3002 | struct | `em_radio_rprt_t` | em-msg | `BssConfigurationReportRadio<'a>` | ported (em-msg B10) |
| 3008 | struct | `em_bss_config_rprt_t` | em-msg | `BssConfigurationReport<'a>` | ported (em-msg B10) |
| 3013 | struct | `em_ap_mld_ssids_t` | em-msg | no Rust type; the SSID is the raw `&[u8]` field `ApMld::ssid` | not needed: SSIDs are raw slices on `ApMld` (0009 D2, B10) |
| 3018 | struct | `em_affiliated_ap_mld_t` | em-msg | `AffiliatedAp` | ported (em-msg B10) |
| 3028 | struct | `em_ap_mld_t` | em-msg | `ApMld<'a>` | ported (em-msg B10) |
| 3044 | struct | `em_ap_mld_config_t` | em-msg | `AgentApMldConfiguration<'a>` | ported (em-msg B10) |
| 3049 | struct | `em_affiliated_bsta_mld_t` | em-msg | `AffiliatedBsta` | ported (em-msg B10) |
| 3057 | struct | `em_bsta_mld_t` | em-msg | `BackhaulStaMldConfiguration` | ported (em-msg B10) |
| 3073 | struct | `em_affiliated_sta_mld_t` | em-msg | `AffiliatedSta` | ported (em-msg B10) |
| 3079 | struct | `em_assoc_sta_mld_t` | em-msg | `AssociatedStaMldConfigurationReport` | ported (em-msg B10) |
| 3092 | struct | `em_tid_to_link_mapping_t` | em-msg | `TidToLinkMapping` | ported (em-msg B10) |
| 3108 | struct | `em_tid_to_link_map_policy_t` | em-msg | `TidToLinkMappingPolicy`, with `TidToLinkScope` | ported (em-msg B10) |
| 3119 | struct | `em_crypto_info_t` | em-core | WSC session state (note 0006 D5) | planned |
| 3140 | enum | `em_bus_event_type_t` | replaced (D7) | not ported as-is; each value maps to a native type per [bus-event inventory](bus-event-inventory.md) | n/a |
| 3215 | struct | `em_subdoc_info_t` | em-agent | OneWifi webconfig subdoc | planned |
| 3220 | struct | `em_commit_info_t` | pending (0002) | command/request/action payload; owner decided by note 0002 (em-cmd / em-core / em-orch boundary) | pending |
| 3228 | enum | `dm_orch_type_t` | em-orch | orchestration | planned |
| 3311 | struct | `em_orch_desc_t` | em-orch | orchestration | planned |
| 3316 | enum | `db_cfg_type_t` | em-db | persistence; em-dm holds the bits as `DbCfgType` in the typed dirty set (`model::dirty`) | planned |
| 3343 | struct | `em_db_cfg_param_t` | em-db | persistence; em-dm holds the set as `DirtySet` (`model::dirty`) | planned |
| 3348 | struct | `m2ctrl_radioconfig` | em-core | internal configuration helper; verify at port | planned |
| 3362 | struct | `em_e4_table_t` | em-core | internal configuration helper; verify at port | planned |
| 3371 | struct | `op_class_channel_sel` | em-core | internal configuration helper; verify at port | planned |
| 3380 | struct | `em_bus_event_type_channel_pref_query_params_t` | pending (0002) | command/request/action payload; owner decided by note 0002 (em-cmd / em-core / em-orch boundary) | pending |
| 3385 | struct | `em_bus_event_type_m2_tx_params_t` | pending (0002) | command/request/action payload; owner decided by note 0002 (em-cmd / em-core / em-orch boundary) | pending |
| 3390 | struct | `em_bus_event_type_client_assoc_params_t` | pending (0002) | command/request/action payload; owner decided by note 0002 (em-cmd / em-core / em-orch boundary) | pending |
| 3396 | struct | `em_bus_event_type_sta_cap_update_params_t` | pending (0002) | command/request/action payload; owner decided by note 0002 (em-cmd / em-core / em-orch boundary) | pending |
| 3404 | struct | `em_bus_event_type_sta_mld_update_params_t` | pending (0002) | command/request/action payload; owner decided by note 0002 (em-cmd / em-core / em-orch boundary) | pending |
| 3409 | struct | `em_bus_event_type_cfg_renew_params_t` | pending (0002) | command/request/action payload; owner decided by note 0002 (em-cmd / em-core / em-orch boundary) | pending |
| 3414 | struct | `em_cmd_args_t` | pending (0002) | command/request/action payload; owner decided by note 0002 (em-cmd / em-core / em-orch boundary) | pending |
| 3420 | enum | `em_steering_req_mode_t` | em-cmd | command / request parameters | planned |
| 3425 | struct | `em_cmd_steer_params_t` | em-cmd | command / request parameters | planned |
| 3439 | struct | `em_cmd_btm_report_params_t` | em-cmd | command / request parameters | planned |
| 3446 | struct | `em_cmd_client_assoc_params_t` | em-cmd | command / request parameters | planned |
| 3454 | struct | `em_disassoc_params_t` | em-cmd | command / request parameters | planned |
| 3462 | struct | `em_cmd_disassoc_params_t` | em-cmd | command / request parameters | planned |
| 3467 | struct | `em_cmd_ap_metrics_rprt_params_t` | em-cmd | command / request parameters | planned |
| 3475 | enum | `em_network_node_data_type_t` | em-core | network-node tree (`src/em/em_net_node.cpp`) | planned |
| 3489 | struct | `em_node_display_info_t` | em-core | network-node tree (`src/em/em_net_node.cpp`) | planned |
| 3496 | struct | `em_network_node_t` | em-core | network-node tree (`src/em/em_net_node.cpp`) | planned |
| 3506 | struct | `em_cmd_unassoc_sta_query_params_t` | em-cmd | command / request parameters | planned |
| 3510 | alias | `em_cmd_scan_params_t` | pending (0002) | command/request/action payload; owner decided by note 0002 (em-cmd / em-core / em-orch boundary) | pending |
| 3511 | alias | `em_cmd_beacon_metrics_param_t` | pending (0002) | command/request/action payload; owner decided by note 0002 (em-cmd / em-core / em-orch boundary) | pending |
| 3513 | struct | `em_cmd_params_t` | pending (0002) | command/request/action payload; owner decided by note 0002 (em-cmd / em-core / em-orch boundary) | pending |
| 3528 | struct | `em_cmd_ctx_t` | pending (0002) | command/request/action payload; owner decided by note 0002 (em-cmd / em-core / em-orch boundary) | pending |
| 3534 | struct | `em_cmd_stats_t` | pending (0002) | command/request/action payload; owner decided by note 0002 (em-cmd / em-core / em-orch boundary) | pending |
| 3540 | struct | `em_bus_event_t` | replaced (D7) | not ported as-is; replaced by `em-cmd` requests and `em-core` input events / output actions | n/a |
| 3551 | enum | `nb_req_type_e` | em-core | events; redefined natively, see D7 | planned |
| 3557 | struct | `em_nb_event_t` | em-core | events; redefined natively, see D7 | planned |
| 3579 | enum | `em_cmd_event_type_t` | em-core | events; redefined natively, see D7 | planned |
| 3586 | struct | `em_connection_status_evt_data_t` | em-core | events; redefined natively, see D7 | planned |
| 3595 | struct | `em_cmd_event_t` | em-core | events; redefined natively, see D7 | planned |
| 3600 | struct | `em_event_t` | em-core | events; redefined natively, see D7 | planned |
| 3611 | alias | `db_table_name_t` | em-db | persistence | planned |
| 3612 | alias | `db_column_name_t` | em-db | persistence | planned |
| 3631 | enum | `db_client_type_t` | em-db | persistence | planned |
| 3637 | enum | `db_data_type_t` | em-db | persistence | planned |
| 3659 | alias | `db_data_type_args_t` | em-db | persistence | planned |
| 3660 | alias | `em_advertise_cce_t` | em-easyconnect | DPP CCE advertisement flag | planned |
| 3661 | alias | `em_eapol_frame_payload_t` | do not port | zero-length trailing array; replaced by slices/`Vec<u8>` | n/a |
| 3663 | alias | `db_query_t` | do not port | fixed-size C char buffer; replaced by `String`/bounded types | n/a |
| 3664 | alias | `db_result_t` | do not port | fixed-size C char buffer; replaced by `String`/bounded types | n/a |
| 3665 | alias | `db_fmt_t` | do not port | fixed-size C char buffer; replaced by `String`/bounded types | n/a |
| 3667 | enum | `em_orch_state_t` | em-orch | orchestration | planned |
| 3676 | enum | `em_commit_target_type_t` | em-cmd | command / request parameters | planned |
| 3688 | struct | `em_commit_target_t` | em-cmd | command / request parameters | planned |
| 3694 | enum | `em_cmd_out_status_t` | em-cmd | command / request parameters | planned |
| 3706 | struct | `em_vap_info_t` | em-agent | OneWifi VAP info; verify at port | planned |
| 3711 | enum | `tag_type_t` | deferred (ieee80211, D6) | 802.11 element tags | planned |
| 3731 | struct | `ieee80211_tagvalue_t` | deferred (ieee80211, D6) | 802.11 element tags | planned |
| 3737 | enum | `em_get_sta_list_reason_t` | em-cmd | command / request parameters | planned |
| 3747 | enum | `em_get_radio_list_reason_t` | em-cmd | command / request parameters | planned |
| 3754 | enum | `em_get_channel_list_reason_t` | em-cmd | command / request parameters | planned |
| 3760 | enum | `em_policy_id_type_t` | em-dm | policy record (`dm_policy`) | ported |
| 3778 | struct | `em_policy_id_t` | em-dm | policy record (`dm_policy`) | ported |
| 3785 | enum | `em_steering_policy_type_t` | em-dm | policy record (`dm_policy`) | ported |
| 3792 | struct | `em_8021q_settings_policy_t` | em-dm | policy record (`dm_policy`) | ported |
| 3797 | struct | `em_ssid_info_traffic_separ_pol_t` | em-dm | policy record (`dm_policy`) | ported |
| 3803 | struct | `em_traffic_separation_policy_t` | em-dm | policy record (`dm_policy`) | ported |
| 3808 | struct | `em_qos_mgt_policy_t` | em-dm | policy record (`dm_policy`) | ported |
| 3815 | struct | `em_backhaul_bss_config_policy_t` | em-dm | policy record (`dm_policy`) | ported |
| 3821 | struct | `em_policy_t` | em-dm | policy record (`dm_policy`) | ported |
| 3850 | alias | `em_editor_callback_t` | em-core | network-node tree editor callback (function pointer); verify at port | planned |
| 3852 | enum | `em_cli_type_t` | em-cli | CLI parameters | planned |
| 3858 | struct | `user_data_t` | em-cli | CLI parameters | planned |
| 3863 | struct | `em_cli_params_t` | em-cli | CLI parameters | planned |
| 3869 | enum | `em_dev_test_type` | em-ctrl | device-test feature (`em_dev_test_ctrl`) | planned |
| 3875 | enum | `em_dev_test_status` | em-ctrl | device-test feature (`em_dev_test_ctrl`) | planned |
| 3883 | struct | `em_dev_test_info` | em-ctrl | device-test feature (`em_dev_test_ctrl`) | planned |
| 3894 | struct | `SecurityTypeMap` | em-dm | security-type string map; verify at port | planned |
