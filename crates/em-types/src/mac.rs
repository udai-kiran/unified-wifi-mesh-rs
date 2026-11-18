// SPDX-License-Identifier: Apache-2.0

//! MAC address type ([`MacAddr`]) and its parse error ([`MacAddrParseError`]).

use std::fmt;
use std::str::FromStr;

/// A 6-byte IEEE 802 MAC address.
///
/// Stored as a plain array of bytes; all byte orders are those of the network
/// wire. Formatting is lowercase colon-separated (`aa:bb:cc:dd:ee:ff`), matching
/// upstream's `macbytes_to_string`.
///
/// Parsing accepts two forms (case-insensitive hex):
/// - Colon-separated: `aa:bb:cc:dd:ee:ff` (exactly 17 characters)
/// - Compact: `aabbccddeeff` (exactly 12 characters)
///
/// Both forms match what upstream's `string_to_macbytes` accepts
/// (`src/dm/dm_easy_mesh.cpp:2070-2078`). Whitespace, `-` separators,
/// single-digit octets, and other formats are rejected.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct MacAddr([u8; 6]);

impl MacAddr {
    /// Constructs a [`MacAddr`] from a byte array.
    #[must_use]
    pub const fn new(octets: [u8; 6]) -> Self {
        Self(octets)
    }

    /// Returns the 6 octets of the address in network byte order.
    #[must_use]
    pub const fn octets(self) -> [u8; 6] {
        self.0
    }

    /// The all-zeros MAC address `00:00:00:00:00:00`.
    pub const ZERO: Self = Self([0, 0, 0, 0, 0, 0]);

    /// The broadcast address `ff:ff:ff:ff:ff:ff`.
    pub const BROADCAST: Self = Self([0xff, 0xff, 0xff, 0xff, 0xff, 0xff]);

    /// The IEEE 1905.1 multicast address `01:80:c2:00:00:13`.
    pub const IEEE1905_MULTICAST: Self = Self([0x01, 0x80, 0xc2, 0x00, 0x00, 0x13]);

    /// Returns `true` if the I/G (Individual/Group) bit is set, i.e., if this
    /// is a multicast or broadcast address.
    ///
    /// The I/G bit is the least-significant bit of the first octet.
    #[must_use]
    pub const fn is_multicast(self) -> bool {
        self.0[0] & 0x01 != 0
    }

    /// Returns `true` if all octets are zero (`00:00:00:00:00:00`).
    #[must_use]
    pub const fn is_zero(self) -> bool {
        self.0[0] == 0
            && self.0[1] == 0
            && self.0[2] == 0
            && self.0[3] == 0
            && self.0[4] == 0
            && self.0[5] == 0
    }

    /// Returns `true` if this is the broadcast address (`ff:ff:ff:ff:ff:ff`).
    #[must_use]
    pub const fn is_broadcast(self) -> bool {
        self.0[0] == 0xff
            && self.0[1] == 0xff
            && self.0[2] == 0xff
            && self.0[3] == 0xff
            && self.0[4] == 0xff
            && self.0[5] == 0xff
    }
}

// --- Conversions -----------------------------------------------------------

impl From<[u8; 6]> for MacAddr {
    fn from(octets: [u8; 6]) -> Self {
        Self(octets)
    }
}

impl From<MacAddr> for [u8; 6] {
    fn from(addr: MacAddr) -> [u8; 6] {
        addr.0
    }
}

impl AsRef<[u8; 6]> for MacAddr {
    fn as_ref(&self) -> &[u8; 6] {
        &self.0
    }
}

impl AsRef<[u8]> for MacAddr {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl TryFrom<&[u8]> for MacAddr {
    type Error = MacAddrParseError;

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        bytes
            .try_into()
            .map(Self)
            .map_err(|_| MacAddrParseError::InvalidByteLength { len: bytes.len() })
    }
}

// --- Formatting ------------------------------------------------------------

impl fmt::Display for MacAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let o = self.0;
        write!(
            f,
            "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            o[0], o[1], o[2], o[3], o[4], o[5]
        )
    }
}

/// Debug output is identical to [`Display`](fmt::Display): `aa:bb:cc:dd:ee:ff`.
impl fmt::Debug for MacAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

// --- Parsing ---------------------------------------------------------------

