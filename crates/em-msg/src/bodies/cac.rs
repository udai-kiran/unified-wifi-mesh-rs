// SPDX-License-Identifier: Apache-2.0

//! CAC, anticipated channel preference and usage, spatial reuse and spectrum inquiry bodies.

use em_types::{MacAddr, TlvType};

use super::{TlvBody, bounded_vec};
use crate::body::{Reader, Writer};
use crate::error::{DecodeError, EncodeError};

fn checked_bits(value: u8, max: u8) -> Result<u8, EncodeError> {
    if value > max {
        Err(EncodeError::ValueTooLarge {
            value: u32::from(value),
            max: u32::from(max),
        })
    } else {
        Ok(value)
    }
}

/// A radio in CAC Request, `EasyMesh` §17.2.42 Table 64; audit rows 153–155.
/// Upstream `em_cac_req_t` (`em_base.h:1043-1051`) models only one radio;
/// there is no production sender or receiver (test sender `validation_test.cpp:1010-1042`).
/// Its completion-action bitfield is one bit too wide; this type uses the spec width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacRequestRadio {
    /// Radio unique identifier.
    pub ruid: MacAddr,
    /// Global operating class.
    pub operating_class: u8,
    /// Channel on which CAC is requested.
    pub channel: u8,
    /// Three-bit CAC method, including reserved codes.
    pub method: u8,
    /// Two-bit completion action, including reserved codes.
    pub completion_action: u8,
}

/// CAC Request, `EasyMesh` §17.2.42 Table 64; audit rows 153–155.
/// Upstream `em_cac_req_t` (`em_base.h:1043-1051`) has no production sender or
/// receiver; the test sender `validation_test.cpp:1010-1042` writes uninitialised
/// struct bytes. Unlike that struct, all radio groups and the spec bit widths are used.
#[derive(Debug, PartialEq, Eq)]
pub struct CacRequest {
    /// Radios in wire order; the count may be zero.
    pub radios: Vec<CacRequestRadio>,
}

impl TlvBody<'_> for CacRequest {
    const TLV_TYPE: TlvType = TlvType::CAC_REQUEST;

    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let count = usize::from(reader.u8()?);
        let mut radios = bounded_vec(count, reader.remaining(), 9);
        for _ in 0..count {
            let ruid = reader.mac()?;
            let operating_class = reader.u8()?;
            let channel = reader.u8()?;
            let flags = reader.u8()?;
            radios.push(CacRequestRadio {
                ruid,
                operating_class,
                channel,
                method: flags >> 5,
                completion_action: (flags >> 3) & 3,
            });
        }
        reader.finish()?;
        Ok(Self { radios })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.count_u8(self.radios.len())?;
        for radio in &self.radios {
            writer.mac(radio.ruid);
            writer.u8(radio.operating_class);
            writer.u8(radio.channel);
            writer.u8((checked_bits(radio.method, 7)? << 5)
                | (checked_bits(radio.completion_action, 3)? << 3));
        }
        Ok(writer.into_bytes())
    }
}

/// A radio in CAC Termination, `EasyMesh` §17.2.43 Table 65; audit rows 156–157.
/// Upstream `em_cac_term_t` (`em_base.h:1035-1040`) has a different field order
/// and no repeated group; no production sender or field parser exists
/// (`em_msg.cpp:1104-1107` checks only presence and length).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacTerminationRadio {
    /// Radio unique identifier.
    pub ruid: MacAddr,
    /// Global operating class.
    pub operating_class: u8,
    /// Channel of the CAC to terminate.
    pub channel: u8,
}

/// CAC Termination, `EasyMesh` §17.2.43 Table 65; audit rows 156–157.
/// Upstream `em_cac_term_t` (`em_base.h:1035-1040`) is not used by a production
/// sender or field parser; `em_msg.cpp:1104-1107` checks only presence and length.
/// This codec follows the spec's count-first, repeating layout.
#[derive(Debug, PartialEq, Eq)]
pub struct CacTermination {
    /// Radios in wire order; the count may be zero.
    pub radios: Vec<CacTerminationRadio>,
}

impl TlvBody<'_> for CacTermination {
    const TLV_TYPE: TlvType = TlvType::CAC_TERMINATION;

    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let count = usize::from(reader.u8()?);
        let mut radios = bounded_vec(count, reader.remaining(), 8);
        for _ in 0..count {
            radios.push(CacTerminationRadio {
                ruid: reader.mac()?,
                operating_class: reader.u8()?,
                channel: reader.u8()?,
            });
        }
        reader.finish()?;
        Ok(Self { radios })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.count_u8(self.radios.len())?;
        for radio in &self.radios {
            writer.mac(radio.ruid);
            writer.u8(radio.operating_class);
            writer.u8(radio.channel);
        }
        Ok(writer.into_bytes())
    }
}

/// A detected class and channel in CAC Completion Report, `EasyMesh` §17.2.44
/// Table 66; audit rows 158–159. Upstream `em_cac_comp_rprt_pair_t`
/// (`em_base.h:1508-1511`), sender `em_channel.cpp:1466-1507`; no receiver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacDetectedPair {
    /// Operating class on which radar was detected.
    pub operating_class: u8,
    /// Channel on which radar was detected.
    pub channel: u8,
}

/// A radio in CAC Completion Report, `EasyMesh` §17.2.44 Table 66; audit rows 158–159.
/// Upstream `em_cac_comp_rprt_radio_t` (`em_base.h:1513-1520`), sender
/// `em_channel.cpp:1466-1507`; no receiver. Unlike the sender's shared data-model
/// record, each radio retains its own status and detected pairs.
#[derive(Debug, PartialEq, Eq)]
pub struct CacCompletionRadio {
    /// Radio unique identifier.
    pub ruid: MacAddr,
    /// Operating class used for CAC.
    pub operating_class: u8,
    /// Channel used for CAC.
    pub channel: u8,
    /// Completion status, including reserved values.
    pub status: u8,
    /// Detected class/channel pairs; status/pair consistency is for the consumer.
    pub detected_pairs: Vec<CacDetectedPair>,
}

/// CAC Completion Report, `EasyMesh` §17.2.44 Table 66; audit rows 158–159.
/// Upstream `em_cac_comp_rprt_t` (`em_base.h:1522-1525`), sender
/// `em_channel.cpp:1466-1507`; the report handler `:1875-1912` ignores it.
/// Counts use the full wire range instead of the data model's four-pair limit.
#[derive(Debug, PartialEq, Eq)]
pub struct CacCompletionReport {
    /// Radio reports in wire order.
    pub radios: Vec<CacCompletionRadio>,
}

impl TlvBody<'_> for CacCompletionReport {
    const TLV_TYPE: TlvType = TlvType::CAC_COMPLETION_REPORT;

    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let count = usize::from(reader.u8()?);
        let mut radios = bounded_vec(count, reader.remaining(), 10);
        for _ in 0..count {
            let ruid = reader.mac()?;
            let operating_class = reader.u8()?;
            let channel = reader.u8()?;
            let status = reader.u8()?;
            let pairs = usize::from(reader.u8()?);
            let mut detected_pairs = bounded_vec(pairs, reader.remaining(), 2);
            for _ in 0..pairs {
                detected_pairs.push(CacDetectedPair {
                    operating_class: reader.u8()?,
                    channel: reader.u8()?,
                });
            }
            radios.push(CacCompletionRadio {
                ruid,
                operating_class,
                channel,
                status,
                detected_pairs,
            });
        }
        reader.finish()?;
        Ok(Self { radios })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.count_u8(self.radios.len())?;
        for radio in &self.radios {
            writer.mac(radio.ruid);
            writer.u8(radio.operating_class);
            writer.u8(radio.channel);
            writer.u8(radio.status);
            writer.count_u8(radio.detected_pairs.len())?;
            for pair in &radio.detected_pairs {
                writer.u8(pair.operating_class);
                writer.u8(pair.channel);
            }
        }
        Ok(writer.into_bytes())
    }
}

