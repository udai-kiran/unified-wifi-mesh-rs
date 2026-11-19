// SPDX-License-Identifier: Apache-2.0

//! Capacity-bounded strings and byte strings (0007 D4).
//!
//! Every type stores its bytes inline, is `Copy`, and takes its capacity as a const generic
//! `CAP`: the largest payload in bytes, without any terminator. Construction is the only way in,
//! so the invariants below hold for every value:
//!
//! | Type | Content | Embedded NUL |
//! |---|---|---|
//! | [`Text`] | UTF-8 text | rejected |
//! | [`CStrBytes`] | NUL-terminated C string payload: any bytes except NUL | rejected |
//! | [`LenBytes`] | length-delimited raw bytes: any bytes | accepted |
//!
//! Over-long input is an error, never truncated (upstream defects 1 and 2). The terminator of a
//! `char[N]` is not stored; callers state the payload capacity (`N - 1` for a NUL-terminated
//! array) in the type. The stored length is one byte, so `CAP` is at
//! most 255; a `LenBytes<255>` is then exactly 256 bytes, the `Copy` limit of the crate.

use std::fmt;

/// Why a string constructor refused its input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StringError {
    /// The input is longer than the capacity of the target type.
    TooLong {
        /// Length of the rejected input in bytes.
        len: usize,
        /// Capacity of the target type in bytes.
        capacity: usize,
    },
    /// The input contains a NUL byte, which the target type reserves as the C terminator.
    EmbeddedNul {
        /// Index of the first NUL byte.
        position: usize,
    },
    /// The input is not valid UTF-8 (text types only).
    NotUtf8,
}

impl fmt::Display for StringError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLong { len, capacity } => {
                write!(
                    f,
                    "input of {len} bytes exceeds the capacity of {capacity} bytes"
                )
            }
            Self::EmbeddedNul { position } => {
                write!(f, "input contains a NUL byte at index {position}")
            }
            Self::NotUtf8 => f.write_str("input is not valid UTF-8"),
        }
    }
}

impl std::error::Error for StringError {}

/// Inline storage shared by the three string types. The bytes after `len` are always zero.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Inline<const CAP: usize> {
    len: u8,
    buf: [u8; CAP],
}

impl<const CAP: usize> Inline<CAP> {
    const EMPTY: Self = {
        assert!(CAP <= u8::MAX as usize, "capacity must fit the u8 length");
        Self {
            len: 0,
            buf: [0; CAP],
        }
    };

    /// Checks the length and copies `bytes` in. The NUL and UTF-8 rules are the caller's to check.
    fn from_checked(bytes: &[u8]) -> Result<Self, StringError> {
        if bytes.len() > CAP {
            return Err(StringError::TooLong {
                len: bytes.len(),
                capacity: CAP,
            });
        }
        let mut inline = Self::EMPTY;
        inline.buf[..bytes.len()].copy_from_slice(bytes);
        // The length check above bounds `bytes.len()` by `CAP <= u8::MAX`.
        inline.len = u8::try_from(bytes.len()).unwrap_or(u8::MAX);
        Ok(inline)
    }

    fn as_bytes(&self) -> &[u8] {
        &self.buf[..usize::from(self.len)]
    }
}

fn reject_nul(bytes: &[u8]) -> Result<(), StringError> {
    match bytes.iter().position(|&b| b == 0) {
        Some(position) => Err(StringError::EmbeddedNul { position }),
        None => Ok(()),
    }
}

fn write_escaped(f: &mut fmt::Formatter<'_>, name: &str, bytes: &[u8]) -> fmt::Result {
    write!(f, "{name}(b\"{}\")", bytes.escape_ascii())
}

/// A NUL-terminated C string payload of at most `CAP` bytes: any bytes except NUL.
///
/// Use it for `char[N]` members that carry raw bytes (SSIDs, credentials, HAL strings), where
/// the content is not known to be UTF-8. `CAP` is `N - 1` for a plain `char[N]`.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct CStrBytes<const CAP: usize>(Inline<CAP>);

