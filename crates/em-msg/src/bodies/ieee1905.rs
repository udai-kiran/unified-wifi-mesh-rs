// SPDX-License-Identifier: Apache-2.0

//! IEEE 1905.1 identity and neighbor TLV bodies.

use em_types::{MacAddr, MediaType, TlvType};

use super::{TlvBody, TlvBodyLenient, bounded_vec};
use crate::body::{Reader, Writer};
use crate::error::{DecodeError, EncodeError};

/// Vendor Specific, IEEE 1905.1 §6.4.2, Table 6-7; audit rows 304–305 and 304(a).
///
/// Upstream: `em_vendor_specific_t` (`em_base.h:1803`), sender
/// `create_airties_radio_capability_tlv` (`em.cpp:1330-1352`) and its caller
/// (`em_capability.cpp:184-190`); receiver `em_capability.cpp:1679-1695`.
/// When the capability is missing that caller writes an empty value; the
/// lenient decoder treats it as an absent TLV.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VendorSpecific<'a> {
    /// Three-octet vendor organizationally unique identifier.
    pub oui: [u8; 3],
    /// Vendor information, borrowed from the remainder of the value.
    pub data: &'a [u8],
}

impl<'a> TlvBody<'a> for VendorSpecific<'a> {
    const TLV_TYPE: TlvType = TlvType::VENDOR_SPECIFIC;

    fn decode_value(value: &'a [u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let oui = reader.array()?;
        let data = reader.rest();
        reader.finish()?;
        Ok(Self { oui, data })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.bytes(&self.oui);
        writer.bytes(self.data);
        Ok(writer.into_bytes())
    }
}

impl<'a> TlvBodyLenient<'a> for VendorSpecific<'a> {
    fn decode_value_lenient(value: &'a [u8]) -> Result<Option<Self>, DecodeError> {
        if value.is_empty() {
            Ok(None)
        } else {
            Self::decode_value(value).map(Some)
        }
    }
}

/// AL MAC Address, IEEE 1905.1 §6.4.3, Table 6-8; audit row 307.
///
/// Upstream sender `em_discovery.cpp:125-132` and receiver
/// `em_msg.cpp:71-84` use the six-byte address directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AlMacAddress {
    /// AL MAC address of the transmitting device.
    pub mac: MacAddr,
}

/// MAC Address, IEEE 1905.1 §6.4.4, Table 6-9; audit row 309.
///
/// Upstream sender `em_discovery.cpp:134-141` uses the six-byte address directly;
/// there is no upstream receiver at the pin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MacAddress {
    /// MAC address of the transmitting interface.
    pub mac: MacAddr,
}

// The two distinct public bodies share the same six-octet layout.
macro_rules! mac_body {
    ($name:ident, $tlv:expr) => {
        impl TlvBody<'_> for $name {
            const TLV_TYPE: TlvType = $tlv;

            fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
                let mut reader = Reader::new(value);
                let mac = reader.mac()?;
                reader.finish()?;
                Ok(Self { mac })
            }

            fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
                let mut writer = Writer::new();
                writer.mac(self.mac);
                Ok(writer.into_bytes())
            }
        }
    };
}

mac_body!(AlMacAddress, TlvType::AL_MAC_ADDRESS);
mac_body!(MacAddress, TlvType::MAC_ADDRESS);

/// A local interface in IEEE 1905.1 §6.4.5, Table 6-10; audit rows 311–314.
///
/// Upstream structs `em_local_interface_t` and `em_media_spec_data_t` are at
/// `em_base.h:1571` and `:1561`; sender `em_configuration.cpp:1171-1213`;
/// no upstream receiver at the pin. Media information is carried as found,
/// independent of the media type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalInterface<'a> {
    /// Local interface MAC address.
    pub mac: MacAddr,
    /// Open IEEE 1905.1 media type.
    pub media_type: MediaType,
    /// Length-prefixed media-specific bytes, borrowed from the value.
    pub media_info: &'a [u8],
}

