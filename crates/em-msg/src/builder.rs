// SPDX-License-Identifier: Apache-2.0

//! Message builder (upstream `em_msg_builder_t` and `add_tlv`).
//!
//! The budget counts the whole CMDU: the header, every TLV (3 bytes plus the
//! value) and the final End of Message (3 bytes). The Ethernet header is not
//! part of it.

use em_types::TlvType;

use crate::bodies::TlvBody;
use crate::error::BuildError;
use crate::header::CmduHeader;
use crate::tlv::TLV_HEADER_LEN;

/// Budget for the raw socket: `EM_MAX_MSG_SZ` (1514) minus the 14 Ethernet bytes.
pub const RAW_BUDGET: usize = 1500;
/// Budget for AL-SAP: 65531 minus the 14 Ethernet bytes.
pub const AL_SAP_BUDGET: usize = 65517;
/// Largest TLV value length (the length field is 16 bits).
pub const MAX_TLV_VALUE: usize = 65535;

/// Builds one CMDU: header first, then TLVs, then End of Message.
///
/// The first error is kept. It can come from [`MsgBuilder::new`], [`MsgBuilder::tlv`] or
/// [`MsgBuilder::body`]. Later calls to `tlv` and `body` do nothing, and
/// [`MsgBuilder::finish`] returns that error.
#[derive(Debug)]
pub struct MsgBuilder {
    buf: Vec<u8>,
    budget: usize,
    error: Option<BuildError>,
}

impl MsgBuilder {
    /// Starts a message with `header`, limited to `budget` bytes in total.
    #[must_use]
    pub fn new(header: &CmduHeader, budget: usize) -> Self {
        let mut buf = Vec::new();
        header.write(&mut buf);
        let error = if buf.len() > budget {
            Some(BuildError::OverBudget)
        } else {
            None
        };
        Self { buf, budget, error }
    }

    /// Appends a TLV.
    ///
    /// Keeps [`BuildError::EndOfMessageNotAllowed`] for End of Message,
    /// [`BuildError::TlvTooLong`] for a value over 65535 bytes (checked before
    /// the budget) and [`BuildError::OverBudget`] if the TLV does not fit.
    pub fn tlv(&mut self, tlv_type: TlvType, value: &[u8]) -> &mut Self {
        if self.error.is_some() {
            return self;
        }
        if tlv_type == TlvType::END_OF_MESSAGE {
            self.error = Some(BuildError::EndOfMessageNotAllowed);
            return self;
        }
        let Ok(value_len) = u16::try_from(value.len()) else {
            self.error = Some(BuildError::TlvTooLong);
            return self;
        };
        if self.buf.len() + TLV_HEADER_LEN + value.len() > self.budget {
            self.error = Some(BuildError::OverBudget);
            return self;
        }
        self.buf.push(tlv_type.0);
        self.buf.extend_from_slice(&value_len.to_be_bytes());
        self.buf.extend_from_slice(value);
        self
    }

    /// Encodes `body` and appends it as a TLV of type `B::TLV_TYPE`.
    ///
    /// An encode failure is kept as [`BuildError::Encode`], like any other first
    /// error. A builder that already holds an error does not encode. The errors
    /// of [`MsgBuilder::tlv`] apply to the encoded value.
    pub fn body<'a, B: TlvBody<'a>>(&mut self, body: &B) -> &mut Self {
        if self.error.is_some() {
            return self;
        }
        match body.encode_value() {
            Ok(value) => self.tlv(B::TLV_TYPE, &value),
            Err(error) => {
                self.error = Some(BuildError::Encode(error));
                self
            }
        }
    }