/// Converts a single hex digit byte to its numeric value, or returns `None`.
const fn hex_digit(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// Parses two hex digit bytes into one byte value.
fn hex_byte(hi: u8, lo: u8) -> Option<u8> {
    Some((hex_digit(hi)? << 4) | hex_digit(lo)?)
}

impl FromStr for MacAddr {
    type Err = MacAddrParseError;

    /// Parses a MAC address from a string.
    ///
    /// Accepts exactly two forms (case-insensitive hex):
    /// - `aa:bb:cc:dd:ee:ff` (17 characters, colon-separated)
    /// - `aabbccddeeff` (12 characters, compact)
    ///
    /// Any other form — including `-` separators, single-digit octets, mixed
    /// separators, whitespace, or trailing data — is rejected.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Reject non-ASCII early to avoid indexing on multi-byte char boundaries.
        if !s.is_ascii() {
            return Err(MacAddrParseError::InvalidFormat);
        }
        let b = s.as_bytes();

        if b.len() == 17 {
            // Colon-separated form: aa:bb:cc:dd:ee:ff
            if b[2] != b':' || b[5] != b':' || b[8] != b':' || b[11] != b':' || b[14] != b':' {
                return Err(MacAddrParseError::InvalidFormat);
            }
            let o0 = hex_byte(b[0], b[1]).ok_or(MacAddrParseError::InvalidFormat)?;
            let o1 = hex_byte(b[3], b[4]).ok_or(MacAddrParseError::InvalidFormat)?;
            let o2 = hex_byte(b[6], b[7]).ok_or(MacAddrParseError::InvalidFormat)?;
            let o3 = hex_byte(b[9], b[10]).ok_or(MacAddrParseError::InvalidFormat)?;
            let o4 = hex_byte(b[12], b[13]).ok_or(MacAddrParseError::InvalidFormat)?;
            let o5 = hex_byte(b[15], b[16]).ok_or(MacAddrParseError::InvalidFormat)?;
            Ok(Self([o0, o1, o2, o3, o4, o5]))
        } else if b.len() == 12 {
            // Compact form: aabbccddeeff
            let o0 = hex_byte(b[0], b[1]).ok_or(MacAddrParseError::InvalidFormat)?;
            let o1 = hex_byte(b[2], b[3]).ok_or(MacAddrParseError::InvalidFormat)?;
            let o2 = hex_byte(b[4], b[5]).ok_or(MacAddrParseError::InvalidFormat)?;
            let o3 = hex_byte(b[6], b[7]).ok_or(MacAddrParseError::InvalidFormat)?;
            let o4 = hex_byte(b[8], b[9]).ok_or(MacAddrParseError::InvalidFormat)?;
            let o5 = hex_byte(b[10], b[11]).ok_or(MacAddrParseError::InvalidFormat)?;
            Ok(Self([o0, o1, o2, o3, o4, o5]))
        } else {
            Err(MacAddrParseError::InvalidFormat)
        }
    }
}

// --- Error type ------------------------------------------------------------

/// Error returned when a [`MacAddr`] cannot be parsed or converted.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MacAddrParseError {
    /// The string is not a recognised MAC address format.
    InvalidFormat,
    /// A byte slice had the wrong length (must be exactly 6 bytes).
    InvalidByteLength {
        /// The actual byte-slice length.
        len: usize,
    },
}

impl fmt::Display for MacAddrParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidFormat => {
                write!(f, "invalid MAC address format")
            }
            Self::InvalidByteLength { len } => {
                write!(f, "MAC address byte slice has length {len}, expected 6")
            }
        }
    }
}

impl std::error::Error for MacAddrParseError {}