/// Available channel in CAC Status Report, `EasyMesh` §17.2.45 Table 67;
/// audit rows 160–162. Upstream `em_cac_avail_t` (`em_base.h:1528-1532`),
/// sender `em_channel.cpp:1367-1406`; no receiver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacAvailableChannel {
    /// Global operating class.
    pub operating_class: u8,
    /// Channel number.
    pub channel: u8,
    /// Minutes since CAC completed.
    pub minutes: u16,
}

/// Non-occupancy entry in CAC Status Report, `EasyMesh` §17.2.45 Table 67;
/// audit rows 160–162. Upstream `em_cac_non_occ_t` (`em_base.h:1534-1538`),
/// sender `em_channel.cpp:1408-1432`; no receiver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacNonOccupancyChannel {
    /// Global operating class.
    pub operating_class: u8,
    /// Channel number.
    pub channel: u8,
    /// Seconds remaining in the non-occupancy period.
    pub duration: u16,
}

/// Active CAC entry in CAC Status Report, `EasyMesh` §17.2.45 Table 67;
/// audit rows 160–162. Upstream `em_cac_active_t` (`em_base.h:1540-1544`),
/// sender `em_channel.cpp:1434-1460`; no receiver. Countdown is three
/// big-endian octets (0008 D4). Intentional difference from the sender: it
/// writes only the low 24 bits of its countdown (`em_channel.cpp:1449-1451`),
/// so `0x0100_0000` goes out as `00 00 00`; this encoder returns
/// `ValueTooLarge` instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacActiveChannel {
    /// Global operating class.
    pub operating_class: u8,
    /// Channel number.
    pub channel: u8,
    /// Seconds remaining, up to `0xff_ffff`.
    pub countdown: u32,
}

/// CAC Status Report, `EasyMesh` §17.2.45 Table 67; audit rows 160–162.
/// Upstream `em_cac_status_rprt_*_t` (`em_base.h:1546-1559`), sender
/// `em_channel.cpp:1367-1464`; handler `:1875-1912` ignores it. Three empty
/// lists are accepted unlike the upstream validation minimum. Intentional
/// difference from the sender: a count above 255 returns `CountTooLarge`
/// instead of wrapping its 8-bit counter (0008 D4, `em_channel.cpp:1404,1430,1458`).
#[derive(Debug, PartialEq, Eq)]
pub struct CacStatusReport {
    /// Available channels.
    pub available: Vec<CacAvailableChannel>,
    /// Channels in a non-occupancy period.
    pub non_occupancy: Vec<CacNonOccupancyChannel>,
    /// Channels with active CAC.
    pub active: Vec<CacActiveChannel>,
}

impl TlvBody<'_> for CacStatusReport {
    const TLV_TYPE: TlvType = TlvType::CAC_STATUS_REPORT;

    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let count = usize::from(reader.u8()?);
        let mut available = bounded_vec(count, reader.remaining(), 4);
        for _ in 0..count {
            available.push(CacAvailableChannel {
                operating_class: reader.u8()?,
                channel: reader.u8()?,
                minutes: reader.be_u16()?,
            });
        }
        let count = usize::from(reader.u8()?);
        let mut non_occupancy = bounded_vec(count, reader.remaining(), 4);
        for _ in 0..count {
            non_occupancy.push(CacNonOccupancyChannel {
                operating_class: reader.u8()?,
                channel: reader.u8()?,
                duration: reader.be_u16()?,
            });
        }
        let count = usize::from(reader.u8()?);
        let mut active = bounded_vec(count, reader.remaining(), 5);
        for _ in 0..count {
            active.push(CacActiveChannel {
                operating_class: reader.u8()?,
                channel: reader.u8()?,
                countdown: reader.be_u24()?,
            });
        }
        reader.finish()?;
        Ok(Self {
            available,
            non_occupancy,
            active,
        })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.count_u8(self.available.len())?;
        for entry in &self.available {
            writer.u8(entry.operating_class);
            writer.u8(entry.channel);
            writer.be_u16(entry.minutes);
        }
        writer.count_u8(self.non_occupancy.len())?;
        for entry in &self.non_occupancy {
            writer.u8(entry.operating_class);
            writer.u8(entry.channel);
            writer.be_u16(entry.duration);
        }
        writer.count_u8(self.active.len())?;
        for entry in &self.active {
            writer.u8(entry.operating_class);
            writer.u8(entry.channel);
            writer.be_u24(entry.countdown)?;
        }
        Ok(writer.into_bytes())
    }
}

/// Operating class in Anticipated Channel Preference, `EasyMesh` §17.2.87
/// Table 109; audit rows 259–261. Upstream `em_anti_channel_pref_t`
/// (`em_base.h:1745-1748`) has a different unused layout and no sender or parser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnticipatedOperatingClass<'a> {
    /// Global operating class.
    pub operating_class: u8,
    /// Channel numbers; empty means all channels in the class.
    pub channels: &'a [u8],
}

/// Anticipated Channel Preference, `EasyMesh` §17.2.87 Table 109; audit rows 259–261.
/// Upstream `em_anti_channel_pref_t` (`em_base.h:1745-1748`) has no sender or
/// parser; `em_msg.cpp:1025-1028` only validates presence and rejects short
/// valid values. This codec follows the repeated spec layout and ignores the
/// four reserved octets in each class.
#[derive(Debug, PartialEq, Eq)]
pub struct AnticipatedChannelPreference<'a> {
    /// Operating classes in wire order.
    pub operating_classes: Vec<AnticipatedOperatingClass<'a>>,
}

impl<'a> TlvBody<'a> for AnticipatedChannelPreference<'a> {
    const TLV_TYPE: TlvType = TlvType::ANTICIPATED_CHANNEL_PREFERENCE;

    fn decode_value(value: &'a [u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let count = usize::from(reader.u8()?);
        let mut operating_classes = bounded_vec(count, reader.remaining(), 6);
        for _ in 0..count {
            let operating_class = reader.u8()?;
            let len = usize::from(reader.u8()?);
            let channels = reader.bytes(len)?;
            reader.skip(4)?;
            operating_classes.push(AnticipatedOperatingClass {
                operating_class,
                channels,
            });
        }
        reader.finish()?;
        Ok(Self { operating_classes })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.count_u8(self.operating_classes.len())?;
        for class in &self.operating_classes {
            writer.u8(class.operating_class);
            writer.count_u8(class.channels.len())?;
            writer.bytes(class.channels);
            writer.zeros(4);
        }
        Ok(writer.into_bytes())
    }
}

