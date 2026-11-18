// SPDX-License-Identifier: Apache-2.0

//! Network interface types: [`InterfaceName`] and [`Interface`].

use std::fmt;
use std::str::FromStr;

use crate::codes::MediaType;
use crate::mac::MacAddr;

// ---------------------------------------------------------------------------
// InterfaceName
// ---------------------------------------------------------------------------

/// A validated Linux network interface name.
///
/// The invariant follows Linux's `dev_valid_name` rule:
/// 1 to [`InterfaceName::MAX_LEN`] bytes, valid UTF-8, no `'\0'`, `'/'`, `':'`
/// or whitespace, and not `"."` or `".."`.
///
/// This crate rejects names that are not valid UTF-8 (see [`InterfaceNameError::NotUtf8`]).
/// Any place that loads names from external data must report such a name as an
/// error with context, and must not store it.
#[derive(PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct InterfaceName(String);

impl InterfaceName {
    /// Maximum byte length of a valid [`InterfaceName`] (Linux `IFNAMSIZ` − 1).
    pub const MAX_LEN: usize = 15;

    /// Returns the name as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Parses an interface name from a byte slice.
    ///
    /// # Errors
    ///
    /// Returns [`InterfaceNameError::NotUtf8`] if `bytes` is not valid UTF-8.
    /// Other validation errors (empty, too long, invalid character, reserved)
    /// may also be returned once the bytes are decoded.
    ///
    /// The error does not carry the interface's MAC address. A caller that
    /// loads names from the kernel or from persisted data must attach the MAC
    /// address (and may use the hex rendering of [`InterfaceNameError::NotUtf8`])
    /// when reporting the error (design note 0001, D5).
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, InterfaceNameError> {
        let s = std::str::from_utf8(bytes).map_err(|_| InterfaceNameError::NotUtf8 {
            bytes: bytes.to_vec(),
        })?;
        Self::try_from(s)
    }
}

fn validate(s: &str) -> Result<(), InterfaceNameError> {
    if s.is_empty() {
        return Err(InterfaceNameError::Empty);
    }
    // `str::len()` returns byte count.
    if s.len() > InterfaceName::MAX_LEN {
        return Err(InterfaceNameError::TooLong { len: s.len() });
    }
    for ch in s.chars() {
        if ch == '\0' || ch == '/' || ch == ':' || ch.is_whitespace() {
            return Err(InterfaceNameError::InvalidChar { ch });
        }
    }
    if s == "." || s == ".." {
        return Err(InterfaceNameError::Reserved);
    }
    Ok(())
}

// --- Conversions -----------------------------------------------------------

impl TryFrom<&str> for InterfaceName {
    type Error = InterfaceNameError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        validate(s)?;
        Ok(Self(s.to_owned()))
    }
}

impl TryFrom<String> for InterfaceName {
    type Error = InterfaceNameError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        validate(&s)?;
        Ok(Self(s))
    }
}

impl FromStr for InterfaceName {
    type Err = InterfaceNameError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::try_from(s)
    }
}

impl AsRef<str> for InterfaceName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

// --- Formatting ------------------------------------------------------------

impl fmt::Display for InterfaceName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for InterfaceName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "InterfaceName({:?})", self.0)
    }
}

// ---------------------------------------------------------------------------
// Interface
// ---------------------------------------------------------------------------

/// A network interface descriptor: an optional name, a MAC address, and a media type.
///
/// `name` is `None` when the interface name is not known.
#[derive(Debug, PartialEq, Eq, Hash)]
pub struct Interface {
    /// Interface name, or `None` when not yet known.
    pub name: Option<InterfaceName>,
    /// MAC address of the interface.
    pub mac: MacAddr,
    /// Media type of the interface.
    pub media: MediaType,
}

// ---------------------------------------------------------------------------
// InterfaceNameError
// ---------------------------------------------------------------------------

/// Error returned when an [`InterfaceName`] cannot be constructed.
#[non_exhaustive]
#[derive(Debug, PartialEq, Eq)]
pub enum InterfaceNameError {
    /// The name is empty (zero bytes).
    Empty,
    /// The name exceeds [`InterfaceName::MAX_LEN`] bytes.
    TooLong {
        /// The actual byte length of the rejected name.
        len: usize,
    },
    /// The name contains a forbidden character: `'\0'`, `'/'`, `':'`, or any
    /// character for which [`char::is_whitespace`] is true. This includes
    /// Unicode whitespace such as U+00A0, which is stricter than the kernel's
    /// byte-oriented `dev_valid_name` check (design note 0001, D5).
    InvalidChar {
        /// The offending character.
        ch: char,
    },
    /// The name is `"."` or `".."`, which the kernel rejects.
    Reserved,
    /// The byte slice is not valid UTF-8.
    NotUtf8 {
        /// The raw bytes of the rejected name.
        bytes: Vec<u8>,
    },
}