/// Device Information, IEEE 1905.1 §6.4.5, Table 6-10; audit rows 311–314.
///
/// Upstream struct `em_device_info_type_t` is at `em_base.h:1576`; sender
/// `create_device_info_type_tlv` is at `em_configuration.cpp:1171-1213`; no
/// upstream receiver at the pin. A zero interface count is accepted under
/// design note 0009 D2.
#[derive(Debug, PartialEq, Eq)]
pub struct DeviceInformation<'a> {
    /// AL MAC address of the device.
    pub al_mac: MacAddr,
    /// Local interfaces, in wire order.
    pub interfaces: Vec<LocalInterface<'a>>,
}

impl<'a> TlvBody<'a> for DeviceInformation<'a> {
    const TLV_TYPE: TlvType = TlvType::DEVICE_INFORMATION;

    fn decode_value(value: &'a [u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let al_mac = reader.mac()?;
        let count = usize::from(reader.u8()?);
        // Each interface takes at least 6 + 2 + 1 bytes.
        let mut interfaces = bounded_vec(count, reader.remaining(), 9);
        for _ in 0..count {
            let mac = reader.mac()?;
            let media_type = MediaType::from(reader.be_u16()?);
            let len = usize::from(reader.u8()?);
            let media_info = reader.bytes(len)?;
            interfaces.push(LocalInterface {
                mac,
                media_type,
                media_info,
            });
        }
        reader.finish()?;
        Ok(Self { al_mac, interfaces })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.mac(self.al_mac);
        writer.count_u8(self.interfaces.len())?;
        for interface in &self.interfaces {
            writer.mac(interface.mac);
            writer.be_u16(u16::from(interface.media_type));
            writer.count_u8(interface.media_info.len())?;
            writer.bytes(interface.media_info);
        }
        Ok(writer.into_bytes())
    }
}

/// One bridging tuple in IEEE 1905.1 §6.4.6, Table 6-11; audit row 315.
///
/// Upstream structs `em_bridge_tuple_entry_t` and `em_bridge_tuple_mac_entry_t`
/// are at `em_base.h:1586` and `:1582`; no upstream sender or receiver at the pin.
#[derive(Debug, PartialEq, Eq)]
pub struct BridgingTuple {
    /// Interface MAC addresses in the tuple, in wire order.
    pub macs: Vec<MacAddr>,
}

/// Device Bridging Capability, IEEE 1905.1 §6.4.6, Table 6-11; audit row 315.
///
/// Upstream struct `em_device_bridge_cap_t` is at `em_base.h:1591`; no upstream
/// sender or receiver at the pin. Zero tuples and zero MACs are accepted.
#[derive(Debug, PartialEq, Eq)]
pub struct DeviceBridgingCapability {
    /// Bridging tuples, in wire order.
    pub tuples: Vec<BridgingTuple>,
}

impl TlvBody<'_> for DeviceBridgingCapability {
    const TLV_TYPE: TlvType = TlvType::DEVICE_BRIDGING_CAPABILITY;

    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let count = usize::from(reader.u8()?);
        // Each tuple contains at least its one-byte MAC count.
        let mut tuples = bounded_vec(count, reader.remaining(), 1);
        for _ in 0..count {
            let mac_count = usize::from(reader.u8()?);
            let mut macs = bounded_vec(mac_count, reader.remaining(), 6);
            for _ in 0..mac_count {
                macs.push(reader.mac()?);
            }
            tuples.push(BridgingTuple { macs });
        }
        reader.finish()?;
        Ok(Self { tuples })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.count_u8(self.tuples.len())?;
        for tuple in &self.tuples {
            writer.count_u8(tuple.macs.len())?;
            for &mac in &tuple.macs {
                writer.mac(mac);
            }
        }
        Ok(writer.into_bytes())
    }
}

