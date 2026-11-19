// SPDX-License-Identifier: Apache-2.0

//! Reader and writer for TLV bodies (design note 0008, D1).
//!
//! [`Reader`] borrows the value of one TLV and reads fields in order;
//! [`Writer`] owns the bytes of a value being built. A read past the end and
//! a write the field cannot hold return an error value; neither truncates
//! nor writes a partial field (see [`Writer::zeros`] for its one panic).
//!
//! # Entry points of a body
//!
//! Every body follows the same pattern, so the callers look alike:
//!
//! ```text
//! impl<'a> Body<'a> {
//!     pub fn decode_value(value: &'a [u8]) -> Result<Self, DecodeError> {
//!         let mut reader = Reader::new(value);
//!         let body = Self::decode(&mut reader)?;
//!         reader.finish()?;
//!         Ok(body)
//!     }
//!
//!     pub fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
//!         let mut writer = Writer::new();
//!         self.encode(&mut writer)?;
//!         Ok(writer.into_bytes())
//!     }
//! }
//! ```
//!
//! `decode_value` takes `Tlv::value`. `encode_value` returns the bytes only if
//! the whole body was encoded, so a failed encode publishes nothing; the
//! caller passes the bytes to `MsgBuilder::tlv`. A body with a tail that runs
//! to the end of the TLV calls [`Reader::rest`] and `finish` then succeeds.

use em_types::MacAddr;

use crate::error::{DecodeError, EncodeError};

/// Largest value of a 24-bit field.
const MAX_U24: u32 = 0x00FF_FFFF;

/// Reads the fields of one TLV value, front to back.
///
/// A read that fails leaves the reader where it was.
#[derive(Debug)]
pub struct Reader<'a> {
    input: &'a [u8],
}

impl<'a> Reader<'a> {
    /// Starts reading at the first byte of `input`.
    #[must_use]
    pub fn new(input: &'a [u8]) -> Self {
        Self { input }
    }

    /// Number of bytes not read yet.
    #[must_use]
    pub fn remaining(&self) -> usize {
        self.input.len()
    }