/// A usage in Anticipated Channel Usage, `EasyMesh` §17.2.88 Table 110;
/// audit rows 262–264. Upstream `em_anti_chan_usage_t` (`em_base.h:978-993`)
/// has no sender or parser and fixes the bitmask at ten bytes. This codec uses
/// the self-describing wire length, including lengths other than 2, 3, 5 or 10.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnticipatedUsage<'a> {
    /// Low four octets of the reference TSF at burst start.
    pub burst_start_time: u32,
    /// Burst duration in microseconds.
    pub burst_length: u32,
    /// Number of repetitions; `u32::MAX` means indefinite or unknown.
    pub repetitions: u32,
    /// Interval between bursts in microseconds.
    pub burst_interval: u32,
    /// Raw RU bitmask, whose length is carried on the wire.
    pub ru_bitmask: &'a [u8],
    /// Transmitter identifier as six raw EUI-48 octets.
    pub transmitter: MacAddr,
    /// Transmit power as a 2s-complement signed integer in dBm (§17.2.88
    /// Table 110). Octet 0xFF, read as -1, means unknown or several
    /// transmitters with different powers, so a real -1 dBm reading cannot
    /// be expressed.
    pub power_level: i8,
    /// Usage reason, including reserved values.
    pub reason: u8,
}

/// Anticipated Channel Usage, `EasyMesh` §17.2.88 Table 110; audit rows 262–264.
/// Upstream `em_anti_chan_usage_t` (`em_base.h:978-993`) has no sender or
/// parser; `em_msg.cpp:1020-1023` validates only a minimum length. Unlike that
/// struct, the codec retains all usages and accepts any RU bitmask length.
#[derive(Debug, PartialEq, Eq)]
pub struct AnticipatedChannelUsage<'a> {
    /// Global operating class.
    pub operating_class: u8,
    /// Channel number.
    pub channel: u8,
    /// Reference BSSID for the TSF timer.
    pub reference_bssid: MacAddr,
    /// Anticipated usages in wire order.
    pub usages: Vec<AnticipatedUsage<'a>>,
}

impl<'a> TlvBody<'a> for AnticipatedChannelUsage<'a> {
    const TLV_TYPE: TlvType = TlvType::ANTICIPATED_CHANNEL_USAGE;

    fn decode_value(value: &'a [u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let operating_class = reader.u8()?;
        let channel = reader.u8()?;
        let reference_bssid = reader.mac()?;
        let count = usize::from(reader.u8()?);
        let mut usages = bounded_vec(count, reader.remaining(), 29);
        for _ in 0..count {
            let burst_start_time = reader.be_u32()?;
            let burst_length = reader.be_u32()?;
            let repetitions = reader.be_u32()?;
            let burst_interval = reader.be_u32()?;
            let len = usize::from(reader.u8()?);
            let ru_bitmask = reader.bytes(len)?;
            let transmitter = reader.mac()?;
            let power_level = i8::from_ne_bytes([reader.u8()?]);
            let reason = reader.u8()?;
            reader.skip(4)?;
            usages.push(AnticipatedUsage {
                burst_start_time,
                burst_length,
                repetitions,
                burst_interval,
                ru_bitmask,
                transmitter,
                power_level,
                reason,
            });
        }
        reader.finish()?;
        Ok(Self {
            operating_class,
            channel,
            reference_bssid,
            usages,
        })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.u8(self.operating_class);
        writer.u8(self.channel);
        writer.mac(self.reference_bssid);
        writer.count_u8(self.usages.len())?;
        for usage in &self.usages {
            writer.be_u32(usage.burst_start_time);
            writer.be_u32(usage.burst_length);
            writer.be_u32(usage.repetitions);
            writer.be_u32(usage.burst_interval);
            writer.count_u8(usage.ru_bitmask.len())?;
            writer.bytes(usage.ru_bitmask);
            writer.mac(usage.transmitter);
            writer.u8(usage.power_level.to_ne_bytes()[0]);
            writer.u8(usage.reason);
            writer.zeros(4);
        }
        Ok(writer.into_bytes())
    }
}

/// Common spatial-reuse fields, `EasyMesh` §17.2.89 Table 111 and §17.2.90
/// Table 112; audit rows 265–268. Upstream structs `em_base.h:1384-1402,1472-1488`,
/// senders `em_channel.cpp:944-968,1166-1194`, report receiver `:1752-1774`;
/// these functions have no callers. Offsets and bitmaps remain present even
/// when the corresponding validity flag is clear.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)] // Independent wire flags.
pub struct SpatialReuseParameters {
    /// Six-bit BSS color.
    pub bss_color: u8,
    /// HESIGA spatial reuse value 15 is allowed.
    pub hesiga_value_15_allowed: bool,
    /// SRG information fields are valid.
    pub srg_information_valid: bool,
    /// Non-SRG maximum offset is valid.
    pub non_srg_offset_valid: bool,
    /// PSR-based spatial reuse is disallowed.
    pub psr_disallowed: bool,
    /// Non-SRG OBSS PD maximum offset.
    pub non_srg_obss_pd_max_offset: u8,
    /// SRG OBSS PD minimum offset.
    pub srg_obss_pd_min_offset: u8,
    /// SRG OBSS PD maximum offset.
    pub srg_obss_pd_max_offset: u8,
    /// Raw SRG BSS color bitmap.
    pub srg_bss_color_bitmap: [u8; 8],
    /// Raw SRG partial BSSID bitmap.
    pub srg_partial_bssid_bitmap: [u8; 8],
}

impl SpatialReuseParameters {
    fn read(reader: &mut Reader<'_>, color: u8) -> Result<Self, DecodeError> {
        let flags = reader.u8()?;
        Ok(Self {
            bss_color: color & 0x3f,
            hesiga_value_15_allowed: flags & 0x10 != 0,
            srg_information_valid: flags & 0x08 != 0,
            non_srg_offset_valid: flags & 0x04 != 0,
            psr_disallowed: flags & 0x01 != 0,
            non_srg_obss_pd_max_offset: reader.u8()?,
            srg_obss_pd_min_offset: reader.u8()?,
            srg_obss_pd_max_offset: reader.u8()?,
            srg_bss_color_bitmap: reader.array()?,
            srg_partial_bssid_bitmap: reader.array()?,
        })
    }

    fn write(&self, writer: &mut Writer) {
        writer.u8((u8::from(self.hesiga_value_15_allowed) << 4)
            | (u8::from(self.srg_information_valid) << 3)
            | (u8::from(self.non_srg_offset_valid) << 2)
            | u8::from(self.psr_disallowed));
        writer.u8(self.non_srg_obss_pd_max_offset);
        writer.u8(self.srg_obss_pd_min_offset);
        writer.u8(self.srg_obss_pd_max_offset);
        writer.bytes(&self.srg_bss_color_bitmap);
        writer.bytes(&self.srg_partial_bssid_bitmap);
    }
}

/// Spatial Reuse Request, `EasyMesh` §17.2.89 Table 111; audit rows 265–266.
/// Upstream `em_spatial_reuse_req_t` (`em_base.h:1472-1488`), sender
/// `em_channel.cpp:944-968` has no caller; the channel selection request handler skips this TLV
/// (`em_channel.cpp:2018-2053`). The codec reads all 29 required octets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpatialReuseRequest {
    /// Radio unique identifier.
    pub ruid: MacAddr,
    /// Color, flags, offsets and bitmaps.
    pub parameters: SpatialReuseParameters,
}

