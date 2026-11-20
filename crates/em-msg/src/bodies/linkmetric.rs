// SPDX-License-Identifier: Apache-2.0

//! IEEE 1905.1 link metric and push button TLV bodies.

use em_types::{MacAddr, MediaType, TlvType};

use super::{TlvBody, bounded_vec};
use crate::body::{Reader, Writer};
use crate::error::{DecodeError, EncodeError};

/// Link Metric Query, IEEE 1905.1 §6.4.10, Table 6-16; audit rows 319–321.
///
/// Spec-only, no upstream reference sender or receiver: the upstream
/// `em_link_metric_query_t` (`em_base.h:1830-1834`) is not used for the wire.
/// Note 0009 D3, amended by A4 (user decision 2026-11-20): selector 0 never
/// carries a MAC, and encoding selector 0 always writes two octets. Decoding
/// accepts an eight-octet value with selector 0 but ignores its MAC octets, like
/// reserved octets. Every other selector requires the MAC. The constructor makes
/// both a missing required MAC and a selector-0 MAC unrepresentable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinkMetricQuery {
    selector: u8,
    neighbor: Option<MacAddr>,
    /// Requested metrics code, including reserved values.
    pub metrics: u8,
}

impl LinkMetricQuery {
    /// Creates a query, returning `None` if a nonzero selector has no MAC or
    /// if selector 0 is given one.
    #[must_use]
    pub const fn new(selector: u8, neighbor: Option<MacAddr>, metrics: u8) -> Option<Self> {
        if (selector == 0) == neighbor.is_some() {
            None
        } else {
            Some(Self {
                selector,
                neighbor,
                metrics,
            })
        }
    }

    /// Returns the selector code, including reserved values.
    #[must_use]
    pub const fn selector(&self) -> u8 {
        self.selector
    }

    /// Returns the neighbor MAC, or `None` for selector 0, which never carries one.
    #[must_use]
    pub const fn neighbor(&self) -> Option<MacAddr> {
        self.neighbor
    }
}

impl TlvBody<'_> for LinkMetricQuery {
    const TLV_TYPE: TlvType = TlvType::LINK_METRIC_QUERY;

    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let selector = reader.u8()?;
        let neighbor = if selector == 0 {
            if reader.remaining() > 1 {
                // The eight-octet form: the MAC is read and dropped.
                reader.mac()?;
            }
            None
        } else {
            Some(reader.mac()?)
        };
        let metrics = reader.u8()?;
        reader.finish()?;
        Ok(Self {
            selector,
            neighbor,
            metrics,
        })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.u8(self.selector);
        if let Some(neighbor) = self.neighbor {
            writer.mac(neighbor);
        }
        writer.u8(self.metrics);
        Ok(writer.into_bytes())
    }
}

/// One transmitter interface pair, IEEE 1905.1 §6.4.11, Tables 6-17 and 6-18;
/// audit rows 322–324.
///
/// Spec-only, no upstream reference sender or receiver. The unused upstream
/// entry struct is `em_tx_link_metric_entry_t` (`em_base.h:1836-1846`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransmitterLinkMetricPair {
    /// Local interface MAC address.
    pub local_interface: MacAddr,
    /// Neighbor interface MAC address.
    pub neighbor_interface: MacAddr,
    /// Open interface media type.
    pub media_type: MediaType,
    /// Bridge flag, including reserved values.
    pub bridge_flag: u8,
    /// Transmit packet errors.
    pub packet_errors: u32,
    /// Number of transmitted packets.
    pub transmitted_packets: u32,
    /// MAC throughput capacity.
    pub mac_throughput_capacity: u16,
    /// Link availability, carried raw even above 100.
    pub link_availability: u16,
    /// PHY rate.
    pub phy_rate: u16,
}

/// Transmitter Link Metric, IEEE 1905.1 §6.4.11, Tables 6-17 and 6-18;
/// audit rows 322–324.
///
/// Spec-only, no upstream reference sender or receiver. The upstream
/// `em_tx_link_metric_t` (`em_base.h:1848-1853`) has a count and pointer
/// rather than the spec's inline records. The constructor requires at least
/// one pair; the private list preserves that invariant for encoding.
#[derive(Debug, PartialEq, Eq)]
pub struct TransmitterLinkMetric {
    /// AL MAC address of the transmitting device.
    pub local_al: MacAddr,
    /// AL MAC address of its neighbor.
    pub neighbor_al: MacAddr,
    pairs: Vec<TransmitterLinkMetricPair>,
}