    /// Takes the next `n` bytes. A zero-byte read always succeeds.
    fn take(&mut self, n: usize) -> Result<&'a [u8], DecodeError> {
        let Some((taken, left)) = self.input.split_at_checked(n) else {
            return Err(DecodeError::Truncated {
                needed: n,
                remaining: self.input.len(),
            });
        };
        self.input = left;
        Ok(taken)
    }

    /// Reads one byte.
    ///
    /// # Errors
    ///
    /// [`DecodeError::Truncated`] if no byte is left.
    pub fn u8(&mut self) -> Result<u8, DecodeError> {
        let [value] = self.array()?;
        Ok(value)
    }

    /// Reads a big-endian 16-bit value.
    ///
    /// # Errors
    ///
    /// [`DecodeError::Truncated`] if fewer than 2 bytes are left.
    pub fn be_u16(&mut self) -> Result<u16, DecodeError> {
        Ok(u16::from_be_bytes(self.array()?))
    }

    /// Reads a big-endian 24-bit value (3 bytes, most significant first).
    ///
    /// # Errors
    ///
    /// [`DecodeError::Truncated`] if fewer than 3 bytes are left.
    pub fn be_u24(&mut self) -> Result<u32, DecodeError> {
        let [high, middle, low] = self.array()?;
        Ok(u32::from_be_bytes([0, high, middle, low]))
    }

    /// Reads a big-endian 32-bit value.
    ///
    /// # Errors
    ///
    /// [`DecodeError::Truncated`] if fewer than 4 bytes are left.
    pub fn be_u32(&mut self) -> Result<u32, DecodeError> {
        Ok(u32::from_be_bytes(self.array()?))
    }

    /// Reads a little-endian 16-bit value, for fields upstream writes in host order (note 0008, D6).
    ///
    /// # Errors
    ///
    /// [`DecodeError::Truncated`] if fewer than 2 bytes are left.
    pub fn le_u16(&mut self) -> Result<u16, DecodeError> {
        Ok(u16::from_le_bytes(self.array()?))
    }

    /// Reads a little-endian 32-bit value, for fields upstream writes in host order (note 0008, D6).
    ///
    /// # Errors
    ///
    /// [`DecodeError::Truncated`] if fewer than 4 bytes are left.
    pub fn le_u32(&mut self) -> Result<u32, DecodeError> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    /// Reads `n` bytes, borrowed from the input.
    ///
    /// # Errors
    ///
    /// [`DecodeError::Truncated`] if fewer than `n` bytes are left.
    pub fn bytes(&mut self, n: usize) -> Result<&'a [u8], DecodeError> {
        self.take(n)
    }

    /// Reads `N` bytes into an array.
    ///
    /// # Errors
    ///
    /// [`DecodeError::Truncated`] if fewer than `N` bytes are left.
    pub fn array<const N: usize>(&mut self) -> Result<[u8; N], DecodeError> {
        let taken = self.take(N)?;
        let mut array = [0; N];
        array.copy_from_slice(taken);
        Ok(array)
    }

    /// Reads a 6-byte MAC address.
    ///
    /// # Errors
    ///
    /// [`DecodeError::Truncated`] if fewer than 6 bytes are left.
    pub fn mac(&mut self) -> Result<MacAddr, DecodeError> {
        Ok(MacAddr::new(self.array()?))
    }

    /// Skips `n` bytes, for reserved fields.
    ///
    /// # Errors
    ///
    /// [`DecodeError::Truncated`] if fewer than `n` bytes are left.
    pub fn skip(&mut self, n: usize) -> Result<(), DecodeError> {
        self.take(n)?;
        Ok(())
    }

    /// Takes everything that is left, for a tail bounded by the TLV length.
    /// The reader is empty afterwards.
    pub fn rest(&mut self) -> &'a [u8] {
        let everything = self.input;
        self.input = &[];
        everything
    }

    /// Ends the read.
    ///
    /// # Errors
    ///
    /// [`DecodeError::TrailingBytes`] if input is left.
    pub fn finish(self) -> Result<(), DecodeError> {
        if self.input.is_empty() {
            Ok(())
        } else {
            Err(DecodeError::TrailingBytes {
                remaining: self.input.len(),
            })
        }
    }
}

/// Builds one TLV value.
///
/// A call that fails appends nothing.
#[derive(Debug, Default)]
pub struct Writer {
    buf: Vec<u8>,
}

impl Writer {
    /// Starts an empty value.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of bytes written so far.
    #[must_use]
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    /// `true` if nothing has been written.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// Returns the bytes written.
    #[must_use]
    pub fn into_bytes(self) -> Vec<u8> {
        self.buf
    }

    /// Writes one byte.
    pub fn u8(&mut self, value: u8) {
        self.buf.push(value);
    }

    /// Writes a big-endian 16-bit value.
    pub fn be_u16(&mut self, value: u16) {
        self.buf.extend_from_slice(&value.to_be_bytes());
    }

    /// Writes a big-endian 24-bit value (3 bytes, most significant first).
    ///
    /// # Errors
    ///
    /// [`EncodeError::ValueTooLarge`] if `value` is above `0xFF_FFFF`; the
    /// value is never cut to fit.
    pub fn be_u24(&mut self, value: u32) -> Result<(), EncodeError> {
        if value > MAX_U24 {
            return Err(EncodeError::ValueTooLarge {
                value,
                max: MAX_U24,
            });
        }
        let [_, high, middle, low] = value.to_be_bytes();
        self.buf.extend_from_slice(&[high, middle, low]);
        Ok(())
    }

    /// Writes a big-endian 32-bit value.
    pub fn be_u32(&mut self, value: u32) {
        self.buf.extend_from_slice(&value.to_be_bytes());
    }