/// Non-1905 Neighbor Device List, IEEE 1905.1 §6.4.8, Table 6-14; audit row 316.
///
/// Upstream structs `em_non_1905_neigh_device_list_t` and
/// `em_non_1905_neigh_entry_t` are at `em_base.h:1600` and `:1596`; no upstream
/// sender or receiver at the pin. The wire has no neighbor count.
#[derive(Debug, PartialEq, Eq)]
pub struct Non1905NeighborDeviceList {
    /// MAC address of the local interface.
    pub local_mac: MacAddr,
    /// Non-1905 neighbor MAC addresses, in wire order.
    pub neighbors: Vec<MacAddr>,
}

impl TlvBody<'_> for Non1905NeighborDeviceList {
    const TLV_TYPE: TlvType = TlvType::NON_1905_NEIGHBOR_DEVICE_LIST;

    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let local_mac = reader.mac()?;
        let mut neighbors = bounded_vec(reader.remaining(), reader.remaining(), 6);
        while reader.remaining() != 0 {
            neighbors.push(reader.mac()?);
        }
        reader.finish()?;
        Ok(Self {
            local_mac,
            neighbors,
        })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.mac(self.local_mac);
        for &mac in &self.neighbors {
            writer.mac(mac);
        }
        Ok(writer.into_bytes())
    }
}

/// A 1905.1 neighbor in IEEE 1905.1 §6.4.9, Table 6-15; audit rows 317–318.
///
/// Upstream struct `em_neigh_entry_t` is at `em_base.h:1606`; no upstream
/// sender or receiver at the pin. The reserved flag bits are discarded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ieee1905Neighbor {
    /// AL MAC address of the neighbor.
    pub al_mac: MacAddr,
    /// Whether at least one IEEE 802.1 bridge exists on the link.
    pub bridge: bool,
}

/// 1905.1 Neighbor Device, IEEE 1905.1 §6.4.9, Table 6-15; audit rows 317–318.
///
/// Upstream structs `em_neigh_device_list_t` and `em_neigh_entry_t` are at
/// `em_base.h:1611` and `:1606`; no upstream sender or receiver at the pin.
/// The wire has no neighbor count; reserved flag bits are encoded as zero.
#[derive(Debug, PartialEq, Eq)]
pub struct Ieee1905NeighborDevice {
    /// MAC address of the local interface.
    pub local_mac: MacAddr,
    /// 1905.1 neighbors, in wire order.
    pub neighbors: Vec<Ieee1905Neighbor>,
}