impl TransmitterLinkMetric {
    /// Creates a body, returning `None` if `pairs` is empty.
    #[must_use]
    pub fn new(
        local_al: MacAddr,
        neighbor_al: MacAddr,
        pairs: Vec<TransmitterLinkMetricPair>,
    ) -> Option<Self> {
        if pairs.is_empty() {
            None
        } else {
            Some(Self {
                local_al,
                neighbor_al,
                pairs,
            })
        }
    }

    /// Returns the nonempty interface pairs in wire order.
    #[must_use]
    pub fn pairs(&self) -> &[TransmitterLinkMetricPair] {
        &self.pairs
    }

    /// Returns the pairs for modification without changing their count.
    pub fn pairs_mut(&mut self) -> &mut [TransmitterLinkMetricPair] {
        &mut self.pairs
    }

    /// Appends an interface pair.
    pub fn push_pair(&mut self, pair: TransmitterLinkMetricPair) {
        self.pairs.push(pair);
    }
}

impl TlvBody<'_> for TransmitterLinkMetric {
    const TLV_TYPE: TlvType = TlvType::TRANSMITTER_LINK_METRIC;

    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let local_al = reader.mac()?;
        let neighbor_al = reader.mac()?;
        if reader.remaining() == 0 {
            return Err(DecodeError::InvalidLength { len: value.len() });
        }
        let mut pairs = bounded_vec(reader.remaining(), reader.remaining(), 29);
        while reader.remaining() != 0 {
            pairs.push(TransmitterLinkMetricPair {
                local_interface: reader.mac()?,
                neighbor_interface: reader.mac()?,
                media_type: MediaType::from(reader.be_u16()?),
                bridge_flag: reader.u8()?,
                packet_errors: reader.be_u32()?,
                transmitted_packets: reader.be_u32()?,
                mac_throughput_capacity: reader.be_u16()?,
                link_availability: reader.be_u16()?,
                phy_rate: reader.be_u16()?,
            });
        }
        reader.finish()?;
        Ok(Self {
            local_al,
            neighbor_al,
            pairs,
        })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.mac(self.local_al);
        writer.mac(self.neighbor_al);
        for pair in &self.pairs {
            writer.mac(pair.local_interface);
            writer.mac(pair.neighbor_interface);
            writer.be_u16(u16::from(pair.media_type));
            writer.u8(pair.bridge_flag);
            writer.be_u32(pair.packet_errors);
            writer.be_u32(pair.transmitted_packets);
            writer.be_u16(pair.mac_throughput_capacity);
            writer.be_u16(pair.link_availability);
            writer.be_u16(pair.phy_rate);
        }
        Ok(writer.into_bytes())
    }
}

/// One receiver interface pair, IEEE 1905.1 §6.4.12, Tables 6-19 and 6-20;
/// audit rows 325–327.
///
/// Spec-only, no upstream reference sender or receiver. The unused upstream
/// entry struct is `em_rx_link_metric_entry_t` (`em_base.h:1855-1862`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReceiverLinkMetricPair {
    /// Local interface MAC address.
    pub local_interface: MacAddr,
    /// Neighbor interface MAC address.
    pub neighbor_interface: MacAddr,
    /// Open interface media type.
    pub media_type: MediaType,
    /// Receive packet errors.
    pub packet_errors: u32,
    /// Number of received packets.
    pub received_packets: u32,
    /// Received signal strength indicator, as an octet.
    pub rssi: u8,
}

/// Receiver Link Metric, IEEE 1905.1 §6.4.12, Tables 6-19 and 6-20;
/// audit rows 325–327.
///
/// Spec-only, no upstream reference sender or receiver. The upstream
/// `em_rx_link_metric_t` (`em_base.h:1864-1869`) has a count and pointer
/// rather than the spec's inline records. The constructor requires at least
/// one pair; the private list preserves that invariant for encoding.
#[derive(Debug, PartialEq, Eq)]
pub struct ReceiverLinkMetric {
    /// AL MAC address of the transmitting device.
    pub local_al: MacAddr,
    /// AL MAC address of its neighbor.
    pub neighbor_al: MacAddr,
    pairs: Vec<ReceiverLinkMetricPair>,
}

impl ReceiverLinkMetric {
    /// Creates a body, returning `None` if `pairs` is empty.
    #[must_use]
    pub fn new(
        local_al: MacAddr,
        neighbor_al: MacAddr,
        pairs: Vec<ReceiverLinkMetricPair>,
    ) -> Option<Self> {
        if pairs.is_empty() {
            None
        } else {
            Some(Self {
                local_al,
                neighbor_al,
                pairs,
            })
        }
    }