    /// Writes a little-endian 16-bit value, for fields upstream writes in host order (note 0008, D6).
    pub fn le_u16(&mut self, value: u16) {
        self.buf.extend_from_slice(&value.to_le_bytes());
    }

    /// Writes a little-endian 32-bit value, for fields upstream writes in host order (note 0008, D6).
    pub fn le_u32(&mut self, value: u32) {
        self.buf.extend_from_slice(&value.to_le_bytes());
    }

    /// Writes raw bytes.
    pub fn bytes(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    /// Writes a 6-byte MAC address.
    pub fn mac(&mut self, mac: MacAddr) {
        self.buf.extend_from_slice(&mac.octets());
    }

    /// Writes `n` zero bytes, for reserved fields.
    ///
    /// Bytes already written are never cut. An `n` above
    /// `isize::MAX - self.len()` (for example a padding length that wrapped
    /// below zero in the caller) panics with "capacity overflow" in every
    /// build profile; a smaller `n` that memory cannot hold aborts like any
    /// failed allocation.
    pub fn zeros(&mut self, n: usize) {
        self.buf.extend(core::iter::repeat_n(0, n));
    }

    /// Writes the 1-byte count of a collection of `len` items.
    ///
    /// # Errors
    ///
    /// [`EncodeError::CountTooLarge`] if `len` is above 255.
    pub fn count_u8(&mut self, len: usize) -> Result<(), EncodeError> {
        let Ok(count) = u8::try_from(len) else {
            return Err(EncodeError::CountTooLarge {
                len,
                max: usize::from(u8::MAX),
            });
        };
        self.u8(count);
        Ok(())
    }

    /// Writes the big-endian 2-byte count of a collection of `len` items.
    ///
    /// # Errors
    ///
    /// [`EncodeError::CountTooLarge`] if `len` is above 65535.
    pub fn count_u16(&mut self, len: usize) -> Result<(), EncodeError> {
        let Ok(count) = u16::try_from(len) else {
            return Err(EncodeError::CountTooLarge {
                len,
                max: usize::from(u16::MAX),
            });
        };
        self.be_u16(count);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_every_width_in_its_byte_order() {
        let input = [
            0x01, // u8
            0x01, 0x02, // be_u16
            0x01, 0x02, 0x03, // be_u24
            0x01, 0x02, 0x03, 0x04, // be_u32
            0x01, 0x02, // le_u16
            0x01, 0x02, 0x03, 0x04, // le_u32
        ];
        let mut reader = Reader::new(&input);
        assert_eq!(reader.u8(), Ok(0x01));
        assert_eq!(reader.be_u16(), Ok(0x0102));
        assert_eq!(reader.be_u24(), Ok(0x01_0203));
        assert_eq!(reader.be_u32(), Ok(0x0102_0304));
        assert_eq!(reader.le_u16(), Ok(0x0201));
        assert_eq!(reader.le_u32(), Ok(0x0403_0201));
        assert_eq!(reader.finish(), Ok(()));
    }

    #[test]
    fn reads_bytes_array_mac_and_skip() {
        let input = [
            9, 8, 7, 1, 2, 3, 4, 5, 6, 0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF, 0x42,
        ];
        let mut reader = Reader::new(&input);
        assert_eq!(reader.bytes(2), Ok(&[9, 8][..]));
        assert_eq!(reader.array::<1>(), Ok([7]));
        assert_eq!(reader.mac(), Ok(MacAddr::new([1, 2, 3, 4, 5, 6])));
        assert_eq!(reader.skip(2), Ok(()));
        assert_eq!(reader.array::<4>(), Ok([0xCC, 0xDD, 0xEE, 0xFF]));
        assert_eq!(reader.remaining(), 1);
    }

    #[test]
    fn read_past_the_end_is_truncated_with_context() {
        let mut reader = Reader::new(&[1, 2, 3]);
        assert_eq!(
            reader.be_u32(),
            Err(DecodeError::Truncated {
                needed: 4,
                remaining: 3
            })
        );
        assert_eq!(
            reader.bytes(4),
            Err(DecodeError::Truncated {
                needed: 4,
                remaining: 3
            })
        );
        assert_eq!(
            reader.skip(4),
            Err(DecodeError::Truncated {
                needed: 4,
                remaining: 3
            })
        );
    }

    #[test]
    fn one_byte_read_at_the_end_is_truncated() {
        let mut reader = Reader::new(&[]);
        assert_eq!(
            reader.u8(),
            Err(DecodeError::Truncated {
                needed: 1,
                remaining: 0
            })
        );
        let mut reader = Reader::new(&[1, 2, 3, 4, 5]);
        assert_eq!(reader.skip(5), Ok(()));
        assert_eq!(
            reader.bytes(1),
            Err(DecodeError::Truncated {
                needed: 1,
                remaining: 0
            })
        );
    }

    #[test]
    fn zero_byte_read_at_the_end_succeeds() {
        let mut reader = Reader::new(&[]);
        assert_eq!(reader.bytes(0), Ok(&[][..]));
        assert_eq!(reader.skip(0), Ok(()));
        assert_eq!(reader.array::<0>(), Ok([]));
        assert_eq!(reader.finish(), Ok(()));
    }

    #[test]
    fn failed_read_leaves_the_reader_unchanged() {
        let mut reader = Reader::new(&[1, 2, 3]);
        assert_eq!(reader.u8(), Ok(1));
        assert_eq!(reader.remaining(), 2);
        assert!(reader.bytes(3).is_err());
        assert_eq!(reader.remaining(), 2);
        assert!(reader.be_u32().is_err());
        assert_eq!(reader.remaining(), 2);
        assert_eq!(reader.bytes(1), Ok(&[2][..]));
        assert_eq!(
            reader.be_u16(),
            Err(DecodeError::Truncated {
                needed: 2,
                remaining: 1
            })
        );
        assert_eq!(reader.remaining(), 1);
        assert_eq!(reader.u8(), Ok(3));
    }

    #[test]
    fn rest_takes_everything_and_empties_the_reader() {
        let mut reader = Reader::new(&[1, 2, 3, 4]);
        assert_eq!(reader.u8(), Ok(1));
        assert_eq!(reader.rest(), &[2, 3, 4]);
        assert_eq!(reader.remaining(), 0);
        assert_eq!(reader.rest(), &[] as &[u8]);
        assert_eq!(reader.finish(), Ok(()));
    }

    #[test]
    fn finish_with_leftover_bytes_is_trailing_bytes() {
        let mut reader = Reader::new(&[1, 2, 3]);
        assert_eq!(reader.u8(), Ok(1));
        assert_eq!(
            reader.finish(),
            Err(DecodeError::TrailingBytes { remaining: 2 })
        );
    }

    #[test]
    fn writes_every_width_in_its_byte_order() {
        let mut writer = Writer::new();
        assert!(writer.is_empty());
        writer.u8(0x01);
        writer.be_u16(0x0102);
        writer.be_u24(0x01_0203).unwrap();
        writer.be_u32(0x0102_0304);
        writer.le_u16(0x0102);
        writer.le_u32(0x0102_0304);
        assert_eq!(writer.len(), 16);
        assert_eq!(
            writer.into_bytes(),
            [
                0x01, // u8
                0x01, 0x02, // be_u16
                0x01, 0x02, 0x03, // be_u24
                0x01, 0x02, 0x03, 0x04, // be_u32
                0x02, 0x01, // le_u16
                0x04, 0x03, 0x02, 0x01, // le_u32
            ]
        );
    }

    #[test]
    fn writes_bytes_mac_and_zeros() {
        let mut writer = Writer::new();
        writer.bytes(&[7, 8]);
        writer.mac(MacAddr::new([1, 2, 3, 4, 5, 6]));
        writer.zeros(3);
        writer.zeros(0);
        assert_eq!(writer.into_bytes(), [7, 8, 1, 2, 3, 4, 5, 6, 0, 0, 0]);
    }

    #[test]
    fn be_u24_accepts_the_largest_value_and_rejects_the_next() {
        let mut writer = Writer::new();
        writer.be_u24(0xFF_FFFF).unwrap();
        assert_eq!(writer.into_bytes(), [0xFF, 0xFF, 0xFF]);

        let mut writer = Writer::new();
        assert_eq!(
            writer.be_u24(0x0100_0000),
            Err(EncodeError::ValueTooLarge {
                value: 0x0100_0000,
                max: 0xFF_FFFF
            })
        );
        assert!(writer.is_empty());
    }

    #[test]
    fn count_u8_accepts_255_and_rejects_256() {
        let mut writer = Writer::new();
        assert_eq!(writer.count_u8(255), Ok(()));
        assert_eq!(writer.len(), 1);
        assert_eq!(
            writer.count_u8(256),
            Err(EncodeError::CountTooLarge { len: 256, max: 255 })
        );
        assert_eq!(writer.into_bytes(), [0xFF]);
    }

    #[test]
    fn count_u16_is_big_endian_accepts_65535_and_rejects_65536() {
        let mut writer = Writer::new();
        assert_eq!(writer.count_u16(0x0102), Ok(()));
        assert_eq!(writer.count_u16(65535), Ok(()));
        assert_eq!(writer.len(), 4);
        assert_eq!(
            writer.count_u16(65536),
            Err(EncodeError::CountTooLarge {
                len: 65536,
                max: 65535
            })
        );
        assert_eq!(writer.into_bytes(), [0x01, 0x02, 0xFF, 0xFF]);
    }

    #[test]
    fn failed_writer_calls_append_nothing() {
        let mut writer = Writer::new();
        writer.u8(0xAA);
        assert!(writer.count_u8(256).is_err());
        assert_eq!(writer.len(), 1);
        assert!(writer.count_u16(65536).is_err());
        assert_eq!(writer.len(), 1);
        assert!(writer.be_u24(0x0100_0000).is_err());
        assert_eq!(writer.len(), 1);
        assert_eq!(writer.into_bytes(), [0xAA]);
    }

    #[test]
    fn zeros_appends_after_existing_bytes() {
        let mut writer = Writer::new();
        writer.u8(0xAA);
        writer.zeros(3);
        writer.zeros(0);
        assert_eq!(writer.into_bytes(), [0xAA, 0, 0, 0]);
    }

    #[test]
    #[should_panic(expected = "capacity overflow")]
    fn zeros_with_a_wrapped_length_panics_instead_of_cutting() {
        let mut writer = Writer::new();
        writer.bytes(&[0x41; 34]);
        // A caller padding to 32 bytes with a 34-byte value: 32 - 34 wraps.
        writer.zeros(32_usize.wrapping_sub(34));
    }

    #[test]
    fn errors_have_readable_messages() {
        let truncated = DecodeError::Truncated {
            needed: 4,
            remaining: 1,
        };
        assert_eq!(
            truncated.to_string(),
            "the TLV value is truncated: 4 bytes needed, 1 left"
        );
        let trailing = DecodeError::TrailingBytes { remaining: 2 };
        assert_eq!(
            trailing.to_string(),
            "the TLV value has 2 unexpected trailing bytes"
        );
        let count = EncodeError::CountTooLarge { len: 256, max: 255 };
        assert!(count.to_string().contains("256"));
        let value = EncodeError::ValueTooLarge {
            value: 0x0100_0000,
            max: 0xFF_FFFF,
        };
        assert!(value.to_string().contains("16777216"));
    }
}
