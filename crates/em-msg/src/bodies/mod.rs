// SPDX-License-Identifier: Apache-2.0

//! Typed TLV bodies (design note 0009, D2).
//!
//! This module holds the traits every body implements and the bodies so far:
//! the autoconfiguration, search, topology, profile, WSC and operational BSS bodies, in the
//! `autoconfig` submodule, the IEEE 1905.1 identity and neighbor bodies, in the `ieee1905`
//! submodule, the link metric and push button bodies, in the `linkmetric` submodule, the
//! capability bodies, in the `capability` submodule, the channel selection and channel scan
//! bodies, in the `channel` submodule, the CAC, anticipated channel, spatial reuse and
//! spectrum inquiry bodies, in the `cac` submodule, the metrics bodies, in the `metrics`
//! submodule, the steering, client association control, backhaul steering and code bodies,
//! in the `steering` submodule, and the policy and `QoS` bodies, in the `policy` submodule.
//! Other bodies come in later batches.
//!
//! A body decodes from [`Tlv::value`](crate::Tlv::value) and encodes to the bytes that
//! [`MsgBuilder::tlv`](crate::MsgBuilder::tlv) takes. The convenience entry points check the TLV
//! type too: [`Tlv::decode`](crate::Tlv::decode),
//! [`Tlv::decode_lenient`](crate::Tlv::decode_lenient) and
//! [`MsgBuilder::body`](crate::MsgBuilder::body).

use em_types::TlvType;

use crate::error::{DecodeError, EncodeError};

mod autoconfig;
mod cac;
mod capability;
mod channel;
mod ieee1905;
mod linkmetric;
mod metrics;
mod policy;
mod steering;

pub use autoconfig::{
    Agent, AgentList, AgentSecurity, ApOperationalBss, ApRadioIdentifier, AssociatedBss,
    AssociatedClient, AssociatedClients, AutoconfigFreqBand, Bssid, FrequencyBand, MacValue,
    MultiApProfile, OperationalBss, OperationalRadio, Profile2ErrorCode, Profile2ErrorDetail,
    Profile2ErrorReason, Role, RoleValue, SearchedRole, SearchedService, ServiceList,
    SupportedFreqBand, SupportedRole, SupportedService, VendorBss, VendorOperationalBss,
    VendorRadio, Wsc,
};
pub use cac::*;
pub use capability::{
    ApCapability, ApHeCapabilities, ApHtCapabilities, ApRadioAdvancedCapabilities,
    ApRadioBasicCapabilities, ApVhtCapabilities, ApWifi6Capabilities, BackhaulStaRadioCapabilities,
    BasicOperatingClass, CacCapabilities, CacMethod, CacOperatingClass, CacRadio,
    ClientCapabilityReport, ClientInfo, ControllerCapability, DeviceInventory, HeMcsSet, HeMcsSets,
    InventoryRadio, Profile2ApCapability, Wifi6Role, Wifi7AgentCapabilities, Wifi7FrequencyRecord,
    Wifi7ModeCapabilities, Wifi7Radio,
};
pub use channel::*;
pub use ieee1905::{
    AlMacAddress, BridgingTuple, DeviceBridgingCapability, DeviceInformation, Ieee1905Neighbor,
    Ieee1905NeighborDevice, LocalInterface, MacAddress, Non1905NeighborDeviceList, VendorSpecific,
};
pub use linkmetric::{
    LinkMetricQuery, LinkMetricResultCode, PushButtonEventNotification, PushButtonJoinNotification,
    PushButtonMedia, ReceiverLinkMetric, ReceiverLinkMetricPair, TransmitterLinkMetric,
    TransmitterLinkMetricPair,
};
pub use metrics::{
    ApChannelReport, ApExtendedMetrics, ApMetrics, ApMetricsQuery, AssociatedStaBssMetrics,
    AssociatedStaExtendedLinkMetrics, AssociatedStaLinkMetrics, AssociatedStaTrafficStats,
    AssociatedWifi6StaStatusReport, BeaconMetricsQuery, BeaconMetricsResponse,
    ExtendedLinkMetricsBss, MeasurementReportElement, MetricCollectionInterval, RadioMetrics,
    StaMacAddress, UnassociatedQueryChannel, UnassociatedStaLinkMetricsQuery,
    UnassociatedStaLinkMetricsResponse, UnassociatedStaMetrics, Wifi6TidQueueSize,
};
pub use policy::{
    BackhaulBssConfiguration, Default8021QSettings, DscpMappingTable, MetricReportingPolicy,
    QosManagementDescriptor, QosManagementPolicy, RadioMetricReportingPolicy, RadioSteeringPolicy,
    ServicePrioritizationRule, SteeringPolicy, TrafficSeparationPolicy, TrafficSeparationSsid,
    UnsuccessfulAssociationPolicy,
};
pub use steering::{
    AssociationStatusEntry, AssociationStatusNotification, BackhaulSteeringRequest,
    BackhaulSteeringResponse, ClientAssociationControlRequest, ClientAssociationEvent, ErrorCode,
    HigherLayerData, Profile2SteeringRequest, Profile2SteeringTarget, ReasonCode, StatusCode,
    SteeringBtmReport, SteeringFlags, SteeringMode, SteeringRequest, SteeringTarget,
};