    /// Returns the nonempty interface pairs in wire order.
    #[must_use]
    pub fn pairs(&self) -> &[ReceiverLinkMetricPair] {
        &self.pairs
    }

    /// Returns the pairs for modification without changing their count.
    pub fn pairs_mut(&mut self) -> &mut [ReceiverLinkMetricPair] {
        &mut self.pairs
    }

    /// Appends an interface pair.
    pub fn push_pair(&mut self, pair: ReceiverLinkMetricPair) {
        self.pairs.push(pair);
    }
}

impl TlvBody<'_> for ReceiverLinkMetric {
    const TLV_TYPE: TlvType = TlvType::RECEIVER_LINK_METRIC;

    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let local_al = reader.mac()?;
        let neighbor_al = reader.mac()?;
        if reader.remaining() == 0 {
            return Err(DecodeError::InvalidLength { len: value.len() });
        }
        let mut pairs = bounded_vec(reader.remaining(), reader.remaining(), 23);
        while reader.remaining() != 0 {
            pairs.push(ReceiverLinkMetricPair {
                local_interface: reader.mac()?,
                neighbor_interface: reader.mac()?,
                media_type: MediaType::from(reader.be_u16()?),
                packet_errors: reader.be_u32()?,
                received_packets: reader.be_u32()?,
                rssi: reader.u8()?,
            });
        }
        reader.finish()?;
        Ok(Self {
            local_al,
            neighbor_al,
            pairs,
        })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.mac(self.local_al);
        writer.mac(self.neighbor_al);
        for pair in &self.pairs {
            writer.mac(pair.local_interface);
            writer.mac(pair.neighbor_interface);
            writer.be_u16(u16::from(pair.media_type));
            writer.be_u32(pair.packet_errors);
            writer.be_u32(pair.received_packets);
            writer.u8(pair.rssi);
        }
        Ok(writer.into_bytes())
    }
}

/// Link Metric Result Code, IEEE 1905.1 §6.4.13, Table 6-21; audit row 328.
///
/// Spec-only, no upstream reference sender or receiver; the upstream
/// validation table only lists the TLV as optional (`em_msg.cpp:806`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinkMetricResultCode {
    /// Result code, including reserved values.
    pub code: u8,
}

impl TlvBody<'_> for LinkMetricResultCode {
    const TLV_TYPE: TlvType = TlvType::LINK_METRIC_RESULT_CODE;

    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let code = reader.u8()?;
        reader.finish()?;
        Ok(Self { code })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.u8(self.code);
        Ok(writer.into_bytes())
    }
}

/// A medium in IEEE 1905.1 §6.4.19, Table 6-27; audit row 343.
///
/// Spec-only, no upstream reference sender or receiver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PushButtonMedia<'a> {
    /// Open media type.
    pub media_type: MediaType,
    /// Media-specific bytes, borrowed from the value.
    pub info: &'a [u8],
}

/// Push Button Event Notification, IEEE 1905.1 §6.4.19, Table 6-27;
/// audit row 343.
///
/// Spec-only, no upstream reference sender or receiver. A zero media count is
/// explicitly permitted; information bytes are carried independently of type.
#[derive(Debug, PartialEq, Eq)]
pub struct PushButtonEventNotification<'a> {
    /// Media entries, in wire order.
    pub media: Vec<PushButtonMedia<'a>>,
}

impl<'a> TlvBody<'a> for PushButtonEventNotification<'a> {
    const TLV_TYPE: TlvType = TlvType::PUSH_BUTTON_EVENT_NOTIFICATION;

    fn decode_value(value: &'a [u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let count = usize::from(reader.u8()?);
        // A media entry requires at least two type bytes and one length byte.
        let mut media = bounded_vec(count, reader.remaining(), 3);
        for _ in 0..count {
            let media_type = MediaType::from(reader.be_u16()?);
            let len = usize::from(reader.u8()?);
            let info = reader.bytes(len)?;
            media.push(PushButtonMedia { media_type, info });
        }
        reader.finish()?;
        Ok(Self { media })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.count_u8(self.media.len())?;
        for item in &self.media {
            writer.be_u16(u16::from(item.media_type));
            writer.count_u8(item.info.len())?;
            writer.bytes(item.info);
        }
        Ok(writer.into_bytes())
    }
}