    /// Appends End of Message and returns the finished message.
    ///
    /// # Errors
    ///
    /// The first error kept by [`MsgBuilder::new`], [`MsgBuilder::tlv`] or
    /// [`MsgBuilder::body`] (including [`BuildError::Encode`]), or
    /// [`BuildError::OverBudget`] if End of Message does not fit.
    pub fn finish(mut self) -> Result<Vec<u8>, BuildError> {
        if let Some(error) = self.error {
            return Err(error);
        }
        if self.buf.len() + TLV_HEADER_LEN > self.budget {
            return Err(BuildError::OverBudget);
        }
        self.buf.extend_from_slice(&[0, 0, 0]);
        Ok(self.buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::header::CmduHeader;
    use crate::tlv::{Tlv, TlvIter};
    use em_types::MsgType;

    /// Bound for collecting an iterator in tests, so a runaway iterator fails the test instead of exhausting memory.
    const MAX_ITEMS: usize = 64;

    fn header() -> CmduHeader {
        CmduHeader {
            msg_type: MsgType::TOPOLOGY_QUERY,
            msg_id: 0x0102,
            frag_id: 0,
            last_fragment: true,
            relay: false,
        }
    }

    #[test]
    fn budget_constants() {
        assert_eq!(RAW_BUDGET, 1500);
        assert_eq!(AL_SAP_BUDGET, 65517);
        assert_eq!(MAX_TLV_VALUE, 65535);
    }

    #[test]
    fn exact_bytes_for_a_small_message() {
        let mut builder = MsgBuilder::new(&header(), RAW_BUDGET);
        builder
            .tlv(TlvType(0x01), &[0xAA, 0xBB])
            .tlv(TlvType(0x02), &[]);
        let bytes = builder.finish().unwrap();
        let expected = [
            0x00, 0x00, 0x00, 0x02, 0x01, 0x02, 0x00, 0x80, // header
            0x01, 0x00, 0x02, 0xAA, 0xBB, // first TLV
            0x02, 0x00, 0x00, // second TLV, empty
            0x00, 0x00, 0x00, // End of Message
        ];
        assert_eq!(bytes, expected);
    }

    #[test]
    fn over_budget_at_a_tlv() {
        // Header (8) + TLV (3 + 2) = 13 does not fit in 12.
        let mut builder = MsgBuilder::new(&header(), 12);
        builder.tlv(TlvType(0x01), &[1, 2]);
        assert_eq!(builder.finish(), Err(BuildError::OverBudget));
    }

    #[test]
    fn over_budget_at_end_of_message() {
        // Header (8) + TLV (3 + 2) = 13 fills the budget; End of Message needs 3 more.
        let mut builder = MsgBuilder::new(&header(), 13);
        builder.tlv(TlvType(0x01), &[1, 2]);
        assert_eq!(builder.finish(), Err(BuildError::OverBudget));
    }

    #[test]
    fn tlv_that_exactly_fills_the_budget_does_not_hide_the_next_error() {
        // Header (8) + TLV (3 + 2) = 13 fills the budget exactly, so the TLV
        // is accepted (`>` not `>=`) and the End of Message call is the first error.
        let mut builder = MsgBuilder::new(&header(), 13);
        builder
            .tlv(TlvType(0x01), &[1, 2])
            .tlv(TlvType::END_OF_MESSAGE, &[]);
        assert_eq!(builder.finish(), Err(BuildError::EndOfMessageNotAllowed));
    }

    #[test]
    fn exactly_filling_the_budget_is_accepted() {
        let mut builder = MsgBuilder::new(&header(), 16);
        builder.tlv(TlvType(0x01), &[1, 2]);
        assert_eq!(builder.finish().unwrap().len(), 16);
    }

    #[test]
    fn over_budget_at_the_header() {
        let builder = MsgBuilder::new(&header(), 7);
        assert_eq!(builder.finish(), Err(BuildError::OverBudget));
    }

    #[test]
    fn header_only_message_with_budget_eleven() {
        let builder = MsgBuilder::new(&header(), 11);
        assert_eq!(builder.finish().unwrap().len(), 11);
    }

    #[test]
    fn first_error_is_sticky() {
        let mut builder = MsgBuilder::new(&header(), 12);
        builder
            .tlv(TlvType(0x01), &[1, 2]) // over budget, kept
            .tlv(TlvType::END_OF_MESSAGE, &[]) // would be another error, ignored
            .tlv(TlvType(0x02), &[]);
        assert_eq!(builder.finish(), Err(BuildError::OverBudget));
    }

    #[test]
    fn end_of_message_is_refused_by_tlv() {
        let mut builder = MsgBuilder::new(&header(), RAW_BUDGET);
        builder.tlv(TlvType::END_OF_MESSAGE, &[]);
        assert_eq!(builder.finish(), Err(BuildError::EndOfMessageNotAllowed));
    }

    #[test]
    fn value_of_65536_bytes_is_too_long() {
        let value = vec![0u8; MAX_TLV_VALUE + 1];
        let mut builder = MsgBuilder::new(&header(), 100_000);
        builder.tlv(TlvType(0x01), &value);
        assert_eq!(builder.finish(), Err(BuildError::TlvTooLong));
    }

    #[test]
    fn too_long_is_checked_before_the_budget() {
        let value = vec![0u8; MAX_TLV_VALUE + 1];
        let mut builder = MsgBuilder::new(&header(), 20);
        builder.tlv(TlvType(0x01), &value);
        assert_eq!(builder.finish(), Err(BuildError::TlvTooLong));
    }

    #[test]
    fn value_of_65535_bytes_is_accepted() {
        let value = vec![0x5Au8; MAX_TLV_VALUE];
        let mut builder = MsgBuilder::new(&header(), 70_000);
        builder.tlv(TlvType(0x01), &value);
        let bytes = builder.finish().unwrap();
        assert_eq!(bytes.len(), 8 + 3 + MAX_TLV_VALUE + 3);
        assert_eq!(bytes[8..11], [0x01, 0xFF, 0xFF]);
    }

    #[test]
    fn finished_message_parses_back() {
        let mut builder = MsgBuilder::new(&header(), RAW_BUDGET);
        builder
            .tlv(TlvType(0x01), &[7, 8, 9])
            .tlv(TlvType(0x20), &[]);
        let bytes = builder.finish().unwrap();
        let (parsed, tlv_area) = CmduHeader::parse(&bytes).unwrap();
        assert_eq!(parsed, header());
        let items: Vec<_> = TlvIter::new(tlv_area).take(MAX_ITEMS).collect();
        let expected = [
            Ok(Tlv {
                tlv_type: TlvType(0x01),
                value: &[7, 8, 9],
            }),
            Ok(Tlv {
                tlv_type: TlvType(0x20),
                value: &[],
            }),
        ];
        assert_eq!(items, expected);
    }
}
