// SPDX-License-Identifier: Apache-2.0

//! CMDU header, TLV framing, message builder and validation.
//!
//! This crate contains the pieces listed in design note
//! [0005-em-msg-scope](https://github.com/udai-kiran/unified-wifi-mesh-rs/blob/main/docs/0005-em-msg-scope.md), section D1, Step 2.
//! `CmduHeader::parse` and `validate` take the whole CMDU (from the 8-byte
//! header); `TlvIter::new` and `wsc_msg_type` take the TLV area after the
//! header; `MsgBuilder` produces the whole CMDU. The Ethernet header belongs
//! to `em-transport`.
//!
//! | Module | Holds | Replaces (upstream) |
//! |---|---|---|
//! | [`error`] | [`FramingError`], [`BuildError`] | log messages and `false` returns |
//! | [`header`] | [`CmduHeader`], [`default_relay`] | `em_cmdu_t`, `add_1905_header` |
//! | [`tlv`] | [`Tlv`], [`TlvIter`] | `em_tlv_t`, `get_tlv`, `get_first_tlv`, `get_next_tlv` |
//! | [`builder`] | [`MsgBuilder`] and the size budgets | `em_msg_builder_t`, `add_tlv` |
//! | [`wsc`] | [`wsc_msg_type`] | `em_configuration_t::get_wsc_msg_type` |
//! | [`tables`] | [`TABLES`], the 70 validation tables | the table functions of `em_msg_t` |
//! | [`mod@validate`] | [`validate()`] | `em_msg_t::validate` |
//!
//! Depends only on `em-types`, no `unsafe`.
//!
//! Base: `rdkcentral/unified-wifi-mesh` @ `ba4180b`.

#![forbid(unsafe_code)]

pub mod builder;
pub mod error;
pub mod header;
pub mod tables;
pub mod tlv;
pub mod validate;
pub mod wsc;

#[cfg(test)]
mod upstream_vectors;

pub use builder::{AL_SAP_BUDGET, MAX_TLV_VALUE, MsgBuilder, RAW_BUDGET};
pub use error::{BuildError, FramingError};
pub use header::{CmduHeader, default_relay};
pub use tables::{Requirement, Row, Rule, TABLES, Table, WscKind};
pub use tlv::{Tlv, TlvIter};
pub use validate::{ValidateError, Violation, ViolationKind, validate};
pub use wsc::{WSC_M1, WSC_M2, wsc_msg_type};