/// Push Button Join Notification, IEEE 1905.1 §6.4.20, Table 6-28;
/// audit row 344.
///
/// Spec-only, no upstream reference sender or receiver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PushButtonJoinNotification {
    /// AL ID of the device that sent the push button event notification.
    pub al_id: MacAddr,
    /// Message identifier of that event notification.
    pub message_id: u16,
    /// Transmitting interface MAC address on the medium joined.
    pub transmitting_interface: MacAddr,
    /// New device's interface MAC address.
    pub new_interface: MacAddr,
}

impl TlvBody<'_> for PushButtonJoinNotification {
    const TLV_TYPE: TlvType = TlvType::PUSH_BUTTON_JOIN_NOTIFICATION;

    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let al_id = reader.mac()?;
        let message_id = reader.be_u16()?;
        let transmitting_interface = reader.mac()?;
        let new_interface = reader.mac()?;
        reader.finish()?;
        Ok(Self {
            al_id,
            message_id,
            transmitting_interface,
            new_interface,
        })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.mac(self.al_id);
        writer.be_u16(self.message_id);
        writer.mac(self.transmitting_interface);
        writer.mac(self.new_interface);
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

    const GROUP: &str = "linkmetric";
    const QUERY_GOLD: Key = ("0x08", "golden");
    const TX_GOLD: Key = ("0x09", "golden");
    const RX_GOLD: Key = ("0x0a", "golden");
    const RESULT_GOLD: Key = ("0x0c", "golden");
    const EVENT_GOLD: Key = ("0x12", "golden");
    const JOIN_GOLD: Key = ("0x13", "golden");

    // Each spec key occurs in a golden or alternate body check below.
    const SPEC_CASES: &[Key] = &[
        QUERY_GOLD,
        ("0x08", "all-neighbors"),
        ("0x08", "all-neighbors-rx"),
        ("0x08", "all-neighbors-with-mac"),
        ("0x08", "specific-tx"),
        ("0x08", "metrics-reserved"),
        ("0x08", "metrics-reserved-specific"),
        ("0x08", "selector-reserved"),
        TX_GOLD,
        ("0x09", "one-link"),
        ("0x09", "bridge-reserved"),
        ("0x09", "link-availability-over-100"),
        RX_GOLD,
        ("0x0a", "one-link"),
        RESULT_GOLD,
        ("0x0c", "result-reserved"),
        ("0x0c", "result-reserved-ff"),
        EVENT_GOLD,
        ("0x12", "zero-media"),
        ("0x12", "info-absent-ethernet"),
        ("0x12", "media-80211-info-empty"),
        JOIN_GOLD,
    ];

    const fn truncated(needed: usize, remaining: usize) -> DecodeError {
        DecodeError::Truncated { needed, remaining }
    }

    const TRAILING: DecodeError = DecodeError::TrailingBytes { remaining: 1 };
    // Every invalid key is checked through both decoding entry points.
    const INVALID_CASES: &[(Key, DecodeError)] = &[
        (("0x08", "specific-without-mac"), truncated(6, 1)),
        (("0x08", "selector-reserved-without-mac"), truncated(6, 1)),
        (("0x08", "empty"), truncated(1, 0)),
        (("0x08", "selector-only"), truncated(1, 0)),
        (("0x08", "cut-mac"), truncated(6, 5)),
        (("0x08", "trailing-bytes"), TRAILING),
        (
            ("0x09", "zero-links"),
            DecodeError::InvalidLength { len: 12 },
        ),
        (("0x09", "empty"), truncated(6, 0)),
        (("0x09", "cut-local-al"), truncated(6, 5)),
        (("0x09", "cut-neighbor-al"), truncated(6, 5)),
        (("0x09", "partial-link"), truncated(4, 2)),
        (("0x09", "cut-phy-rate"), truncated(2, 1)),
        (
            ("0x0a", "zero-links"),
            DecodeError::InvalidLength { len: 12 },
        ),
        (("0x0a", "empty"), truncated(6, 0)),
        (("0x0a", "cut-neighbor-al"), truncated(6, 5)),
        (("0x0a", "partial-link"), truncated(4, 0)),
        (("0x0a", "missing-rssi"), truncated(1, 0)),
        (("0x0c", "empty"), truncated(1, 0)),
        (("0x0c", "trailing-bytes"), TRAILING),
        (("0x12", "empty"), truncated(1, 0)),
        (("0x12", "count-overstates"), truncated(2, 0)),
        (("0x12", "cut-media-type"), truncated(2, 1)),
        (("0x12", "missing-info-length"), truncated(1, 0)),
        (("0x12", "info-length-overstates"), truncated(10, 6)),
        (("0x12", "trailing-bytes"), TRAILING),
        (("0x13", "empty"), truncated(6, 0)),
        (("0x13", "cut-mid"), truncated(2, 1)),
        (("0x13", "cut-new-if-mac"), truncated(6, 5)),
        (("0x13", "trailing-bytes"), TRAILING),
    ];

    const AL: MacAddr = MacAddr::new([0x02, 0x11, 0x22, 0x33, 0x44, 0x55]);
    const NEIGH: MacAddr = MacAddr::new([0x02, 0x66, 0x77, 0x88, 0x99, 0xaa]);
    const NEIGH2: MacAddr = MacAddr::new([0x02, 0xbb, 0xcc, 0xdd, 0xee, 0xff]);
    const LOCAL1: MacAddr = MacAddr::new([0x02, 0xa0, 0xb1, 0xc2, 0xd3, 0xe1]);
    const LOCAL2: MacAddr = MacAddr::new([0x02, 0xa0, 0xb1, 0xc2, 0xd3, 0xe2]);
    const REMOTE1: MacAddr = MacAddr::new([0x3c, 0x1d, 0x2e, 0x3f, 0x4a, 0x5b]);
    const REMOTE2: MacAddr = MacAddr::new([0x7a, 0x6b, 0x5c, 0x4d, 0x3e, 0x2f]);
    const WIFI_INFO: &[u8] = &[0x02, 0xf0, 0xe1, 0xd2, 0xc3, 0xb4, 0x40, 0x02, 0x2a, 0x9b];
    const TOO_LONG: EncodeError = EncodeError::CountTooLarge { len: 256, max: 255 };

    fn fixture(key: Key) -> Vec<u8> {
        body_fixtures::value(GROUP, key)
    }

    fn query(selector: u8, neighbor: Option<MacAddr>, metrics: u8) -> LinkMetricQuery {
        LinkMetricQuery::new(selector, neighbor, metrics).unwrap()
    }

    const fn tx_pair1() -> TransmitterLinkMetricPair {
        TransmitterLinkMetricPair {
            local_interface: LOCAL1,
            neighbor_interface: REMOTE1,
            media_type: MediaType(0x0001),
            bridge_flag: 0,
            packet_errors: 0x0102_0304,
            transmitted_packets: 0x1234_5678,
            mac_throughput_capacity: 940,
            link_availability: 87,
            phy_rate: 1000,
        }
    }

    const fn tx_pair2() -> TransmitterLinkMetricPair {
        TransmitterLinkMetricPair {
            local_interface: LOCAL2,
            neighbor_interface: REMOTE2,
            media_type: MediaType(0x0105),
            bridge_flag: 1,
            packet_errors: 258,
            transmitted_packets: 0x2abb_ccdd,
            mac_throughput_capacity: 866,
            link_availability: 100,
            phy_rate: 65535,
        }
    }

    fn tx(pairs: Vec<TransmitterLinkMetricPair>) -> TransmitterLinkMetric {
        TransmitterLinkMetric::new(AL, NEIGH, pairs).unwrap()
    }

    fn tx_raw(bridge_flag: u8, link_availability: u16) -> TransmitterLinkMetric {
        tx(vec![TransmitterLinkMetricPair {
            bridge_flag,
            packet_errors: 0,
            transmitted_packets: 0,
            mac_throughput_capacity: 100,
            link_availability,
            phy_rate: 100,
            ..tx_pair1()
        }])
    }

    const fn rx_pair1() -> ReceiverLinkMetricPair {
        ReceiverLinkMetricPair {
            local_interface: LOCAL1,
            neighbor_interface: REMOTE1,
            media_type: MediaType(0x0104),
            packet_errors: 0x0001_0203,
            received_packets: 0x0a0b_0c0d,
            rssi: 60,
        }
    }

    const fn rx_pair2() -> ReceiverLinkMetricPair {
        ReceiverLinkMetricPair {
            local_interface: LOCAL2,
            neighbor_interface: REMOTE2,
            media_type: MediaType(0),
            packet_errors: 1024,
            received_packets: 0x0012_3456,
            rssi: 255,
        }
    }

    fn rx(pairs: Vec<ReceiverLinkMetricPair>) -> ReceiverLinkMetric {
        ReceiverLinkMetric::new(AL, NEIGH, pairs).unwrap()
    }

    const fn medium(media_type: u16, info: &'static [u8]) -> PushButtonMedia<'static> {
        PushButtonMedia {
            media_type: MediaType(media_type),
            info,
        }
    }

    const fn field(offset: usize, width: usize) -> Field {
        Field { offset, width }
    }

    body_checks! {
        mod query_checks;
        body: LinkMetricQuery;
        golden: &fixture(QUERY_GOLD), query(1, Some(NEIGH), 2);
        prefixes: |_| false, truncated_in(&[field(0, 1), field(1, 6), field(7, 1)]);
        trailing_byte: &fixture(QUERY_GOLD);
        reserved_bits: [
            // Selector 0 never carries a MAC: the MAC octets are ignored and not written.
            (&fixture(("0x08", "all-neighbors-with-mac")), query(0, None, 1), &fixture(("0x08", "all-neighbors-rx"))),
        ];
        alternates: [
            (&fixture(("0x08", "all-neighbors")), query(0, None, 0), &fixture(("0x08", "all-neighbors"))),
            (&fixture(("0x08", "all-neighbors-rx")), query(0, None, 1), &fixture(("0x08", "all-neighbors-rx"))),
                        (&fixture(("0x08", "specific-tx")), query(1, Some(NEIGH2), 0), &fixture(("0x08", "specific-tx"))),
            (&fixture(("0x08", "metrics-reserved")), query(0, None, 3), &fixture(("0x08", "metrics-reserved"))),
            (&fixture(("0x08", "metrics-reserved-specific")), query(1, Some(NEIGH), 255), &fixture(("0x08", "metrics-reserved-specific"))),
            (&fixture(("0x08", "selector-reserved")), query(2, Some(NEIGH), 2), &fixture(("0x08", "selector-reserved"))),
        ];
    }

    body_checks! {
        mod tx_checks;
        body: TransmitterLinkMetric;
        golden: &fixture(TX_GOLD), tx(vec![tx_pair1(), tx_pair2()]);
        prefixes: |len| len == 41, |len| {
            if len == 12 { DecodeError::InvalidLength { len } } else {
                truncated_in(&[
                    field(0, 6), field(6, 6),
                    field(12, 6), field(18, 6), field(24, 2), field(26, 1), field(27, 4), field(31, 4), field(35, 2), field(37, 2), field(39, 2),
                    field(41, 6), field(47, 6), field(53, 2), field(55, 1), field(56, 4), field(60, 4), field(64, 2), field(66, 2), field(68, 2)
                ])(len)
            }
        };
        alternates: [
            (&fixture(("0x09", "one-link")), tx(vec![tx_pair1()]), &fixture(("0x09", "one-link"))),
            (&fixture(("0x09", "bridge-reserved")), tx_raw(2, 50), &fixture(("0x09", "bridge-reserved"))),
            (&fixture(("0x09", "link-availability-over-100")), tx_raw(0, 101), &fixture(("0x09", "link-availability-over-100"))),
        ];
    }

    body_checks! {
        mod rx_checks;
        body: ReceiverLinkMetric;
        golden: &fixture(RX_GOLD), rx(vec![rx_pair1(), rx_pair2()]);
        prefixes: |len| len == 35, |len| {
            if len == 12 { DecodeError::InvalidLength { len } } else {
                truncated_in(&[
                    field(0, 6), field(6, 6),
                    field(12, 6), field(18, 6), field(24, 2), field(26, 4), field(30, 4), field(34, 1),
                    field(35, 6), field(41, 6), field(47, 2), field(49, 4), field(53, 4), field(57, 1)
                ])(len)
            }
        };
        alternates: [
            (&fixture(("0x0a", "one-link")), rx(vec![rx_pair1()]), &fixture(("0x0a", "one-link"))),
        ];
    }

    body_checks! {
        mod result_checks;
        body: LinkMetricResultCode;
        golden: &fixture(RESULT_GOLD), LinkMetricResultCode { code: 0 };
        prefixes: |_| false, truncated_in(&[field(0, 1)]);
        trailing_byte: &fixture(RESULT_GOLD);
        alternates: [
            (&fixture(("0x0c", "result-reserved")), LinkMetricResultCode { code: 1 }, &fixture(("0x0c", "result-reserved"))),
            (&fixture(("0x0c", "result-reserved-ff")), LinkMetricResultCode { code: 255 }, &fixture(("0x0c", "result-reserved-ff"))),
        ];
    }

    body_checks! {
        mod event_checks;
        body: PushButtonEventNotification<'a>;
        golden: &fixture(EVENT_GOLD), PushButtonEventNotification { media: vec![medium(0x0104, WIFI_INFO), medium(0x0001, &[])] };
        prefixes: |_| false, truncated_in(&[field(0, 1), field(1, 2), field(3, 1), field(4, 10), field(14, 2), field(16, 1)]);
        trailing_byte: &fixture(EVENT_GOLD);
        count_too_large: PushButtonEventNotification { media: vec![medium(0, &[]); 256] }, TOO_LONG;
        alternates: [
            (&fixture(("0x12", "zero-media")), PushButtonEventNotification { media: vec![] }, &fixture(("0x12", "zero-media"))),
            (&fixture(("0x12", "info-absent-ethernet")), PushButtonEventNotification { media: vec![medium(0x0001, &[])] }, &fixture(("0x12", "info-absent-ethernet"))),
            (&fixture(("0x12", "media-80211-info-empty")), PushButtonEventNotification { media: vec![medium(0x0105, &[])] }, &fixture(("0x12", "media-80211-info-empty"))),
        ];
    }

    body_checks! {
        mod join_checks;
        body: PushButtonJoinNotification;
        golden: &fixture(JOIN_GOLD), PushButtonJoinNotification { al_id: AL, message_id: 0x1a2b, transmitting_interface: LOCAL1, new_interface: REMOTE1 };
        prefixes: |_| false, truncated_in(&[field(0, 6), field(6, 2), field(8, 6), field(14, 6)]);
        trailing_byte: &fixture(JOIN_GOLD);
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
                DecodeError::InvalidLength { .. } => "error=InvalidLength",
                _ => panic!("unexpected error kind for {key:?}"),
            };
            assert!(
                fields.split(';').any(|field| field == kind),
                "{key:?}: {fields}"
            );
            let value = fixture(key);
            match key.0 {
                "0x08" => check_invalid::<LinkMetricQuery>(&value, expected, key),
                "0x09" => check_invalid::<TransmitterLinkMetric>(&value, expected, key),
                "0x0a" => check_invalid::<ReceiverLinkMetric>(&value, expected, key),
                "0x0c" => check_invalid::<LinkMetricResultCode>(&value, expected, key),
                "0x12" => check_invalid::<PushButtonEventNotification<'_>>(&value, expected, key),
                "0x13" => check_invalid::<PushButtonJoinNotification>(&value, expected, key),
                _ => panic!("unknown fixture TLV for {key:?}"),
            }
        }
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

    /// Rows that re-encode to the bytes of another row: selector 0 never carries a MAC.
    const CANONICAL_ROWS: &[(Key, Key)] = &[(
        ("0x08", "all-neighbors-with-mac"),
        ("0x08", "all-neighbors-rx"),
    )];

    fn dispatch_row(row: &Row<'_>, bytes: &[u8], canonical: Option<&[u8]>) -> Result<(), String> {
        use crate::body_testkit::sweep_row;
        match row.tlv {
            "0x08" => sweep_row::<LinkMetricQuery>(row, bytes, canonical),
            "0x09" => sweep_row::<TransmitterLinkMetric>(row, bytes, canonical),
            "0x0a" => sweep_row::<ReceiverLinkMetric>(row, bytes, canonical),
            "0x0c" => sweep_row::<LinkMetricResultCode>(row, bytes, canonical),
            "0x12" => sweep_row::<PushButtonEventNotification<'_>>(row, bytes, canonical),
            "0x13" => sweep_row::<PushButtonJoinNotification>(row, bytes, canonical),
            other => Err(format!("no body for TLV {other}")),
        }
    }

    #[test]
    fn every_fixture_row_decodes_and_encodes_as_its_kind_says() {
        crate::body_testkit::sweep_group(GROUP, CANONICAL_ROWS, dispatch_row);
    }

    #[test]
    fn query_constructor_gives_selector_zero_no_mac_and_other_selectors_one() {
        for selector in 0..=u8::MAX {
            let without = LinkMetricQuery::new(selector, None, 0xff);
            let with = LinkMetricQuery::new(selector, Some(NEIGH), 0xff);
            if selector == 0 {
                let query = without.unwrap();
                assert_eq!(with, None, "selector 0 with a MAC");
                assert_eq!(query.neighbor(), None);
                let bytes = query.encode_value().unwrap();
                assert_eq!(bytes, [0x00, 0xff]);
                assert_eq!(LinkMetricQuery::decode_value(&bytes), Ok(query));
            } else {
                let query = with.unwrap();
                assert_eq!(without, None, "selector {selector} without a MAC");
                assert_eq!(query.selector(), selector);
                assert_eq!(query.neighbor(), Some(NEIGH));
                let bytes = query.encode_value().unwrap();
                assert_eq!(bytes.len(), 8);
                assert_eq!(LinkMetricQuery::decode_value(&bytes), Ok(query));
            }
        }
    }

    #[test]
    fn query_selector_zero_with_a_mac_decodes_without_it_and_encodes_two_octets() {
        let value = fixture(("0x08", "all-neighbors-with-mac"));
        assert_eq!(value.len(), 8);
        let query = LinkMetricQuery::decode_value(&value).unwrap();
        assert_eq!(query.neighbor(), None);
        assert_eq!(query.encode_value().unwrap(), [0x00, 0x01]);
    }

    #[test]
    fn constructors_require_nonempty_pairs() {
        assert!(TransmitterLinkMetric::new(AL, NEIGH, vec![]).is_none());
        assert!(ReceiverLinkMetric::new(AL, NEIGH, vec![]).is_none());
        let mut transmitter = tx(vec![tx_pair1()]);
        transmitter.pairs_mut()[0].bridge_flag = 2;
        transmitter.push_pair(tx_pair2());
        assert_eq!(
            transmitter.pairs(),
            &[
                TransmitterLinkMetricPair {
                    bridge_flag: 2,
                    ..tx_pair1()
                },
                tx_pair2(),
            ]
        );
        let mut receiver = rx(vec![rx_pair1()]);
        receiver.pairs_mut()[0].rssi = 255;
        receiver.push_pair(rx_pair2());
        assert_eq!(
            receiver.pairs(),
            &[
                ReceiverLinkMetricPair {
                    rssi: 255,
                    ..rx_pair1()
                },
                rx_pair2()
            ]
        );
    }

    #[test]
    fn query_selector_zero_partial_mac_is_truncated() {
        let value = fixture(("0x08", "all-neighbors-with-mac"));
        for len in 3..8 {
            let expected = if len <= 6 {
                truncated(6, len - 1)
            } else {
                truncated(1, 0)
            };
            assert_eq!(
                LinkMetricQuery::decode_value(&value[..len]),
                Err(expected),
                "length {len}"
            );
        }
    }

    #[test]
    fn media_lengths_counts_and_borrowing() {
        let long_info = [0_u8; 256];
        let too_much_info = PushButtonEventNotification {
            media: vec![PushButtonMedia {
                media_type: MediaType(0xffff),
                info: &long_info,
            }],
        };
        assert_eq!(too_much_info.encode_value(), Err(TOO_LONG));
        let info = [0x5a_u8; 255];
        let mut media = vec![medium(0xffff, &[]); 255];
        media[0].info = &info;
        let max = PushButtonEventNotification { media };
        let encoded = max.encode_value().unwrap();
        assert!(crate::body_testkit::build_message(&max).is_ok());
        assert_eq!(PushButtonEventNotification::decode_value(&encoded), Ok(max));
        assert_eq!(
            PushButtonEventNotification::decode_value(&[255]),
            Err(truncated(2, 0))
        );
        assert_eq!(
            PushButtonEventNotification::decode_value(&[1, 0, 1, 255]),
            Err(truncated(255, 0))
        );
        let value = fixture(EVENT_GOLD);
        let decoded = PushButtonEventNotification::decode_value(&value).unwrap();
        assert!(core::ptr::eq(
            decoded.media[0].info.as_ptr(),
            value[4..].as_ptr()
        ));
    }

    #[test]
    fn body_and_record_sizes_have_bounded_stack_cost() {
        let word = size_of::<usize>();
        assert!(size_of::<LinkMetricQuery>() <= 16);
        assert!(size_of::<TransmitterLinkMetricPair>() <= 40);
        assert!(size_of::<TransmitterLinkMetric>() <= 5 * word);
        assert!(size_of::<ReceiverLinkMetricPair>() <= 32);
        assert!(size_of::<ReceiverLinkMetric>() <= 5 * word);
        assert!(size_of::<LinkMetricResultCode>() <= 1);
        assert!(size_of::<PushButtonMedia<'_>>() <= 3 * word);
        assert!(size_of::<PushButtonEventNotification<'_>>() <= 3 * word);
        assert!(size_of::<PushButtonJoinNotification>() <= 20);
    }
}