impl<const CAP: usize> CStrBytes<CAP> {
    /// Maximum payload length in bytes.
    pub const CAPACITY: usize = CAP;

    /// Creates a value from `bytes`.
    ///
    /// # Errors
    ///
    /// [`StringError::TooLong`] if `bytes` is longer than `CAP`;
    /// [`StringError::EmbeddedNul`] if it contains a NUL byte.
    pub fn new(bytes: &[u8]) -> Result<Self, StringError> {
        let inline = Inline::from_checked(bytes)?;
        reject_nul(bytes)?;
        Ok(Self(inline))
    }

    /// The payload bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }

    /// Payload length in bytes.
    #[must_use]
    pub fn len(&self) -> usize {
        usize::from(self.0.len)
    }

    /// True if the payload is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.len == 0
    }
}

impl<const CAP: usize> Default for CStrBytes<CAP> {
    fn default() -> Self {
        Self(Inline::EMPTY)
    }
}

impl<const CAP: usize> fmt::Debug for CStrBytes<CAP> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_escaped(f, "CStrBytes", self.as_bytes())
    }
}

/// Length-delimited raw bytes of at most `CAP` bytes: any byte value, including NUL.
///
/// Use it where the consumer stores an explicit `*_len` next to the buffer (the length folds into
/// this type).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct LenBytes<const CAP: usize>(Inline<CAP>);

impl<const CAP: usize> LenBytes<CAP> {
    /// Maximum length in bytes.
    pub const CAPACITY: usize = CAP;

    /// Creates a value from `bytes`.
    ///
    /// # Errors
    ///
    /// [`StringError::TooLong`] if `bytes` is longer than `CAP`.
    pub fn new(bytes: &[u8]) -> Result<Self, StringError> {
        Inline::from_checked(bytes).map(Self)
    }

    /// The stored bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }

    /// Length in bytes.
    #[must_use]
    pub fn len(&self) -> usize {
        usize::from(self.0.len)
    }

    /// True if no bytes are stored.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.len == 0
    }
}

impl<const CAP: usize> Default for LenBytes<CAP> {
    fn default() -> Self {
        Self(Inline::EMPTY)
    }
}

impl<const CAP: usize> fmt::Debug for LenBytes<CAP> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_escaped(f, "LenBytes", self.as_bytes())
    }
}

/// UTF-8 text of at most `CAP` bytes, without NUL.
///
/// Use it only where upstream treats the member as text; the mapping table of the golden test
/// records the reason for every such field.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Text<const CAP: usize>(Inline<CAP>);

impl<const CAP: usize> Text<CAP> {
    /// Maximum length in bytes.
    pub const CAPACITY: usize = CAP;

    /// Creates a value from a string.
    ///
    /// # Errors
    ///
    /// [`StringError::TooLong`] if `text` is longer than `CAP` bytes;
    /// [`StringError::EmbeddedNul`] if it contains NUL.
    pub fn new(text: &str) -> Result<Self, StringError> {
        let inline = Inline::from_checked(text.as_bytes())?;
        reject_nul(text.as_bytes())?;
        Ok(Self(inline))
    }

    /// Creates a value from bytes that must be valid UTF-8.
    ///
    /// # Errors
    ///
    /// The errors of [`Text::new`], plus [`StringError::NotUtf8`].
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, StringError> {
        let text = std::str::from_utf8(bytes).map_err(|_| StringError::NotUtf8)?;
        Self::new(text)
    }

    /// The text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        // Construction validated UTF-8, so the fallback is never taken.
        std::str::from_utf8(self.0.as_bytes()).unwrap_or_default()
    }

    /// The text as bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }

    /// Length in bytes.
    #[must_use]
    pub fn len(&self) -> usize {
        usize::from(self.0.len)
    }

    /// True if the text is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.len == 0
    }
}

impl<const CAP: usize> Default for Text<CAP> {
    fn default() -> Self {
        Self(Inline::EMPTY)
    }
}