impl TlvBody<'_> for SpatialReuseRequest {
    const TLV_TYPE: TlvType = TlvType::SPATIAL_REUSE_REQUEST;

    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let ruid = reader.mac()?;
        let color = reader.u8()?;
        let parameters = SpatialReuseParameters::read(&mut reader, color)?;
        reader.skip(2)?;
        reader.finish()?;
        Ok(Self { ruid, parameters })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.mac(self.ruid);
        writer.u8(checked_bits(self.parameters.bss_color, 63)?);
        self.parameters.write(&mut writer);
        writer.zeros(2);
        Ok(writer.into_bytes())
    }
}

/// Spatial Reuse Report, `EasyMesh` §17.2.90 Table 112; audit rows 267–268.
/// Upstream `em_spatial_reuse_rprt_t` (`em_base.h:1384-1402`), sender
/// `em_channel.cpp:1166-1194` and receiver `:1752-1774` have no callers.
/// The codec requires all 37 octets instead of trusting the receiver's cast.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpatialReuseReport {
    /// Radio unique identifier.
    pub ruid: MacAddr,
    /// Partial BSS color flag.
    pub partial_bss_color: bool,
    /// Color, flags, offsets and bitmaps.
    pub parameters: SpatialReuseParameters,
    /// Raw bitmap of neighboring BSS colors in use.
    pub neighbor_bss_color_bitmap: [u8; 8],
}

impl TlvBody<'_> for SpatialReuseReport {
    const TLV_TYPE: TlvType = TlvType::SPATIAL_REUSE_REPORT;

    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let ruid = reader.mac()?;
        let color = reader.u8()?;
        let parameters = SpatialReuseParameters::read(&mut reader, color)?;
        let neighbor_bss_color_bitmap = reader.array()?;
        reader.skip(2)?;
        reader.finish()?;
        Ok(Self {
            ruid,
            partial_bss_color: color & 0x40 != 0,
            parameters,
            neighbor_bss_color_bitmap,
        })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.mac(self.ruid);
        writer
            .u8((u8::from(self.partial_bss_color) << 6)
                | checked_bits(self.parameters.bss_color, 63)?);
        self.parameters.write(&mut writer);
        writer.bytes(&self.neighbor_bss_color_bitmap);
        writer.zeros(2);
        Ok(writer.into_bytes())
    }
}

/// Spatial Reuse Config Response, `EasyMesh` §17.2.91 Table 113; audit row 269.
/// Upstream `em_spatial_reuse_cfg_rsp_t` (`em_base.h:1451-1453`) has no
/// production sender or receiver (`em_channel.cpp:1055-1105,2055-2095`);
/// reserved response codes are retained raw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpatialReuseConfigResponse {
    /// Radio unique identifier.
    pub ruid: MacAddr,
    /// Response code, including reserved values.
    pub response_code: u8,
}

impl TlvBody<'_> for SpatialReuseConfigResponse {
    const TLV_TYPE: TlvType = TlvType::SPATIAL_REUSE_CONFIG_RESPONSE;

    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let body = Self {
            ruid: reader.mac()?,
            response_code: reader.u8()?,
        };
        reader.finish()?;
        Ok(body)
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.mac(self.ruid);
        writer.u8(self.response_code);
        Ok(writer.into_bytes())
    }
}

/// Shared inquiry object bytes, `EasyMesh` §17.2.104 Table 126 and §17.2.105
/// Table 127; audit rows 297–300. Upstream `em_avail_spectrum_inquiry_req_t`
/// and `_rsp_t` (`em_base.h:2055-2061`) are pointers; sender
/// `em_channel.cpp:1649-1663` writes uninitialised pointer-sized bytes and no
/// receiver parses them. Unlike that sender, callers supply the object bytes;
/// JSON is not parsed or validated (0009 D5), but an empty value is rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpectrumInquiryObject<'a> {
    bytes: &'a [u8],
}

impl<'a> SpectrumInquiryObject<'a> {
    /// Creates an object from nonempty raw octets, without parsing JSON.
    #[must_use]
    pub const fn new(bytes: &'a [u8]) -> Option<Self> {
        if bytes.is_empty() {
            None
        } else {
            Some(Self { bytes })
        }
    }

    /// Returns the object's raw octets.
    #[must_use]
    pub const fn bytes(&self) -> &'a [u8] {
        self.bytes
    }

    fn decode_value(value: &'a [u8]) -> Result<Self, DecodeError> {
        if value.is_empty() {
            return Err(DecodeError::Truncated {
                needed: 1,
                remaining: 0,
            });
        }
        Ok(Self { bytes: value })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        if self.bytes.is_empty() {
            return Err(EncodeError::CountTooSmall { len: 0, min: 1 });
        }
        Ok(self.bytes.to_vec())
    }
}

tlv_newtype! {
    /// Available Spectrum Inquiry Request, `EasyMesh` §17.2.104 Table 126;
    /// audit rows 297–298. Upstream `em_avail_spectrum_inquiry_req_t`
    /// (`em_base.h:2055-2057`), sender `em_channel.cpp:1649-1655`, no receiver.
    /// Upstream writes uninitialised bytes; this body borrows the supplied raw object (0009 D5).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct AvailableSpectrumInquiryRequest<'a>(pub SpectrumInquiryObject<'a>) => TlvType::AVAILABLE_SPECTRUM_INQUIRY_REQUEST;
}