impl TlvBody<'_> for Ieee1905NeighborDevice {
    const TLV_TYPE: TlvType = TlvType::IEEE1905_NEIGHBOR_DEVICE_LIST;

    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let local_mac = reader.mac()?;
        let mut neighbors = bounded_vec(reader.remaining(), reader.remaining(), 7);
        while reader.remaining() != 0 {
            let al_mac = reader.mac()?;
            let bridge = reader.u8()? & 0x80 != 0;
            neighbors.push(Ieee1905Neighbor { al_mac, bridge });
        }
        reader.finish()?;
        Ok(Self {
            local_mac,
            neighbors,
        })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.mac(self.local_mac);
        for neighbor in &self.neighbors {
            writer.mac(neighbor.al_mac);
            writer.u8(u8::from(neighbor.bridge) << 7);
        }
        Ok(writer.into_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::body_fixtures::{self, Key, Row};
    use crate::body_testkit::{Field, truncated_in};
    use crate::tlv::Tlv;
    use core::fmt::Debug;
    use core::mem::size_of;

    const GROUP: &str = "ieee1905";
    const V_GOLD: Key = ("0x0b", "golden");
    const V_OUI: Key = ("0x0b", "oui-only");
    const A_GOLD: Key = ("0x01", "golden");
    const M_GOLD: Key = ("0x02", "golden");
    const D_GOLD: Key = ("0x03", "golden");
    const D_ZERO: Key = ("0x03", "zero-interfaces");
    const D_ETH: Key = ("0x03", "info-absent-ethernet");
    const D_WIFI: Key = ("0x03", "info-present-80211");
    const D_1901: Key = ("0x03", "info-present-1901");
    const D_THREE: Key = ("0x03", "three-interfaces");
    const D_EMPTY: Key = ("0x03", "media-80211-info-empty");
    const D_0108: Key = ("0x03", "media-0108-info-10");
    const D_UNKNOWN: Key = ("0x03", "media-unknown");
    const B_GOLD: Key = ("0x04", "golden");
    const B_ZERO: Key = ("0x04", "zero-tuples");
    const B_EMPTY_TUPLE: Key = ("0x04", "tuple-zero-macs");
    const N_GOLD: Key = ("0x06", "golden");
    const N_ZERO: Key = ("0x06", "no-neighbors");
    const I_GOLD: Key = ("0x07", "golden");
    const I_ZERO: Key = ("0x07", "no-neighbors");
    const I_CLEAR: Key = ("0x07", "bridge-clear");
    const I_RESERVED: Key = ("0x07", "reserved-bits-set");

    // Each key here is used by a golden, alternate, or reserved-bit body check below.
    const SPEC_CASES: &[Key] = &[
        V_GOLD,
        V_OUI,
        A_GOLD,
        M_GOLD,
        D_GOLD,
        D_ZERO,
        D_ETH,
        D_WIFI,
        D_1901,
        D_THREE,
        D_EMPTY,
        D_0108,
        D_UNKNOWN,
        B_GOLD,
        B_ZERO,
        B_EMPTY_TUPLE,
        N_GOLD,
        N_ZERO,
        I_GOLD,
        I_ZERO,
        I_CLEAR,
        I_RESERVED,
    ];

    const fn truncated(needed: usize, remaining: usize) -> DecodeError {
        DecodeError::Truncated { needed, remaining }
    }

    const TRAILING: DecodeError = DecodeError::TrailingBytes { remaining: 1 };
    // Every entry is tested through both decoding entry points and checked against its metadata.
    const INVALID_CASES: &[(Key, DecodeError)] = &[
        (("0x0b", "cut-oui"), truncated(3, 2)),
        (("0x0b", "empty"), truncated(3, 0)),
        (("0x01", "cut-mac"), truncated(6, 5)),
        (("0x01", "seven-octets"), TRAILING),
        (("0x01", "empty"), truncated(6, 0)),
        (("0x02", "cut-mac"), truncated(6, 5)),
        (("0x02", "seven-octets"), TRAILING),
        (("0x02", "empty"), truncated(6, 0)),
        (("0x03", "cut-al-mac"), truncated(6, 5)),
        (("0x03", "missing-count"), truncated(1, 0)),
        (("0x03", "count-overstates"), truncated(6, 0)),
        (("0x03", "cut-iface-mac"), truncated(6, 4)),
        (("0x03", "cut-media-type"), truncated(2, 1)),
        (("0x03", "info-length-overstates"), truncated(10, 6)),
        (("0x03", "trailing-bytes"), TRAILING),
        (("0x04", "empty"), truncated(1, 0)),
        (("0x04", "tuples-overstate"), truncated(1, 0)),
        (("0x04", "tuple-count-overstates"), truncated(6, 0)),
        (("0x04", "cut-mac"), truncated(6, 5)),
        (("0x04", "trailing-bytes"), TRAILING),
        (("0x06", "empty"), truncated(6, 0)),
        (("0x06", "cut-local-mac"), truncated(6, 4)),
        (("0x06", "partial-neighbor"), truncated(6, 3)),
        (("0x07", "empty"), truncated(6, 0)),
        (("0x07", "cut-local-mac"), truncated(6, 4)),
        (("0x07", "partial-neighbor"), truncated(1, 0)),
    ];

    const AL: MacAddr = MacAddr::new([0x02, 0x11, 0x22, 0x33, 0x44, 0x55]);
    const MAC1: MacAddr = MacAddr::new([0x02, 0xa0, 0xb1, 0xc2, 0xd3, 0xe1]);
    const MAC2: MacAddr = MacAddr::new([0x02, 0xa0, 0xb1, 0xc2, 0xd3, 0xe2]);
    const MAC3: MacAddr = MacAddr::new([0x02, 0xa0, 0xb1, 0xc2, 0xd3, 0xe3]);
    const NON1: MacAddr = MacAddr::new([0x3c, 0x1d, 0x2e, 0x3f, 0x4a, 0x5b]);
    const NON2: MacAddr = MacAddr::new([0x7a, 0x6b, 0x5c, 0x4d, 0x3e, 0x2f]);
    const NEIGH1: MacAddr = MacAddr::new([0x02, 0x66, 0x77, 0x88, 0x99, 0xaa]);
    const NEIGH2: MacAddr = MacAddr::new([0x02, 0xbb, 0xcc, 0xdd, 0xee, 0xff]);
    const WIFI_INFO: &[u8] = &[0x02, 0xf0, 0xe1, 0xd2, 0xc3, 0xb4, 0x40, 0x02, 0x2a, 0x9b];
    const WIFI_INFO_ALT: &[u8] = &[0x02, 0xf0, 0xe1, 0xd2, 0xc3, 0xb4, 0x90, 0x01, 0x07, 0x00];
    const INFO_1901: &[u8] = &[0x0a, 0x1b, 0x2c, 0x3d, 0x4e, 0x5f, 0x60];
    const TOO_LONG: EncodeError = EncodeError::CountTooLarge { len: 256, max: 255 };

    fn fixture(key: Key) -> Vec<u8> {
        body_fixtures::value(GROUP, key)
    }

    const fn interface(mac: MacAddr, media: u16, info: &'static [u8]) -> LocalInterface<'static> {
        LocalInterface {
            mac,
            media_type: MediaType(media),
            media_info: info,
        }
    }

    fn device(interfaces: Vec<LocalInterface<'static>>) -> DeviceInformation<'static> {
        DeviceInformation {
            al_mac: AL,
            interfaces,
        }
    }

    fn device_golden() -> DeviceInformation<'static> {
        device(vec![
            interface(MAC1, 0x0105, WIFI_INFO),
            interface(MAC2, 0x0001, &[]),
        ])
    }

    fn bridging_golden() -> DeviceBridgingCapability {
        DeviceBridgingCapability {
            tuples: vec![
                BridgingTuple {
                    macs: vec![MAC1, MAC2],
                },
                BridgingTuple { macs: vec![MAC3] },
            ],
        }
    }

    fn ieee_golden() -> Ieee1905NeighborDevice {
        Ieee1905NeighborDevice {
            local_mac: MAC1,
            neighbors: vec![
                Ieee1905Neighbor {
                    al_mac: NEIGH1,
                    bridge: true,
                },
                Ieee1905Neighbor {
                    al_mac: NEIGH2,
                    bridge: false,
                },
            ],
        }
    }

    const fn field(offset: usize, width: usize) -> Field {
        Field { offset, width }
    }

    body_checks! {
        mod vendor_checks;
        body: VendorSpecific<'a>;
        golden: &fixture(V_GOLD), VendorSpecific { oui: [0x50, 0x6f, 0x9a], data: &[1, 2, 3, 4, 0xa5] };
        lenient;
        prefixes: |len| len >= 3, truncated_in(&[field(0, 3)]);
        alternates: [(&fixture(V_OUI), VendorSpecific { oui: [0x50, 0x6f, 0x9a], data: &[] }, &fixture(V_OUI))];
        lenient_absent: [&fixture(("0x0b", "empty"))];
        lenient_rejects: [
            (&fixture(("0x0b", "cut-oui")), truncated(3, 2)),
            (&fixture(("0x0b", "cut-oui"))[..1], truncated(3, 1)),
        ];
    }

    body_checks! {
        mod al_checks;
        body: AlMacAddress;
        golden: &fixture(A_GOLD), AlMacAddress { mac: AL };
        prefixes: |_| false, truncated_in(&[field(0, 6)]);
        trailing_byte: &fixture(A_GOLD);
    }

    body_checks! {
        mod mac_checks;
        body: MacAddress;
        golden: &fixture(M_GOLD), MacAddress { mac: MAC1 };
        prefixes: |_| false, truncated_in(&[field(0, 6)]);
        trailing_byte: &fixture(M_GOLD);
    }

    body_checks! {
        mod device_checks;
        body: DeviceInformation<'a>;
        golden: &fixture(D_GOLD), device_golden();
        prefixes: |_| false, truncated_in(&[
            field(0, 6), field(6, 1), field(7, 6), field(13, 2), field(15, 1),
            field(16, 10), field(26, 6), field(32, 2), field(34, 1)
        ]);
        trailing_byte: &fixture(D_GOLD);
        count_too_large: device(vec![interface(MAC1, 0x0105, &[]); 256]), TOO_LONG;
        alternates: [
            (&fixture(D_ZERO), device(vec![]), &fixture(D_ZERO)),
            (&fixture(D_ETH), device(vec![interface(MAC2, 0x0001, &[])]), &fixture(D_ETH)),
            (&fixture(D_WIFI), device(vec![interface(MAC3, 0x0104, WIFI_INFO_ALT)]), &fixture(D_WIFI)),
            (&fixture(D_1901), device(vec![interface(MAC3, 0x0201, INFO_1901)]), &fixture(D_1901)),
            (&fixture(D_THREE), device(vec![interface(MAC1, 0x0105, WIFI_INFO), interface(MAC2, 0x0001, &[]), interface(MAC3, 0x0201, INFO_1901)]), &fixture(D_THREE)),
            (&fixture(D_EMPTY), device(vec![interface(MAC1, 0x0105, &[])]), &fixture(D_EMPTY)),
            (&fixture(D_0108), device(vec![interface(MAC1, 0x0108, WIFI_INFO)]), &fixture(D_0108)),
            (&fixture(D_UNKNOWN), device(vec![interface(MAC2, 0xffff, &[])]), &fixture(D_UNKNOWN)),
        ];
    }

    body_checks! {
        mod bridging_checks;
        body: DeviceBridgingCapability;
        golden: &fixture(B_GOLD), bridging_golden();
        prefixes: |_| false, truncated_in(&[
            field(0, 1), field(1, 1), field(2, 6), field(8, 6), field(14, 1), field(15, 6)
        ]);
        trailing_byte: &fixture(B_GOLD);
        count_too_large: DeviceBridgingCapability { tuples: (0..256).map(|_| BridgingTuple { macs: vec![] }).collect() }, TOO_LONG;
        alternates: [
            (&fixture(B_ZERO), DeviceBridgingCapability { tuples: vec![] }, &fixture(B_ZERO)),
            (&fixture(B_EMPTY_TUPLE), DeviceBridgingCapability { tuples: vec![BridgingTuple { macs: vec![] }, BridgingTuple { macs: vec![MAC3] }] }, &fixture(B_EMPTY_TUPLE)),
        ];
    }

    body_checks! {
        mod non1905_checks;
        body: Non1905NeighborDeviceList;
        golden: &fixture(N_GOLD), Non1905NeighborDeviceList { local_mac: MAC1, neighbors: vec![NON1, NON2] };
        prefixes: |len| len == 6 || len == 12, truncated_in(&[field(0, 6), field(6, 6), field(12, 6)]);
        alternates: [(&fixture(N_ZERO), Non1905NeighborDeviceList { local_mac: MAC1, neighbors: vec![] }, &fixture(N_ZERO))];
    }

    body_checks! {
        mod ieee1905_checks;
        body: Ieee1905NeighborDevice;
        golden: &fixture(I_GOLD), ieee_golden();
        prefixes: |len| len == 6 || len == 13, truncated_in(&[
            field(0, 6), field(6, 6), field(12, 1), field(13, 6), field(19, 1)
        ]);
        reserved_bits: [(&fixture(I_RESERVED), ieee_golden(), &fixture(I_GOLD))];
        alternates: [
            (&fixture(I_ZERO), Ieee1905NeighborDevice { local_mac: MAC1, neighbors: vec![] }, &fixture(I_ZERO)),
            (&fixture(I_CLEAR), Ieee1905NeighborDevice { local_mac: MAC2, neighbors: vec![Ieee1905Neighbor { al_mac: NEIGH2, bridge: false }] }, &fixture(I_CLEAR)),
        ];
    }

    fn check_invalid<'a, B: TlvBody<'a> + Debug>(value: &'a [u8], expected: DecodeError, key: Key) {
        crate::body_testkit::assert_passes(crate::body_testkit::check_tlv_column::<B>(key.0));
        assert_eq!(
            B::decode_value(value).err(),
            Some(expected),
            "direct decode of {key:?}"
        );
        let tlv = Tlv {
            tlv_type: B::TLV_TYPE,
            value,
        };
        assert_eq!(
            tlv.decode::<B>().err(),
            Some(expected),
            "TLV decode of {key:?}"
        );
    }

    #[test]
    fn every_invalid_fixture_has_its_exact_error_on_both_paths() {
        for &(key, expected) in INVALID_CASES {
            let fields = body_fixtures::fields(GROUP, key);
            assert!(
                fields.split(';').any(|field| field == "kind=invalid"),
                "{key:?}"
            );
            let kind = match expected {
                DecodeError::Truncated { .. } => "error=Truncated",
                DecodeError::TrailingBytes { .. } => "error=TrailingBytes",
                _ => panic!("unexpected error kind for {key:?}"),
            };
            assert!(
                fields.split(';').any(|field| field == kind),
                "{key:?}: {fields}"
            );
            let value = fixture(key);
            match key.0 {
                "0x0b" => check_invalid::<VendorSpecific<'_>>(&value, expected, key),
                "0x01" => check_invalid::<AlMacAddress>(&value, expected, key),
                "0x02" => check_invalid::<MacAddress>(&value, expected, key),
                "0x03" => check_invalid::<DeviceInformation<'_>>(&value, expected, key),
                "0x04" => check_invalid::<DeviceBridgingCapability>(&value, expected, key),
                "0x06" => check_invalid::<Non1905NeighborDeviceList>(&value, expected, key),
                "0x07" => check_invalid::<Ieee1905NeighborDevice>(&value, expected, key),
                _ => panic!("unknown fixture TLV for {key:?}"),
            }
        }
    }

    /// Invalid rows that the lenient form accepts on purpose: upstream's empty Vendor Specific
    /// value is `Ok(None)`.
    const LENIENT_INPUTS: &[Key] = &[("0x0b", "empty")];

    fn dispatch_row(row: &Row<'_>, bytes: &[u8], canonical: Option<&[u8]>) -> Result<(), String> {
        use crate::body_testkit::{sweep_row, sweep_row_lenient};
        match row.tlv {
            "0x0b" => {
                sweep_row_lenient::<VendorSpecific<'_>>(row, bytes, canonical, LENIENT_INPUTS)
            }
            "0x01" => sweep_row::<AlMacAddress>(row, bytes, canonical),
            "0x02" => sweep_row::<MacAddress>(row, bytes, canonical),
            "0x03" => sweep_row::<DeviceInformation<'_>>(row, bytes, canonical),
            "0x04" => sweep_row::<DeviceBridgingCapability>(row, bytes, canonical),
            "0x06" => sweep_row::<Non1905NeighborDeviceList>(row, bytes, canonical),
            "0x07" => sweep_row::<Ieee1905NeighborDevice>(row, bytes, canonical),
            other => Err(format!("no body for TLV {other}")),
        }
    }

    #[test]
    fn every_fixture_row_decodes_and_encodes_as_its_kind_says() {
        crate::body_testkit::sweep_group(GROUP, &[], dispatch_row);
    }

    #[test]
    fn every_fixture_row_is_used() {
        let used: Vec<Key> = SPEC_CASES
            .iter()
            .copied()
            .chain(INVALID_CASES.iter().map(|&(key, _)| key))
            .collect();
        body_fixtures::assert_all_used(GROUP, &used);
        for &key in SPEC_CASES {
            assert!(
                !body_fixtures::fields(GROUP, key)
                    .split(';')
                    .any(|field| field == "kind=invalid"),
                "{key:?}"
            );
        }
    }

    #[test]
    fn nested_counts_and_media_lengths_are_checked() {
        let too_many_macs = DeviceBridgingCapability {
            tuples: vec![BridgingTuple {
                macs: vec![MAC1; 256],
            }],
        };
        assert_eq!(too_many_macs.encode_value(), Err(TOO_LONG));
        let long_info = [0_u8; 256];
        let too_much_info = DeviceInformation {
            al_mac: AL,
            interfaces: vec![LocalInterface {
                mac: MAC1,
                media_type: MediaType(0xffff),
                media_info: &long_info,
            }],
        };
        assert_eq!(too_much_info.encode_value(), Err(TOO_LONG));

        let max_info = [0x5a_u8; 255];
        let mut interfaces = vec![interface(MAC1, 0xffff, &[]); 255];
        interfaces[0].media_info = &max_info;
        let max_device = DeviceInformation {
            al_mac: AL,
            interfaces,
        };
        let encoded = max_device.encode_value().unwrap();
        assert!(crate::body_testkit::build_message(&max_device).is_ok());
        assert_eq!(DeviceInformation::decode_value(&encoded), Ok(max_device));
        let mut tuples: Vec<_> = (0..255).map(|_| BridgingTuple { macs: vec![] }).collect();
        tuples[0].macs = vec![MAC2; 255];
        let max_bridges = DeviceBridgingCapability { tuples };
        let encoded = max_bridges.encode_value().unwrap();
        assert!(crate::body_testkit::build_message(&max_bridges).is_ok());
        assert_eq!(
            DeviceBridgingCapability::decode_value(&encoded),
            Ok(max_bridges)
        );
    }

    #[test]
    fn maximal_untrusted_counts_in_short_values_are_truncated() {
        let mut no_interfaces = AL.octets().to_vec();
        no_interfaces.push(255);
        assert_eq!(
            DeviceInformation::decode_value(&no_interfaces),
            Err(truncated(6, 0))
        );

        let mut no_media_info = AL.octets().to_vec();
        no_media_info.push(1);
        no_media_info.extend_from_slice(&MAC1.octets());
        no_media_info.extend_from_slice(&[0x01, 0x05, 255]);
        assert_eq!(
            DeviceInformation::decode_value(&no_media_info),
            Err(truncated(255, 0))
        );

        assert_eq!(
            DeviceBridgingCapability::decode_value(&[255]),
            Err(truncated(1, 0))
        );
        assert_eq!(
            DeviceBridgingCapability::decode_value(&[1, 255]),
            Err(truncated(6, 0))
        );
    }

    #[test]
    fn decoded_variable_bytes_borrow_the_input() {
        let vendor = fixture(V_GOLD);
        let decoded = VendorSpecific::decode_value(&vendor).unwrap();
        assert!(core::ptr::eq(decoded.data.as_ptr(), vendor[3..].as_ptr()));
        let device = fixture(D_GOLD);
        let decoded = DeviceInformation::decode_value(&device).unwrap();
        assert!(core::ptr::eq(
            decoded.interfaces[0].media_info.as_ptr(),
            device[16..].as_ptr()
        ));
    }

    #[test]
    fn body_and_record_sizes_have_bounded_stack_cost() {
        let word = size_of::<usize>();
        assert!(size_of::<VendorSpecific<'_>>() <= 4 * word);
        assert!(size_of::<AlMacAddress>() <= 6);
        assert!(size_of::<MacAddress>() <= 6);
        assert!(size_of::<LocalInterface<'_>>() <= 4 * word);
        assert!(size_of::<DeviceInformation<'_>>() <= 5 * word);
        assert!(size_of::<BridgingTuple>() <= 3 * word);
        assert!(size_of::<DeviceBridgingCapability>() <= 3 * word);
        assert!(size_of::<Non1905NeighborDeviceList>() <= 5 * word);
        assert!(size_of::<Ieee1905Neighbor>() <= 8);
        assert!(size_of::<Ieee1905NeighborDevice>() <= 5 * word);
    }
}