/// Returns an empty `Vec` with room for at most `count` records of `T`, and for no more records
/// than `remaining` input bytes can hold. The reservation is therefore at most `remaining` bytes:
/// `capacity × size_of::<T>() ≤ remaining`.
///
/// `wire_size` is the smallest size of one record on the wire. In memory a record can be larger
/// (a bridging tuple, which is a list of MACs, is one octet on the wire and 24 bytes in memory),
/// so the capacity divides `remaining` by the larger of the two sizes. The `Vec` still grows past
/// this capacity if the input really holds more records, which happens when a record is smaller
/// on the wire than in memory. A decoded list can then take more memory than `remaining`.
pub(crate) fn bounded_vec<T>(count: usize, remaining: usize, wire_size: usize) -> Vec<T> {
    let record_size = wire_size.max(core::mem::size_of::<T>());
    Vec::with_capacity(count.min(remaining / record_size))
}

/// A typed TLV body.
///
/// `decode_value` follows the spec (note 0009, D4): a value that is too short is an error, and so
/// is a byte left over after a fixed layout. A body whose last field runs to the end of the value
/// takes those bytes instead. `encode_value` writes the spec bytes and returns them only if
/// the whole body was encoded.
pub trait TlvBody<'a>: Sized {
    /// The TLV type this body is carried in.
    const TLV_TYPE: TlvType;

    /// Decodes the value of a TLV (the bytes after the 3-byte TLV header).
    ///
    /// # Errors
    ///
    /// A [`DecodeError`] if the value is not a complete, valid body.
    fn decode_value(value: &'a [u8]) -> Result<Self, DecodeError>;

    /// Encodes the body into the value of a TLV.
    ///
    /// # Errors
    ///
    /// An [`EncodeError`] if a field does not fit its wire form.
    fn encode_value(&self) -> Result<Vec<u8>, EncodeError>;
}

/// A body that also has a lenient form, for values that upstream senders produce and the spec
/// does not allow.
///
/// The lenient decoders accept the listed non-spec sender forms only. They do not copy every
/// upstream receiver check: upstream rejects a Multi-AP Profile of 4 or more, for example, and
/// [`MultiApProfile`] keeps the raw value (note 0009, D2).
///
/// A body that implements this trait must say so in its `body_checks!` invocation with the
/// `lenient;` marker. Without the marker, the body's test run fails (the `body_is_not_lenient`
/// test), so a lenient body cannot pass the strict checks by leaving the marker out.
pub trait TlvBodyLenient<'a>: TlvBody<'a> {
    /// Decodes the value, accepting the listed non-spec sender forms as well as the spec form.
    ///
    /// `Ok(None)` is upstream's absent form for a TLV whose spec fields are
    /// mandatory: the consumer treats it as "TLV absent". It is returned only
    /// for the listed absent forms: an empty value, and for the Associated
    /// Wi-Fi 6 STA Status Report (0xB0) the nine zero octets upstream sends
    /// (note 0009, A7). Every other lenient form decodes to `Ok(Some(body))`.
    ///
    /// # Errors
    ///
    /// A [`DecodeError`] if the value is not valid even in the lenient form.
    fn decode_value_lenient(value: &'a [u8]) -> Result<Option<Self>, DecodeError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::body_testkit::{
        LENIENT_SAMPLE_TLV_TYPE, LenientSample, SAMPLE_TLV_TYPE, Sample, TAIL_SAMPLE_TLV_TYPE,
        TailSample,
    };
    use crate::builder::{MsgBuilder, RAW_BUDGET};
    use crate::error::BuildError;
    use crate::header::CmduHeader;
    use crate::tlv::Tlv;
    use core::mem::size_of;
    use em_types::MsgType;