impl<const CAP: usize> fmt::Debug for Text<CAP> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Text({:?})", self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::{CStrBytes, LenBytes, StringError, Text};

    #[test]
    fn cstr_bytes_limits() {
        assert_eq!(CStrBytes::<4>::new(b"abcd").map(|s| s.len()), Ok(4));
        assert_eq!(
            CStrBytes::<4>::new(b"abcde"),
            Err(StringError::TooLong {
                len: 5,
                capacity: 4
            })
        );
        assert_eq!(CStrBytes::<4>::new(b"").map(|s| s.is_empty()), Ok(true));
    }

    #[test]
    fn cstr_bytes_rejects_nul_and_keeps_non_utf8() {
        assert_eq!(
            CStrBytes::<4>::new(b"a\0b"),
            Err(StringError::EmbeddedNul { position: 1 })
        );
        assert_eq!(
            CStrBytes::<4>::new(b"\0"),
            Err(StringError::EmbeddedNul { position: 0 })
        );
        let raw = [0xff, 0xfe, 0x80];
        let value = CStrBytes::<4>::new(&raw).unwrap();
        assert_eq!(value.as_bytes(), &raw);
    }

    #[test]
    fn cstr_bytes_length_check_wins_over_nul_check() {
        // A too-long input is reported as too long even if it also holds a NUL.
        assert_eq!(
            CStrBytes::<2>::new(b"a\0bc"),
            Err(StringError::TooLong {
                len: 4,
                capacity: 2
            })
        );
    }

    #[test]
    fn len_bytes_accepts_nul_and_enforces_limit() {
        let value = LenBytes::<4>::new(&[0, 1, 0, 2]).unwrap();
        assert_eq!(value.as_bytes(), &[0, 1, 0, 2]);
        assert_eq!(value.len(), 4);
        assert_eq!(
            LenBytes::<4>::new(&[0; 5]),
            Err(StringError::TooLong {
                len: 5,
                capacity: 4
            })
        );
    }

    #[test]
    fn text_limits_nul_and_utf8() {
        assert_eq!(Text::<4>::new("abcd").map(|t| t.len()), Ok(4));
        assert_eq!(
            Text::<4>::new("abcde"),
            Err(StringError::TooLong {
                len: 5,
                capacity: 4
            })
        );
        // Capacity counts bytes, not characters: two 2-byte characters fill four bytes.
        assert_eq!(Text::<4>::new("\u{e9}\u{e9}").map(|t| t.len()), Ok(4));
        assert_eq!(
            Text::<3>::new("\u{e9}\u{e9}"),
            Err(StringError::TooLong {
                len: 4,
                capacity: 3
            })
        );
        assert_eq!(
            Text::<4>::new("a\0"),
            Err(StringError::EmbeddedNul { position: 1 })
        );
        assert_eq!(Text::<4>::from_bytes(&[0xff]), Err(StringError::NotUtf8));
        assert_eq!(Text::<4>::from_bytes(b"ok").unwrap().as_str(), "ok");
    }

    #[test]
    fn defaults_are_empty_and_equal_by_content() {
        assert!(CStrBytes::<8>::default().is_empty());
        assert!(LenBytes::<8>::default().is_empty());
        assert!(Text::<8>::default().is_empty());
        assert_eq!(
            CStrBytes::<8>::new(b"ab").unwrap(),
            CStrBytes::<8>::new(b"ab").unwrap()
        );
        assert_ne!(
            CStrBytes::<8>::new(b"ab").unwrap(),
            CStrBytes::<8>::new(b"ac").unwrap()
        );
    }

    #[test]
    fn largest_capacity_fits_the_length_field() {
        let full = [b'x'; 255];
        assert_eq!(LenBytes::<255>::new(&full).map(|v| v.len()), Ok(255));
        // One length byte plus 255 payload bytes is exactly the 256-byte `Copy` limit.
        assert_eq!(size_of::<LenBytes<255>>(), 256);
    }
}
