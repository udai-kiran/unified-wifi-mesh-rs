// SPDX-License-Identifier: Apache-2.0

//! Shared basic types for the unified-wifi-mesh Rust port.
//!
//! This crate contains exactly the types listed in design note
//! [0001-em-types-scope](https://github.com/udai-kiran/unified-wifi-mesh-rs/blob/main/docs/0001-em-types-scope.md), section D2:
//!
//! | Rust type | Replaces (upstream) |
//! |---|---|
//! | [`MacAddr`] | `mac_address_t`, `mac_addr_str_t`, MAC helpers |
//! | [`InterfaceName`] | `em_interface_name_t` |
//! | [`Interface`] | `em_interface_t` |
//! | [`MsgType`] | `em_msg_type_t` |
//! | [`TlvType`] | `em_tlv_type_t` |
//! | [`FreqBand`] | `em_freq_band_t` |
//! | [`MediaType`] | `em_media_type_t` |
//! | [`ProfileType`] | `em_profile_type_t` |
//! | [`ServiceType`] | `em_service_type_t` |
//!
//! No dependencies, no `unsafe`. Wire encoding lives in `em-msg`, not here.
//!
//! Base: `rdkcentral/unified-wifi-mesh` @ `ba4180b`.

#![forbid(unsafe_code)]

pub mod codes;
pub mod interface;
pub mod mac;

pub use codes::{FreqBand, MediaType, MsgType, ProfileType, ServiceType, TlvType};
pub use interface::{Interface, InterfaceName, InterfaceNameError};
pub use mac::{MacAddr, MacAddrParseError};