tlv_newtype! {
    /// Available Spectrum Inquiry Response, `EasyMesh` §17.2.105 Table 127;
    /// audit rows 299–300. Upstream `em_avail_spectrum_inquiry_rsp_t`
    /// (`em_base.h:2059-2061`), sender `em_channel.cpp:1657-1663`, no receiver.
    /// Upstream writes uninitialised bytes; this body borrows the supplied raw object (0009 D5).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct AvailableSpectrumInquiryResponse<'a>(pub SpectrumInquiryObject<'a>) => TlvType::AVAILABLE_SPECTRUM_INQUIRY_RESPONSE;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::body_fixtures::{self, Key, Row};
    use crate::body_testkit::{Field, sweep_row, truncated_in};
    use core::mem::size_of;

    const GROUP: &str = "cac";
    const RUID: MacAddr = MacAddr::new([0x02, 0xa0, 0xb1, 0xc2, 0xd3, 0xe0]);
    const RUID2: MacAddr = MacAddr::new([0x02, 0xa0, 0xb1, 0xc2, 0xd3, 0xf0]);
    const BSSID: MacAddr = MacAddr::new([0x3c, 0x1d, 0x2e, 0x3f, 0x4a, 0x51]);
    const TRANSMITTER: MacAddr = MacAddr::new([0x7a, 0x6b, 0x5c, 0x4d, 0x3e, 0x21]);
    const ZERO_MAC: MacAddr = MacAddr::new([0; 6]);
    const TOO_MANY: EncodeError = EncodeError::CountTooLarge { len: 256, max: 255 };
    const CANONICAL: &[(Key, Key)] = &[
        (
            ("0xad", "reserved-bits-set"),
            ("0xad", "reserved-bits-clear"),
        ),
        (
            ("0xd6", "reserved-octets-set"),
            ("0xd6", "reserved-octets-clear"),
        ),
        (
            ("0xd7", "reserved-octets-set"),
            ("0xd7", "reserved-octets-clear"),
        ),
        (("0xd8", "reserved-set"), ("0xd8", "reserved-clear")),
        (("0xd9", "reserved-set"), ("0xd9", "reserved-clear")),
    ];

    fn fixture(key: Key) -> Vec<u8> {
        body_fixtures::value(GROUP, key)
    }

    fn dispatch_row(row: &Row<'_>, bytes: &[u8], canonical: Option<&[u8]>) -> Result<(), String> {
        match row.tlv {
            "0xad" => sweep_row::<CacRequest>(row, bytes, canonical),
            "0xae" => sweep_row::<CacTermination>(row, bytes, canonical),
            "0xaf" => sweep_row::<CacCompletionReport>(row, bytes, canonical),
            "0xb1" => sweep_row::<CacStatusReport>(row, bytes, canonical),
            "0xd6" => sweep_row::<AnticipatedChannelPreference<'_>>(row, bytes, canonical),
            "0xd7" => sweep_row::<AnticipatedChannelUsage<'_>>(row, bytes, canonical),
            "0xd8" => sweep_row::<SpatialReuseRequest>(row, bytes, canonical),
            "0xd9" => sweep_row::<SpatialReuseReport>(row, bytes, canonical),
            "0xda" => sweep_row::<SpatialReuseConfigResponse>(row, bytes, canonical),
            "0xe8" => sweep_row::<AvailableSpectrumInquiryRequest<'_>>(row, bytes, canonical),
            "0xe9" => sweep_row::<AvailableSpectrumInquiryResponse<'_>>(row, bytes, canonical),
            other => Err(format!("no body for TLV {other}")),
        }
    }

    #[test]
    fn every_fixture_row_decodes_and_encodes_as_its_kind_says() {
        crate::body_testkit::sweep_group(GROUP, CANONICAL, dispatch_row);
    }

    fn request() -> CacRequest {
        CacRequest {
            radios: vec![
                CacRequestRadio {
                    ruid: RUID,
                    operating_class: 121,
                    channel: 100,
                    method: 1,
                    completion_action: 1,
                },
                CacRequestRadio {
                    ruid: RUID2,
                    operating_class: 128,
                    channel: 58,
                    method: 3,
                    completion_action: 0,
                },
            ],
        }
    }
    fn termination() -> CacTermination {
        CacTermination {
            radios: vec![
                CacTerminationRadio {
                    ruid: RUID,
                    operating_class: 121,
                    channel: 116,
                },
                CacTerminationRadio {
                    ruid: RUID2,
                    operating_class: 129,
                    channel: 50,
                },
            ],
        }
    }
    fn completion() -> CacCompletionReport {
        CacCompletionReport {
            radios: vec![
                CacCompletionRadio {
                    ruid: RUID,
                    operating_class: 121,
                    channel: 100,
                    status: 1,
                    detected_pairs: vec![
                        CacDetectedPair {
                            operating_class: 121,
                            channel: 100,
                        },
                        CacDetectedPair {
                            operating_class: 122,
                            channel: 106,
                        },
                    ],
                },
                CacCompletionRadio {
                    ruid: RUID2,
                    operating_class: 128,
                    channel: 42,
                    status: 0,
                    detected_pairs: vec![],
                },
            ],
        }
    }
    fn status() -> CacStatusReport {
        CacStatusReport {
            available: vec![
                CacAvailableChannel {
                    operating_class: 115,
                    channel: 36,
                    minutes: 0,
                },
                CacAvailableChannel {
                    operating_class: 121,
                    channel: 100,
                    minutes: 0x1234,
                },
            ],
            non_occupancy: vec![CacNonOccupancyChannel {
                operating_class: 118,
                channel: 52,
                duration: 1800,
            }],
            active: vec![CacActiveChannel {
                operating_class: 121,
                channel: 116,
                countdown: 0x0a_1b2c,
            }],
        }
    }
    fn preference() -> AnticipatedChannelPreference<'static> {
        AnticipatedChannelPreference {
            operating_classes: vec![
                AnticipatedOperatingClass {
                    operating_class: 115,
                    channels: &[36, 48],
                },
                AnticipatedOperatingClass {
                    operating_class: 128,
                    channels: &[42],
                },
            ],
        }
    }
    fn usage() -> AnticipatedChannelUsage<'static> {
        AnticipatedChannelUsage {
            operating_class: 128,
            channel: 42,
            reference_bssid: BSSID,
            usages: vec![
                AnticipatedUsage {
                    burst_start_time: 0x1234_5678,
                    burst_length: 20_000,
                    repetitions: 10,
                    burst_interval: 102_400,
                    ru_bitmask: &[0xff, 0xff, 0xff, 0xff, 0x1f],
                    transmitter: TRANSMITTER,
                    power_level: 20,
                    reason: 0,
                },
                AnticipatedUsage {
                    burst_start_time: 0x0a0b_0c0d,
                    burst_length: 1000,
                    repetitions: 0,
                    burst_interval: 0,
                    ru_bitmask: &[3, 1],
                    transmitter: ZERO_MAC,
                    power_level: -1,
                    reason: 4,
                },
            ],
        }
    }
    fn parameters() -> SpatialReuseParameters {
        SpatialReuseParameters {
            bss_color: 45,
            hesiga_value_15_allowed: true,
            srg_information_valid: true,
            non_srg_offset_valid: true,
            psr_disallowed: false,
            non_srg_obss_pd_max_offset: 20,
            srg_obss_pd_min_offset: 4,
            srg_obss_pd_max_offset: 18,
            srg_bss_color_bitmap: [0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef],
            srg_partial_bssid_bitmap: [0xfe, 0xdc, 0xba, 0x98, 0x76, 0x54, 0x32, 0x10],
        }
    }
    fn spatial_request() -> SpatialReuseRequest {
        SpatialReuseRequest {
            ruid: RUID,
            parameters: parameters(),
        }
    }
    fn spatial_report() -> SpatialReuseReport {
        SpatialReuseReport {
            ruid: RUID,
            partial_bss_color: true,
            parameters: parameters(),
            neighbor_bss_color_bitmap: [0, 0, 0x0c, 0, 0, 0xa0, 0, 1],
        }
    }
    fn spatial_response() -> SpatialReuseConfigResponse {
        SpatialReuseConfigResponse {
            ruid: RUID,
            response_code: 0,
        }
    }
    fn spectrum_request() -> AvailableSpectrumInquiryRequest<'static> {
        AvailableSpectrumInquiryRequest(
            SpectrumInquiryObject::new(
                br#"{"requestId":"7","deviceDescriptor":{"serialNumber":"SN-42"}}"#,
            )
            .unwrap(),
        )
    }
    fn spectrum_response() -> AvailableSpectrumInquiryResponse<'static> {
        AvailableSpectrumInquiryResponse(
            SpectrumInquiryObject::new(
                br#"{"requestId":"7","response":{"responseCode":0},"availableChannelInfo":[]}"#,
            )
            .unwrap(),
        )
    }
    const fn field(offset: usize, width: usize) -> Field {
        Field { offset, width }
    }

    fn many_requests() -> CacRequest {
        let mut body = request();
        body.radios.resize(256, body.radios[0]);
        body
    }
    fn many_terminations() -> CacTermination {
        let mut body = termination();
        body.radios.resize(256, body.radios[0]);
        body
    }
    fn many_completions() -> CacCompletionReport {
        CacCompletionReport {
            radios: (0..256)
                .map(|_| CacCompletionRadio {
                    ruid: RUID,
                    operating_class: 0,
                    channel: 0,
                    status: 0,
                    detected_pairs: vec![],
                })
                .collect(),
        }
    }
    fn many_status() -> CacStatusReport {
        let mut body = status();
        body.available.resize(256, body.available[0]);
        body
    }
    fn many_preferences() -> AnticipatedChannelPreference<'static> {
        let mut body = preference();
        body.operating_classes
            .resize(256, body.operating_classes[0]);
        body
    }
    fn many_usages() -> AnticipatedChannelUsage<'static> {
        let mut body = usage();
        body.usages.resize(256, body.usages[0]);
        body
    }

    body_checks! {
        mod cac_request_checks;
        body: CacRequest;
        golden: &fixture(("0xad", "golden")), request();
        prefixes: |_| false, truncated_in(&[field(0,1), field(1,6), field(7,1), field(8,1), field(9,1), field(10,6), field(16,1), field(17,1), field(18,1)]);
        trailing_byte: &fixture(("0xad", "golden"));
        count_too_large: many_requests(), TOO_MANY;
        reserved_bits: [(&fixture(("0xad", "reserved-bits-set")), CacRequest::decode_value(&fixture(("0xad", "reserved-bits-clear"))).unwrap(), &fixture(("0xad", "reserved-bits-clear")))];
    }
    body_checks! {
        mod cac_termination_checks;
        body: CacTermination;
        golden: &fixture(("0xae", "golden")), termination();
        prefixes: |_| false, truncated_in(&[field(0,1), field(1,6), field(7,1), field(8,1), field(9,6), field(15,1), field(16,1)]);
        trailing_byte: &fixture(("0xae", "golden"));
        count_too_large: many_terminations(), TOO_MANY;
    }
    body_checks! {
        mod completion_checks;
        body: CacCompletionReport;
        golden: &fixture(("0xaf", "golden")), completion();
        prefixes: |_| false, truncated_in(&[field(0,1), field(1,6), field(7,1), field(8,1), field(9,1), field(10,1), field(11,1), field(12,1), field(13,1), field(14,1), field(15,6), field(21,1), field(22,1), field(23,1), field(24,1)]);
        trailing_byte: &fixture(("0xaf", "golden"));
        count_too_large: many_completions(), TOO_MANY;
    }
    body_checks! {
        mod status_checks;
        body: CacStatusReport;
        golden: &fixture(("0xb1", "golden")), status();
        prefixes: |_| false, truncated_in(&[field(0,1), field(1,1), field(2,1), field(3,2), field(5,1), field(6,1), field(7,2), field(9,1), field(10,1), field(11,1), field(12,2), field(14,1), field(15,1), field(16,1), field(17,3)]);
        trailing_byte: &fixture(("0xb1", "golden"));
        count_too_large: many_status(), TOO_MANY;
    }
    body_checks! {
        mod preference_checks;
        body: AnticipatedChannelPreference<'a>;
        golden: &fixture(("0xd6", "golden")), preference();
        prefixes: |_| false, truncated_in(&[field(0,1), field(1,1), field(2,1), field(3,2), field(5,4), field(9,1), field(10,1), field(11,1), field(12,4)]);
        trailing_byte: &fixture(("0xd6", "golden"));
        count_too_large: many_preferences(), TOO_MANY;
        reserved_bits: [(&fixture(("0xd6", "reserved-octets-set")), AnticipatedChannelPreference::decode_value(&fixture(("0xd6", "reserved-octets-clear"))).unwrap(), &fixture(("0xd6", "reserved-octets-clear")))];
    }
    body_checks! {
        mod usage_checks;
        body: AnticipatedChannelUsage<'a>;
        golden: &fixture(("0xd7", "golden")), usage();
        prefixes: |_| false, truncated_in(&[field(0,1), field(1,1), field(2,6), field(8,1), field(9,4), field(13,4), field(17,4), field(21,4), field(25,1), field(26,5), field(31,6), field(37,1), field(38,1), field(39,4), field(43,4), field(47,4), field(51,4), field(55,4), field(59,1), field(60,2), field(62,6), field(68,1), field(69,1), field(70,4)]);
        trailing_byte: &fixture(("0xd7", "golden"));
        count_too_large: many_usages(), TOO_MANY;
        reserved_bits: [(&fixture(("0xd7", "reserved-octets-set")), AnticipatedChannelUsage::decode_value(&fixture(("0xd7", "reserved-octets-clear"))).unwrap(), &fixture(("0xd7", "reserved-octets-clear")))];
    }
    body_checks! {
        mod spatial_request_checks;
        body: SpatialReuseRequest;
        golden: &fixture(("0xd8", "golden")), spatial_request();
        prefixes: |_| false, truncated_in(&[field(0,6), field(6,1), field(7,1), field(8,1), field(9,1), field(10,1), field(11,8), field(19,8), field(27,2)]);
        trailing_byte: &fixture(("0xd8", "golden"));
        reserved_bits: [(&fixture(("0xd8", "reserved-set")), spatial_request(), &fixture(("0xd8", "reserved-clear")))];
    }
    body_checks! {
        mod spatial_report_checks;
        body: SpatialReuseReport;
        golden: &fixture(("0xd9", "golden")), spatial_report();
        prefixes: |_| false, truncated_in(&[field(0,6), field(6,1), field(7,1), field(8,1), field(9,1), field(10,1), field(11,8), field(19,8), field(27,8), field(35,2)]);
        trailing_byte: &fixture(("0xd9", "golden"));
        reserved_bits: [(&fixture(("0xd9", "reserved-set")), spatial_report(), &fixture(("0xd9", "reserved-clear")))];
    }
    body_checks! {
        mod spatial_response_checks;
        body: SpatialReuseConfigResponse;
        golden: &fixture(("0xda", "golden")), spatial_response();
        prefixes: |_| false, truncated_in(&[field(0,6), field(6,1)]);
        trailing_byte: &fixture(("0xda", "golden"));
    }
    body_checks! {
        mod spectrum_request_checks;
        body: AvailableSpectrumInquiryRequest<'a>;
        golden: &fixture(("0xe8", "golden")), spectrum_request();
        prefixes: |len| len > 0, truncated_in(&[field(0,1)]);
    }
    body_checks! {
        mod spectrum_response_checks;
        body: AvailableSpectrumInquiryResponse<'a>;
        golden: &fixture(("0xe9", "golden")), spectrum_response();
        prefixes: |len| len > 0, truncated_in(&[field(0,1)]);
    }

    /// A wire flag: octet offset in the value, bit mask, and a setter for the matching field.
    type Flag<B> = (usize, u8, fn(&mut B));

    /// Checks that each flag, set alone in an otherwise zero value, decodes to its own field and
    /// encodes back to the same bytes. Fixture rows that set several flags at once cannot tell
    /// two flags apart when the pair is always set or clear together.
    fn assert_each_flag_alone<B>(zero_bytes: &[u8], zero_body: fn() -> B, flags: &[Flag<B>])
    where
        B: for<'a> TlvBody<'a> + core::fmt::Debug + PartialEq,
    {
        assert_eq!(B::decode_value(zero_bytes), Ok(zero_body()));
        for &(offset, mask, set) in flags {
            let mut bytes = zero_bytes.to_vec();
            bytes[offset] = mask;
            let mut expected = zero_body();
            set(&mut expected);
            assert_eq!(
                B::decode_value(&bytes),
                Ok(expected),
                "decode offset {offset} bit {mask:#04x}"
            );
            let mut expected = zero_body();
            set(&mut expected);
            assert_eq!(
                expected.encode_value(),
                Ok(bytes),
                "encode offset {offset} bit {mask:#04x}"
            );
        }
    }

    fn zero_request() -> CacRequest {
        CacRequest {
            radios: vec![CacRequestRadio {
                ruid: ZERO_MAC,
                operating_class: 0,
                channel: 0,
                method: 0,
                completion_action: 0,
            }],
        }
    }

    #[test]
    fn cac_request_method_and_action_each_use_their_own_bits() {
        let flags: &[Flag<CacRequest>] = &[
            (9, 0x20, |b| b.radios[0].method = 1),
            (9, 0x40, |b| b.radios[0].method = 2),
            (9, 0x80, |b| b.radios[0].method = 4),
            (9, 0x08, |b| b.radios[0].completion_action = 1),
            (9, 0x10, |b| b.radios[0].completion_action = 2),
        ];
        assert_each_flag_alone(&[1, 0, 0, 0, 0, 0, 0, 0, 0, 0], zero_request, flags);
    }

    const fn zero_parameters() -> SpatialReuseParameters {
        SpatialReuseParameters {
            bss_color: 0,
            hesiga_value_15_allowed: false,
            srg_information_valid: false,
            non_srg_offset_valid: false,
            psr_disallowed: false,
            non_srg_obss_pd_max_offset: 0,
            srg_obss_pd_min_offset: 0,
            srg_obss_pd_max_offset: 0,
            srg_bss_color_bitmap: [0; 8],
            srg_partial_bssid_bitmap: [0; 8],
        }
    }
    fn zero_spatial_request() -> SpatialReuseRequest {
        SpatialReuseRequest {
            ruid: ZERO_MAC,
            parameters: zero_parameters(),
        }
    }
    fn zero_spatial_report() -> SpatialReuseReport {
        SpatialReuseReport {
            ruid: ZERO_MAC,
            partial_bss_color: false,
            parameters: zero_parameters(),
            neighbor_bss_color_bitmap: [0; 8],
        }
    }

    #[test]
    fn spatial_request_color_and_flags_each_use_their_own_bits() {
        let flags: &[Flag<SpatialReuseRequest>] = &[
            (6, 0x01, |b| b.parameters.bss_color = 1),
            (6, 0x02, |b| b.parameters.bss_color = 2),
            (6, 0x04, |b| b.parameters.bss_color = 4),
            (6, 0x08, |b| b.parameters.bss_color = 8),
            (6, 0x10, |b| b.parameters.bss_color = 16),
            (6, 0x20, |b| b.parameters.bss_color = 32),
            (7, 0x10, |b| b.parameters.hesiga_value_15_allowed = true),
            (7, 0x08, |b| b.parameters.srg_information_valid = true),
            (7, 0x04, |b| b.parameters.non_srg_offset_valid = true),
            (7, 0x01, |b| b.parameters.psr_disallowed = true),
        ];
        assert_each_flag_alone(&[0; 29], zero_spatial_request, flags);
    }

    #[test]
    fn spatial_report_color_and_flags_each_use_their_own_bits() {
        let flags: &[Flag<SpatialReuseReport>] = &[
            (6, 0x01, |b| b.parameters.bss_color = 1),
            (6, 0x02, |b| b.parameters.bss_color = 2),
            (6, 0x04, |b| b.parameters.bss_color = 4),
            (6, 0x08, |b| b.parameters.bss_color = 8),
            (6, 0x10, |b| b.parameters.bss_color = 16),
            (6, 0x20, |b| b.parameters.bss_color = 32),
            (6, 0x40, |b| b.partial_bss_color = true),
            (7, 0x10, |b| b.parameters.hesiga_value_15_allowed = true),
            (7, 0x08, |b| b.parameters.srg_information_valid = true),
            (7, 0x04, |b| b.parameters.non_srg_offset_valid = true),
            (7, 0x01, |b| b.parameters.psr_disallowed = true),
        ];
        assert_each_flag_alone(&[0; 37], zero_spatial_report, flags);
    }

    #[test]
    fn all_nested_counts_are_checked_before_encoding() {
        let mut body = completion();
        body.radios[0].detected_pairs.resize(
            256,
            CacDetectedPair {
                operating_class: 1,
                channel: 2,
            },
        );
        assert_eq!(body.encode_value(), Err(TOO_MANY));

        let bytes = [0; 256];
        let mut body = preference();
        body.operating_classes[0].channels = &bytes;
        assert_eq!(body.encode_value(), Err(TOO_MANY));
        let mut body = usage();
        body.usages[0].ru_bitmask = &bytes;
        assert_eq!(body.encode_value(), Err(TOO_MANY));
    }

    #[test]
    fn cac_status_count_overflow_is_an_intentional_difference_from_upstream_wrapping() {
        let mut body = status();
        body.available.resize(256, body.available[0]);
        assert_eq!(body.encode_value(), Err(TOO_MANY));
        body.available.clear();
        body.non_occupancy.resize(256, body.non_occupancy[0]);
        assert_eq!(body.encode_value(), Err(TOO_MANY));
        body.non_occupancy.clear();
        body.active.resize(256, body.active[0]);
        assert_eq!(body.encode_value(), Err(TOO_MANY));
        body.active.truncate(255);
        let bytes = body.encode_value().unwrap();
        assert_eq!(bytes[2], 255);
        assert_eq!(CacStatusReport::decode_value(&bytes), Ok(body));
    }

    #[test]
    fn cac_request_bit_two_is_reserved_unlike_upstreams_three_bit_action() {
        let bytes = [1, 0, 0, 0, 0, 0, 0, 0, 0, 4];
        let decoded = CacRequest::decode_value(&bytes).unwrap();
        assert_eq!(decoded, zero_request());
        assert_eq!(
            decoded.encode_value().unwrap(),
            [1, 0, 0, 0, 0, 0, 0, 0, 0, 0]
        );
    }

    #[test]
    fn counts_at_the_wire_limit_are_not_capped_like_upstream_arrays() {
        let mut request = request();
        request.radios.resize(255, request.radios[0]);
        let bytes = request.encode_value().unwrap();
        assert_eq!(CacRequest::decode_value(&bytes), Ok(request));

        let mut termination = termination();
        termination.radios.resize(255, termination.radios[0]);
        let bytes = termination.encode_value().unwrap();
        assert_eq!(CacTermination::decode_value(&bytes), Ok(termination));

        let mut completion = completion();
        completion.radios[0].detected_pairs.resize(
            255,
            CacDetectedPair {
                operating_class: 1,
                channel: 2,
            },
        );
        let bytes = completion.encode_value().unwrap();
        assert_eq!(CacCompletionReport::decode_value(&bytes), Ok(completion));

        let mut status = status();
        status.available.resize(255, status.available[0]);
        status.non_occupancy.resize(255, status.non_occupancy[0]);
        status.active.resize(255, status.active[0]);
        let bytes = status.encode_value().unwrap();
        assert_eq!(CacStatusReport::decode_value(&bytes), Ok(status));

        let mut preference = preference();
        preference.operating_classes[0].channels = &[1; 255];
        preference.operating_classes.resize(
            255,
            AnticipatedOperatingClass {
                operating_class: 81,
                channels: &[],
            },
        );
        let bytes = preference.encode_value().unwrap();
        assert_eq!(
            AnticipatedChannelPreference::decode_value(&bytes),
            Ok(preference)
        );

        let mut usage = usage();
        usage.usages.resize(255, usage.usages[0]);
        let bytes = usage.encode_value().unwrap();
        assert_eq!(AnticipatedChannelUsage::decode_value(&bytes), Ok(usage));
    }

    #[test]
    fn zero_counts_are_accepted_despite_upstreams_validation_minimums() {
        assert_eq!(
            CacStatusReport::decode_value(&[0, 0, 0]),
            Ok(CacStatusReport {
                available: vec![],
                non_occupancy: vec![],
                active: vec![],
            })
        );
        assert_eq!(
            AnticipatedChannelPreference::decode_value(&[0]),
            Ok(AnticipatedChannelPreference {
                operating_classes: vec![],
            })
        );
        assert_eq!(
            AnticipatedChannelUsage::decode_value(&[0; 9]),
            Ok(AnticipatedChannelUsage {
                operating_class: 0,
                channel: 0,
                reference_bssid: ZERO_MAC,
                usages: vec![],
            })
        );
    }

    #[test]
    fn bit_fields_reject_values_that_do_not_fit() {
        let mut body = request();
        body.radios[0].method = 8;
        assert_eq!(
            body.encode_value(),
            Err(EncodeError::ValueTooLarge { value: 8, max: 7 })
        );
        body.radios[0].method = 0;
        body.radios[0].completion_action = 4;
        assert_eq!(
            body.encode_value(),
            Err(EncodeError::ValueTooLarge { value: 4, max: 3 })
        );
        let mut body = spatial_request();
        body.parameters.bss_color = 64;
        assert_eq!(
            body.encode_value(),
            Err(EncodeError::ValueTooLarge { value: 64, max: 63 })
        );
        let mut body = spatial_report();
        body.parameters.bss_color = 64;
        assert_eq!(
            body.encode_value(),
            Err(EncodeError::ValueTooLarge { value: 64, max: 63 })
        );
    }

    #[test]
    fn countdown_above_24_bits_is_an_error_where_upstream_truncates() {
        // intentional-difference: upstream em_channel.cpp:1449-1451 writes
        // only the low 24 bits, so 0x0100_0000 would go out as 00 00 00.
        let mut body = status();
        body.active[0].countdown = 0x0100_0000;
        assert_eq!(
            body.encode_value(),
            Err(EncodeError::ValueTooLarge {
                value: 0x0100_0000,
                max: 0xff_ffff
            })
        );
        body.active[0].countdown = 0xff_ffff;
        let bytes = body.encode_value().unwrap();
        assert_eq!(CacStatusReport::decode_value(&bytes), Ok(body));
    }

    #[test]
    fn spectrum_objects_are_supplied_instead_of_upstreams_uninitialized_pointer_sized_bytes() {
        assert_eq!(SpectrumInquiryObject::new(&[]), None);
        let invalid = SpectrumInquiryObject { bytes: &[] };
        assert_eq!(
            AvailableSpectrumInquiryRequest(invalid).encode_value(),
            Err(EncodeError::CountTooSmall { len: 0, min: 1 })
        );
        assert_eq!(
            AvailableSpectrumInquiryResponse(invalid).encode_value(),
            Err(EncodeError::CountTooSmall { len: 0, min: 1 })
        );
        let input = [0, 0xff, 42];
        let body = AvailableSpectrumInquiryRequest::decode_value(&input).unwrap();
        assert_eq!(body.0.bytes(), &input);
        assert_eq!(body.0.bytes().as_ptr(), input.as_ptr());
        assert_eq!(body.encode_value().unwrap(), input);
    }

    #[test]
    fn spectrum_objects_can_fill_a_tlv_and_remain_borrowed() {
        let bytes = vec![0xa5; 65_535];
        let body = AvailableSpectrumInquiryResponse(SpectrumInquiryObject::new(&bytes).unwrap());
        let message = crate::body_testkit::build_message(&body).unwrap();
        let tlv = crate::body_testkit::single_tlv(&message).unwrap();
        let decoded = tlv
            .decode::<AvailableSpectrumInquiryResponse<'_>>()
            .unwrap();
        assert_eq!(decoded.0.bytes(), bytes);
        assert_eq!(decoded.0.bytes().as_ptr(), tlv.value.as_ptr());
    }

    #[test]
    fn zero_and_unlisted_ru_lengths_borrow_the_frame_and_round_trip() {
        for mask in [&[][..], &[1, 2, 3, 4][..], &[0xff; 255][..]] {
            let mut body = usage();
            body.usages.truncate(1);
            body.usages[0].ru_bitmask = mask;
            let bytes = body.encode_value().unwrap();
            let decoded = AnticipatedChannelUsage::decode_value(&bytes).unwrap();
            assert_eq!(decoded.usages[0].ru_bitmask, mask);
            assert_eq!(decoded.usages[0].ru_bitmask.as_ptr(), bytes[26..].as_ptr());
            assert_eq!(decoded, body);
        }
    }

    #[test]
    fn bodies_and_records_have_bounded_stack_sizes() {
        let word = size_of::<usize>();
        assert!(size_of::<CacRequestRadio>() <= 16);
        assert!(size_of::<CacRequest>() <= 3 * word);
        assert!(size_of::<CacTerminationRadio>() <= 8);
        assert!(size_of::<CacTermination>() <= 3 * word);
        assert!(size_of::<CacDetectedPair>() <= 2);
        assert!(size_of::<CacCompletionRadio>() <= 5 * word);
        assert!(size_of::<CacCompletionReport>() <= 3 * word);
        assert!(size_of::<CacAvailableChannel>() <= 8);
        assert!(size_of::<CacNonOccupancyChannel>() <= 8);
        assert!(size_of::<CacActiveChannel>() <= 8);
        assert!(size_of::<CacStatusReport>() <= 9 * word);
        assert!(size_of::<AnticipatedOperatingClass<'_>>() <= 3 * word);
        assert!(size_of::<AnticipatedChannelPreference<'_>>() <= 3 * word);
        assert!(size_of::<AnticipatedUsage<'_>>() <= 6 * word);
        assert!(size_of::<AnticipatedChannelUsage<'_>>() <= 5 * word);
        assert!(size_of::<SpatialReuseParameters>() <= 32);
        assert!(size_of::<SpatialReuseRequest>() <= 40);
        assert!(size_of::<SpatialReuseReport>() <= 48);
        assert!(size_of::<SpatialReuseConfigResponse>() <= 8);
        assert!(size_of::<SpectrumInquiryObject<'_>>() <= 2 * word);
        assert!(size_of::<AvailableSpectrumInquiryRequest<'_>>() <= 2 * word);
        assert!(size_of::<AvailableSpectrumInquiryResponse<'_>>() <= 2 * word);
    }
}