impl fmt::Display for InterfaceNameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "interface name must not be empty"),
            Self::TooLong { len } => write!(
                f,
                "interface name is {len} bytes, maximum is {}",
                InterfaceName::MAX_LEN
            ),
            Self::InvalidChar { ch } => {
                if *ch == '\0' {
                    write!(f, "interface name contains a NUL character")
                } else {
                    write!(f, "interface name contains invalid character {ch:?}")
                }
            }
            Self::Reserved => {
                write!(f, "interface name is reserved (\".\" or \"..\")")
            }
            Self::NotUtf8 { bytes } => {
                write!(f, "interface name is not valid UTF-8:")?;
                for b in bytes {
                    write!(f, " {b:02x}")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for InterfaceNameError {}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_rejected() {
        assert_eq!(InterfaceName::try_from(""), Err(InterfaceNameError::Empty));
    }

    #[test]
    fn single_char_accepted() {
        let name = InterfaceName::try_from("e").unwrap();
        assert_eq!(name.as_str(), "e");
    }

    #[test]
    fn max_len_accepted() {
        let s = "a".repeat(15);
        let name = InterfaceName::try_from(s.as_str()).unwrap();
        assert_eq!(name.as_str(), s.as_str());
    }

    #[test]
    fn too_long_rejected() {
        let s = "a".repeat(16);
        assert_eq!(
            InterfaceName::try_from(s.as_str()),
            Err(InterfaceNameError::TooLong { len: 16 })
        );
    }

    #[test]
    fn multibyte_utf8_15_bytes_accepted() {
        // "ñ" is U+00F1, 2 bytes in UTF-8.
        // 7 × 'a' + 1 × 'ñ' + 6 × 'b' = 7 + 2 + 6 = 15 bytes, 14 chars.
        let s = "aaaaaaañbbbbbb";
        assert_eq!(s.len(), 15, "test string must be exactly 15 bytes");
        assert!(
            s.chars().count() < 15,
            "test string must have fewer than 15 chars"
        );
        assert!(InterfaceName::try_from(s).is_ok());
    }

    #[test]
    fn multibyte_utf8_16_bytes_rejected() {
        // 7 × 'a' + 1 × 'ñ' + 7 × 'b' = 7 + 2 + 7 = 16 bytes, 15 chars.
        let s = "aaaaaaañbbbbbbb";
        assert_eq!(s.len(), 16, "test string must be exactly 16 bytes");
        assert_eq!(
            InterfaceName::try_from(s),
            Err(InterfaceNameError::TooLong { len: 16 })
        );
    }

    #[test]
    fn invalid_char_nul() {
        assert_eq!(
            InterfaceName::try_from("eth\0"),
            Err(InterfaceNameError::InvalidChar { ch: '\0' })
        );
    }

    #[test]
    fn invalid_char_slash() {
        assert_eq!(
            InterfaceName::try_from("eth/0"),
            Err(InterfaceNameError::InvalidChar { ch: '/' })
        );
    }

    #[test]
    fn invalid_char_colon() {
        assert_eq!(
            InterfaceName::try_from("eth:0"),
            Err(InterfaceNameError::InvalidChar { ch: ':' })
        );
    }

    #[test]
    fn invalid_char_space() {
        assert_eq!(
            InterfaceName::try_from("eth 0"),
            Err(InterfaceNameError::InvalidChar { ch: ' ' })
        );
    }

    #[test]
    fn invalid_char_tab() {
        assert_eq!(
            InterfaceName::try_from("eth\t0"),
            Err(InterfaceNameError::InvalidChar { ch: '\t' })
        );
    }

    #[test]
    fn invalid_char_newline() {
        assert_eq!(
            InterfaceName::try_from("eth\n0"),
            Err(InterfaceNameError::InvalidChar { ch: '\n' })
        );
    }

    #[test]
    fn reserved_dot() {
        assert_eq!(
            InterfaceName::try_from("."),
            Err(InterfaceNameError::Reserved)
        );
    }

    #[test]
    fn reserved_dotdot() {
        assert_eq!(
            InterfaceName::try_from(".."),
            Err(InterfaceNameError::Reserved)
        );
    }

    #[test]
    fn from_bytes_not_utf8() {
        let result = InterfaceName::from_bytes(b"eth\xff");
        match result {
            Err(InterfaceNameError::NotUtf8 { bytes }) => {
                assert_eq!(bytes, b"eth\xff");
                let display = InterfaceNameError::NotUtf8 { bytes }.to_string();
                assert!(
                    display.contains("65 74 68 ff"),
                    "Display should contain hex bytes, got: {display}"
                );
            }
            other => panic!("expected NotUtf8, got {other:?}"),
        }
    }

    #[test]
    fn debug_format() {
        let name: InterfaceName = "eth0".parse().unwrap();
        assert_eq!(format!("{name:?}"), r#"InterfaceName("eth0")"#);
    }

    #[test]
    fn unicode_whitespace_rejected() {
        assert_eq!(
            InterfaceName::try_from("eth\u{00A0}0"),
            Err(InterfaceNameError::InvalidChar { ch: '\u{00A0}' })
        );
        assert_eq!(
            InterfaceName::try_from(String::from("eth\u{00A0}0")),
            Err(InterfaceNameError::InvalidChar { ch: '\u{00A0}' })
        );
    }

    #[test]
    fn interface_no_name() {
        let iface = Interface {
            name: None,
            mac: MacAddr::ZERO,
            media: MediaType::from(0x0001_u16),
        };
        assert_eq!(iface.name, None);
    }

    #[test]
    fn interface_with_name() {
        let iface = Interface {
            name: Some("eth0".parse::<InterfaceName>().unwrap()),
            mac: MacAddr::ZERO,
            media: MediaType::from(0x0001_u16),
        };
        assert_eq!(iface.name, Some("eth0".parse::<InterfaceName>().unwrap()));
    }

    #[test]
    fn interface_equality() {
        let a = Interface {
            name: None,
            mac: MacAddr::BROADCAST,
            media: MediaType::from(0x0100_u16),
        };
        let b = Interface {
            name: None,
            mac: MacAddr::BROADCAST,
            media: MediaType::from(0x0100_u16),
        };
        assert_eq!(a, b);
    }
}