// --- Unit tests ------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_roundtrip() {
        let addr: MacAddr = "aa:bb:cc:dd:ee:ff".parse().unwrap();
        assert_eq!(addr.to_string(), "aa:bb:cc:dd:ee:ff");
    }

    #[test]
    fn lowercase_from_uppercase_input() {
        let addr: MacAddr = "AA:BB:CC:DD:EE:FF".parse().unwrap();
        assert_eq!(addr.to_string(), "aa:bb:cc:dd:ee:ff");
    }

    #[test]
    fn compact_lowercase_accepted() {
        let addr: MacAddr = "aabbccddeeff".parse().unwrap();
        assert_eq!(addr.to_string(), "aa:bb:cc:dd:ee:ff");
    }

    #[test]
    fn compact_uppercase_accepted() {
        let addr: MacAddr = "AABBCCDDEEFF".parse().unwrap();
        assert_eq!(addr.to_string(), "aa:bb:cc:dd:ee:ff");
    }

    #[test]
    fn reject_five_octets_colon() {
        assert_eq!(
            "aa:bb:cc:dd:ee".parse::<MacAddr>(),
            Err(MacAddrParseError::InvalidFormat)
        );
    }

    #[test]
    fn reject_seven_octets_colon() {
        assert_eq!(
            "aa:bb:cc:dd:ee:ff:00".parse::<MacAddr>(),
            Err(MacAddrParseError::InvalidFormat)
        );
    }

    #[test]
    fn reject_single_digit_octet() {
        assert_eq!(
            "a:bb:cc:dd:ee:ff".parse::<MacAddr>(),
            Err(MacAddrParseError::InvalidFormat)
        );
    }

    #[test]
    fn reject_mixed_separators() {
        assert_eq!(
            "aa:bb-cc:dd:ee:ff".parse::<MacAddr>(),
            Err(MacAddrParseError::InvalidFormat)
        );
    }

    #[test]
    fn reject_dash_separators() {
        assert_eq!(
            "aa-bb-cc-dd-ee-ff".parse::<MacAddr>(),
            Err(MacAddrParseError::InvalidFormat)
        );
    }

    #[test]
    fn reject_leading_space() {
        assert_eq!(
            " aa:bb:cc:dd:ee:ff".parse::<MacAddr>(),
            Err(MacAddrParseError::InvalidFormat)
        );
    }

    #[test]
    fn reject_trailing_space() {
        assert_eq!(
            "aa:bb:cc:dd:ee:ff ".parse::<MacAddr>(),
            Err(MacAddrParseError::InvalidFormat)
        );
    }

    #[test]
    fn reject_trailing_data() {
        assert_eq!(
            "aa:bb:cc:dd:ee:ffXX".parse::<MacAddr>(),
            Err(MacAddrParseError::InvalidFormat)
        );
    }

    #[test]
    fn reject_non_hex() {
        assert_eq!(
            "gg:bb:cc:dd:ee:ff".parse::<MacAddr>(),
            Err(MacAddrParseError::InvalidFormat)
        );
    }

    #[test]
    fn reject_empty() {
        assert_eq!("".parse::<MacAddr>(), Err(MacAddrParseError::InvalidFormat));
    }

    #[test]
    fn reject_non_ascii() {
        // "ﬀ" (U+FB00 LATIN SMALL LIGATURE FF) is non-ASCII; must not panic.
        assert_eq!(
            "aa:bb:cc:dd:ee:\u{FB00}".parse::<MacAddr>(),
            Err(MacAddrParseError::InvalidFormat)
        );
    }

    #[test]
    fn reject_plus_sign_would_be_accepted_by_from_str_radix() {
        // Ensure we reject "+a" as a hex octet (from_str_radix would accept it).
        assert_eq!(
            "+a:bb:cc:dd:ee:ff".parse::<MacAddr>(),
            Err(MacAddrParseError::InvalidFormat)
        );
    }

    #[test]
    fn try_from_bytes_five() {
        let b: &[u8] = &[0, 1, 2, 3, 4];
        assert_eq!(
            MacAddr::try_from(b),
            Err(MacAddrParseError::InvalidByteLength { len: 5 })
        );
    }

    #[test]
    fn try_from_bytes_six() {
        let b: &[u8] = &[0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff];
        let addr = MacAddr::try_from(b).unwrap();
        assert_eq!(addr.to_string(), "aa:bb:cc:dd:ee:ff");
    }

    #[test]
    fn try_from_bytes_seven() {
        let b: &[u8] = &[0, 1, 2, 3, 4, 5, 6];
        assert_eq!(
            MacAddr::try_from(b),
            Err(MacAddrParseError::InvalidByteLength { len: 7 })
        );
    }

    #[test]
    fn multicast_flag_on_consts_and_unicast() {
        assert!(MacAddr::BROADCAST.is_multicast(), "broadcast is multicast");
        assert!(
            MacAddr::IEEE1905_MULTICAST.is_multicast(),
            "1905 multicast is multicast"
        );
        assert!(!MacAddr::ZERO.is_multicast(), "zero is not multicast");
        let unicast: MacAddr = [0x02, 0x00, 0x00, 0x00, 0x00, 0x01].into();
        assert!(!unicast.is_multicast(), "unicast is not multicast");
    }

    #[test]
    fn broadcast_and_zero_predicates() {
        assert!(MacAddr::BROADCAST.is_broadcast());
        assert!(!MacAddr::ZERO.is_broadcast());
        assert!(MacAddr::ZERO.is_zero());
        assert!(!MacAddr::BROADCAST.is_zero());
        assert!(!MacAddr::IEEE1905_MULTICAST.is_zero());
        assert!(!MacAddr::IEEE1905_MULTICAST.is_broadcast());
    }

    #[test]
    fn ieee1905_multicast_display() {
        assert_eq!(MacAddr::IEEE1905_MULTICAST.to_string(), "01:80:c2:00:00:13");
    }

    #[test]
    fn debug_equals_display() {
        let addr: MacAddr = "aa:bb:cc:dd:ee:ff".parse().unwrap();
        assert_eq!(format!("{addr:?}"), format!("{addr}"));
    }
}