    #[test]
    fn bounded_vec_never_reserves_more_than_the_input_holds() {
        // An empty bridging tuple is one octet on the wire and larger in memory.
        let tuples: Vec<BridgingTuple> = bounded_vec(255, 255, 1);
        assert!(size_of::<BridgingTuple>() > 1);
        assert!(tuples.capacity() * size_of::<BridgingTuple>() <= 255);
        // A record whose wire size is the larger one keeps the wire size.
        let bytes: Vec<u8> = bounded_vec(255, 100, 3);
        assert_eq!(bytes.capacity(), 33);
        // The wire count bounds the capacity when the input could hold more.
        let few: Vec<u8> = bounded_vec(2, 255, 1);
        assert_eq!(few.capacity(), 2);
        let none: Vec<BridgingTuple> = bounded_vec(255, 0, 1);
        assert_eq!(none.capacity(), 0);
    }

    #[test]
    fn largest_bridging_count_decodes_within_the_input_size() {
        // 255 empty tuples fill a 256-octet value. The pre-allocation is not observable on
        // the result, because the decoded tuples are larger than their wire form.
        let mut value = vec![0_u8; 256];
        value[0] = 255;
        let full = DeviceBridgingCapability::decode_value(&value).unwrap();
        assert_eq!(full.tuples.len(), 255);
        assert_eq!(
            DeviceBridgingCapability::decode_value(&value[..255]),
            Err(DecodeError::Truncated {
                needed: 1,
                remaining: 0
            })
        );
    }

    /// Wire bytes of a [`Sample`] with the default fields and no entries.
    const EMPTY_SAMPLE_BYTES: [u8; 4] = [0, 0, 0, 0];

    fn header() -> CmduHeader {
        CmduHeader {
            msg_type: MsgType::TOPOLOGY_QUERY,
            msg_id: 1,
            frag_id: 0,
            last_fragment: true,
            relay: false,
        }
    }

    fn too_many_entries() -> Sample {
        let mut sample = Sample::default();
        sample.entries.resize(
            256,
            crate::body_testkit::Entry {
                channel: 1,
                weight: 1,
            },
        );
        sample
    }

    /// A body whose encoding panics, to show that a builder in error never encodes.
    struct PanicsOnEncode;

    impl TlvBody<'_> for PanicsOnEncode {
        const TLV_TYPE: TlvType = TlvType(0xFC);

        fn decode_value(_value: &[u8]) -> Result<Self, DecodeError> {
            Ok(Self)
        }

        fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
            panic!("encode_value was called on a builder that already holds an error");
        }
    }

    #[test]
    fn decode_gives_the_body() {
        let tlv = Tlv {
            tlv_type: SAMPLE_TLV_TYPE,
            value: &EMPTY_SAMPLE_BYTES,
        };
        assert_eq!(tlv.decode::<Sample>(), Ok(Sample::default()));
    }

    #[test]
    fn decode_of_the_wrong_type_is_wrong_type() {
        let tlv = Tlv {
            tlv_type: TAIL_SAMPLE_TLV_TYPE,
            value: &EMPTY_SAMPLE_BYTES,
        };
        assert_eq!(
            tlv.decode::<Sample>(),
            Err(DecodeError::WrongType {
                expected: SAMPLE_TLV_TYPE,
                found: TAIL_SAMPLE_TLV_TYPE,
            })
        );
    }

    #[test]
    fn decode_checks_the_type_before_the_value() {
        let tlv = Tlv {
            tlv_type: TAIL_SAMPLE_TLV_TYPE,
            value: &[0xFF],
        };
        assert!(matches!(
            tlv.decode::<Sample>(),
            Err(DecodeError::WrongType { .. })
        ));
    }

    #[test]
    fn decode_passes_on_the_body_error() {
        let tlv = Tlv {
            tlv_type: SAMPLE_TLV_TYPE,
            value: &[0x00],
        };
        assert_eq!(
            tlv.decode::<Sample>(),
            Err(DecodeError::Truncated {
                needed: 2,
                remaining: 0
            })
        );
    }

    #[test]
    fn decode_lenient_returns_none_for_the_empty_value() {
        let tlv = Tlv {
            tlv_type: LENIENT_SAMPLE_TLV_TYPE,
            value: &[],
        };
        assert_eq!(tlv.decode_lenient::<LenientSample>(), Ok(None));
        assert!(tlv.decode::<LenientSample>().is_err());
    }

    #[test]
    fn decode_lenient_returns_some_for_a_value() {
        let tlv = Tlv {
            tlv_type: LENIENT_SAMPLE_TLV_TYPE,
            value: &EMPTY_SAMPLE_BYTES,
        };
        assert_eq!(
            tlv.decode_lenient::<LenientSample>(),
            Ok(Some(LenientSample(Sample::default())))
        );
    }

    #[test]
    fn decode_lenient_of_the_wrong_type_is_wrong_type() {
        let tlv = Tlv {
            tlv_type: SAMPLE_TLV_TYPE,
            value: &[],
        };
        assert_eq!(
            tlv.decode_lenient::<LenientSample>(),
            Err(DecodeError::WrongType {
                expected: LENIENT_SAMPLE_TLV_TYPE,
                found: SAMPLE_TLV_TYPE,
            })
        );
    }

    #[test]
    fn tail_body_decodes_through_tlv() {
        let value = [0, 0, 0, 0, 9, 9];
        let tlv = Tlv {
            tlv_type: TAIL_SAMPLE_TLV_TYPE,
            value: &value,
        };
        let body = tlv.decode::<TailSample<'_>>().unwrap();
        assert_eq!(body.tail, &[9, 9]);
    }

    #[test]
    fn builder_body_appends_a_tlv_of_the_body_type() {
        let mut builder = MsgBuilder::new(&header(), RAW_BUDGET);
        builder.body(&Sample::default());
        let message = builder.finish().unwrap();
        let mut expected = vec![0x00, 0x00, 0x00, 0x02, 0x00, 0x01, 0x00, 0x80];
        expected.extend_from_slice(&[SAMPLE_TLV_TYPE.0, 0x00, 0x04, 0, 0, 0, 0]);
        expected.extend_from_slice(&[0, 0, 0]);
        assert_eq!(message, expected);
    }

    #[test]
    fn builder_keeps_an_encode_failure_as_the_first_error() {
        let mut builder = MsgBuilder::new(&header(), RAW_BUDGET);
        builder
            .body(&too_many_entries())
            .tlv(TlvType::END_OF_MESSAGE, &[]);
        assert_eq!(
            builder.finish(),
            Err(BuildError::Encode(EncodeError::CountTooLarge {
                len: 256,
                max: 255
            }))
        );
    }

    #[test]
    fn builder_keeps_an_earlier_error_over_an_encode_failure() {
        let mut builder = MsgBuilder::new(&header(), RAW_BUDGET);
        builder
            .tlv(TlvType::END_OF_MESSAGE, &[])
            .body(&too_many_entries());
        assert_eq!(builder.finish(), Err(BuildError::EndOfMessageNotAllowed));
    }

    #[test]
    fn builder_in_error_does_not_encode() {
        let mut builder = MsgBuilder::new(&header(), 7);
        builder.body(&PanicsOnEncode);
        assert_eq!(builder.finish(), Err(BuildError::OverBudget));
    }

    #[test]
    fn builder_reports_over_budget_for_an_encoded_body() {
        // Header (8) plus the TLV (3 + 4) is 15, which does not fit in 14.
        let mut builder = MsgBuilder::new(&header(), 14);
        builder.body(&Sample::default());
        assert_eq!(builder.finish(), Err(BuildError::OverBudget));
    }

    #[test]
    fn builder_refuses_end_of_message_as_a_body_type() {
        struct EndOfMessageBody;
        impl TlvBody<'_> for EndOfMessageBody {
            const TLV_TYPE: TlvType = TlvType::END_OF_MESSAGE;
            fn decode_value(_value: &[u8]) -> Result<Self, DecodeError> {
                Ok(Self)
            }
            fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
                Ok(Vec::new())
            }
        }
        let mut builder = MsgBuilder::new(&header(), RAW_BUDGET);
        builder.body(&EndOfMessageBody);
        assert_eq!(builder.finish(), Err(BuildError::EndOfMessageNotAllowed));
    }
}
