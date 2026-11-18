// SPDX-License-Identifier: Apache-2.0

//! Hex dump of a byte slice, laid out like upstream `util::print_hex_dump`.

use core::fmt;

/// Wraps a byte slice so that `Display` prints a hex dump.
///
/// The text is byte-for-byte the same as upstream `print_hex_dump`
/// (`src/utils/util.cpp:214-259`), assuming the C locale, where printable
/// means `0x20..=0x7e`. Each line holds up to 16 bytes:
///
/// - the offset as `"  {offset:04x} "` (more digits above `0xffff`),
/// - each byte as `" {byte:02x}"`,
/// - `"   "` for each missing byte on a short last line,
/// - `"  "` and the text column (`.` for bytes that are not printable),
/// - a newline.
///
/// Intentional difference: an empty slice prints nothing. Upstream prints
/// `buffer NULL or BAD LENGTH = 0 :`. A Rust slice cannot be null, and an
/// empty dump is not an error.
///
/// Nothing is allocated while printing.
#[derive(Debug, Clone, Copy)]
pub struct HexDump<'a>(pub &'a [u8]);

/// How many bytes go on one line.
const BYTES_PER_LINE: usize = 16;

/// Writes the byte as itself when it is printable, otherwise a dot.
fn write_text_char(f: &mut fmt::Formatter<'_>, byte: u8) -> fmt::Result {
    let ch = if (0x20..=0x7e).contains(&byte) {
        char::from(byte)
    } else {
        '.'
    };
    write!(f, "{ch}")
}

/// Writes one line (no more than 16 bytes) of the dump.
fn write_line(f: &mut fmt::Formatter<'_>, offset: usize, line: &[u8]) -> fmt::Result {
    write!(f, "  {offset:04x} ")?;
    for byte in line {
        write!(f, " {byte:02x}")?;
    }
    let missing = BYTES_PER_LINE - line.len();
    for _ in 0..missing {
        f.write_str("   ")?;
    }
    f.write_str("  ")?;
    for byte in line {
        write_text_char(f, *byte)?;
    }
    f.write_str("\n")
}

impl fmt::Display for HexDump<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut offset = 0;
        for line in self.0.chunks(BYTES_PER_LINE) {
            write_line(f, offset, line)?;
            offset += BYTES_PER_LINE;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::HexDump;

    // The expected text below is the output of a C program that holds
    // upstream's loop (util.cpp lines 232-258) with the output set to stdout.

    /// Bytes `0x10`, `0x11`, ... (a mix of control and printable bytes).
    fn sequence(len: u8) -> Vec<u8> {
        let mut bytes = Vec::new();
        for i in 0..len {
            bytes.push(0x10 + i);
        }
        bytes
    }

    #[test]
    fn length_1() {
        let expected = "  0000  10                                               .\n";
        assert_eq!(HexDump(&sequence(1)).to_string(), expected);
    }

    #[test]
    fn length_15() {
        let expected = "  0000  10 11 12 13 14 15 16 17 18 19 1a 1b 1c 1d 1e     ...............\n";
        assert_eq!(HexDump(&sequence(15)).to_string(), expected);
    }

    #[test]
    fn length_16() {
        let expected =
            "  0000  10 11 12 13 14 15 16 17 18 19 1a 1b 1c 1d 1e 1f  ................\n";
        assert_eq!(HexDump(&sequence(16)).to_string(), expected);
    }

    #[test]
    fn length_17() {
        let expected = concat!(
            "  0000  10 11 12 13 14 15 16 17 18 19 1a 1b 1c 1d 1e 1f  ................\n",
            "  0010  20                                                \n",
        );
        assert_eq!(HexDump(&sequence(17)).to_string(), expected);
    }

    #[test]
    fn length_32() {
        let expected = concat!(
            "  0000  10 11 12 13 14 15 16 17 18 19 1a 1b 1c 1d 1e 1f  ................\n",
            "  0010  20 21 22 23 24 25 26 27 28 29 2a 2b 2c 2d 2e 2f   !\"#$%&'()*+,-./\n",
        );
        assert_eq!(HexDump(&sequence(32)).to_string(), expected);
    }

    #[test]
    fn edge_bytes() {
        let expected = "  0000  1f 20 7e 7f ff                                   . ~..\n";
        let bytes = [0x1f, 0x20, 0x7e, 0x7f, 0xff];
        assert_eq!(HexDump(&bytes).to_string(), expected);
    }

    #[test]
    fn empty_prints_nothing() {
        assert_eq!(HexDump(&[]).to_string(), "");
    }

    #[test]
    fn offset_above_0xffff() {
        let mut bytes = Vec::new();
        for i in 0..65552_usize {
            bytes.push(u8::try_from(i % 256).expect("i % 256 fits in u8"));
        }
        let text = HexDump(&bytes).to_string();
        assert_eq!(text.lines().count(), 4097);
        let last = text.lines().last().expect("there is a last line");
        assert_eq!(
            last,
            "  10000  00 01 02 03 04 05 06 07 08 09 0a 0b 0c 0d 0e 0f  ................"
        );
        assert!(text.ends_with('\n'));
    }
}
