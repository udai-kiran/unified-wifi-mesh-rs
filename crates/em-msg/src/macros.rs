// SPDX-License-Identifier: Apache-2.0

//! Crate-private macros for the TLV bodies (design note 0009, D2).

/// Declares an open newtype over an integer, with the same shape as `em-types`' `wire_code!`:
/// unknown values are kept, named constants, `name()`, `ALL` and a `Debug` that prints the name.
///
/// ```text
/// open_code! {
///     /// Doc of the type.
///     pub struct Mode(u8);
///     const HEX_WIDTH: usize = 2;
///     entries: [
///         (OFF, 0x00, "No mode."),
///         (ON, 0x01, "Mode on."),
///     ]
/// }
/// ```
macro_rules! open_code {
    (
        $( #[$type_meta:meta] )*
        $vis:vis struct $Name:ident($Int:ty);
        const HEX_WIDTH: usize = $W:literal;
        entries: [ $( ($Const:ident, $val:literal, $doc:literal) ),* $(,)? ]
    ) => {
        $( #[$type_meta] )*
        #[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        $vis struct $Name(
            /// The raw wire value.
            pub $Int
        );

        impl $Name {
            $(
                #[doc = $doc]
                pub const $Const: Self = Self($val);
            )*

            /// Returns the name of this value if it is a named constant, or `None`
            /// for any other value.
            #[must_use]
            pub const fn name(self) -> Option<&'static str> {
                match self.0 {
                    $( $val => Some(stringify!($Const)), )*
                    _ => None,
                }
            }

            /// All named constants, in declaration order.
            pub const ALL: &'static [Self] = &[ $( Self::$Const, )* ];
        }

        impl From<$Int> for $Name {
            fn from(value: $Int) -> Self {
                Self(value)
            }
        }

        impl From<$Name> for $Int {
            fn from(value: $Name) -> $Int {
                value.0
            }
        }

        impl core::fmt::Debug for $Name {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                match self.name() {
                    Some(name) => write!(f, "{name}(0x{:0>w$X})", self.0, w = $W),
                    None => write!(f, "{}(0x{:0>w$X})", stringify!($Name), self.0, w = $W),
                }
            }
        }
    };
}

/// Declares a TLV body that is a distinct type around a layout shared with other TLVs, and
/// implements [`TlvBody`](crate::bodies::TlvBody) for it with its own TLV type.
///
/// The inner type needs inherent `decode_value(&'a [u8]) -> Result<Self, DecodeError>` and
/// `encode_value(&self) -> Result<Vec<u8>, EncodeError>` (or by value for a `Copy` layout).
/// The attributes, including any `derive`, are the caller's.
///
/// ```text
/// tlv_newtype! {
///     /// Doc of the TLV.
///     #[derive(Debug, PartialEq, Eq)]
///     pub struct SupportedService(pub ServiceList) => TlvType::SUPPORTED_SERVICE;
/// }
/// // Inner type that borrows from the value:
/// tlv_newtype! {
///     #[derive(Debug, PartialEq, Eq)]
///     pub struct Name<'a>(pub InnerBody<'a>) => TlvType::X;
/// }
/// ```
macro_rules! tlv_newtype {
    (
        $( #[$meta:meta] )*
        $vis:vis struct $Name:ident($field_vis:vis $Inner:ty) => $tlv_type:expr;
    ) => {
        $( #[$meta] )*
        $vis struct $Name($field_vis $Inner);

        impl<'a> $crate::bodies::TlvBody<'a> for $Name {
            const TLV_TYPE: em_types::TlvType = $tlv_type;

            fn decode_value(value: &'a [u8]) -> Result<Self, $crate::error::DecodeError> {
                Ok(Self(<$Inner>::decode_value(value)?))
            }

            fn encode_value(&self) -> Result<Vec<u8>, $crate::error::EncodeError> {
                self.0.encode_value()
            }
        }
    };
    (
        $( #[$meta:meta] )*
        $vis:vis struct $Name:ident<$lt:lifetime>($field_vis:vis $Inner:ty) => $tlv_type:expr;
    ) => {
        $( #[$meta] )*
        $vis struct $Name<$lt>($field_vis $Inner);

        impl<$lt> $crate::bodies::TlvBody<$lt> for $Name<$lt> {
            const TLV_TYPE: em_types::TlvType = $tlv_type;

            fn decode_value(value: &$lt [u8]) -> Result<Self, $crate::error::DecodeError> {
                Ok(Self(<$Inner>::decode_value(value)?))
            }

            fn encode_value(&self) -> Result<Vec<u8>, $crate::error::EncodeError> {
                self.0.encode_value()
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use crate::bodies::TlvBody;
    use crate::body::{Reader, Writer};
    use crate::error::{DecodeError, EncodeError};
    use crate::tlv::Tlv;
    use em_types::TlvType;

    open_code! {
        /// Test code over a `u8`.
        pub struct Mode(u8);
        const HEX_WIDTH: usize = 2;
        entries: [
            (OFF, 0x00, "Off."),
            (ON, 0x01, "On."),
            (AUTO, 0x7F, "Automatic."),
        ]
    }

    open_code! {
        /// Test code over a `u16`.
        pub struct Wide(u16);
        const HEX_WIDTH: usize = 4;
        entries: [(ONLY, 0x0102, "The only one.")]
    }

    #[test]
    fn open_code_names_its_constants() {
        assert_eq!(Mode::ON, Mode(1));
        assert_eq!(Mode::ON.name(), Some("ON"));
        assert_eq!(Mode(0x7F).name(), Some("AUTO"));
        assert_eq!(Mode(2).name(), None);
        assert_eq!(Mode::ALL, &[Mode::OFF, Mode::ON, Mode::AUTO]);
    }

    #[test]
    fn open_code_keeps_unknown_values() {
        let unknown = Mode::from(0xEE);
        assert_eq!(u8::from(unknown), 0xEE);
        assert_eq!(unknown.0, 0xEE);
        assert_eq!(u16::from(Wide::from(0xBEEF)), 0xBEEF);
    }

    #[test]
    fn open_code_derives_eq_hash_and_ord() {
        let mut set = std::collections::HashSet::new();
        set.insert(Mode::ON);
        assert!(set.contains(&Mode(1)));
        assert!(Mode::OFF < Mode::ON);
        assert_eq!(Mode::ON.max(Mode::AUTO), Mode::AUTO);
        let copy = Mode::ON;
        assert_eq!(copy, Mode::ON);
    }

    #[test]
    fn open_code_debug_matches_the_em_types_style() {
        assert_eq!(format!("{:?}", Mode::AUTO), "AUTO(0x7F)");
        assert_eq!(format!("{:?}", Mode(0x0A)), "Mode(0x0A)");
        assert_eq!(format!("{:?}", Wide::ONLY), "ONLY(0x0102)");
        assert_eq!(format!("{:?}", Wide(0x0A)), "Wide(0x000A)");
    }

    /// Inner layout shared by two TLVs: one byte.
    #[derive(Debug, PartialEq, Eq)]
    pub struct OneByte(u8);

    impl OneByte {
        fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
            let mut reader = Reader::new(value);
            let byte = reader.u8()?;
            reader.finish()?;
            Ok(Self(byte))
        }

        // The signature is the one `tlv_newtype!` calls; this layout cannot fail.
        #[allow(clippy::unnecessary_wraps)]
        fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
            let mut writer = Writer::new();
            writer.u8(self.0);
            Ok(writer.into_bytes())
        }
    }

    /// Inner layout that borrows its value.
    #[derive(Debug, PartialEq, Eq)]
    pub struct Borrowed<'a>(&'a [u8]);

    impl<'a> Borrowed<'a> {
        fn decode_value(value: &'a [u8]) -> Result<Self, DecodeError> {
            let mut reader = Reader::new(value);
            reader.skip(1)?;
            Ok(Self(value))
        }

        fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
            let mut writer = Writer::new();
            writer.count_u8(self.0.len())?;
            writer.bytes(self.0);
            Ok(writer.into_bytes())
        }
    }

    tlv_newtype! {
        /// First TLV of the shared layout.
        #[derive(Debug, PartialEq, Eq)]
        pub struct FirstTlv(pub OneByte) => TlvType(0xF8);
    }

    tlv_newtype! {
        /// Second TLV of the shared layout.
        #[derive(Debug, PartialEq, Eq)]
        pub struct SecondTlv(pub OneByte) => TlvType(0xF9);
    }

    tlv_newtype! {
        /// A TLV whose inner body borrows.
        #[derive(Debug, PartialEq, Eq)]
        pub struct BorrowingTlv<'a>(pub Borrowed<'a>) => TlvType(0xFA);
    }

    #[test]
    fn tlv_newtypes_share_a_layout_with_their_own_type() {
        assert_eq!(FirstTlv::TLV_TYPE, TlvType(0xF8));
        assert_eq!(SecondTlv::TLV_TYPE, TlvType(0xF9));
        assert_eq!(FirstTlv::decode_value(&[7]), Ok(FirstTlv(OneByte(7))));
        assert_eq!(SecondTlv(OneByte(7)).encode_value(), Ok(vec![7]));
    }

    #[test]
    fn tlv_newtype_passes_on_the_inner_errors() {
        assert_eq!(
            FirstTlv::decode_value(&[]),
            Err(DecodeError::Truncated {
                needed: 1,
                remaining: 0
            })
        );
        assert_eq!(
            SecondTlv::decode_value(&[1, 2]),
            Err(DecodeError::TrailingBytes { remaining: 1 })
        );
    }

    #[test]
    fn tlv_newtype_checks_the_tlv_type_through_tlv() {
        let tlv = Tlv {
            tlv_type: TlvType(0xF8),
            value: &[3],
        };
        assert_eq!(tlv.decode::<FirstTlv>(), Ok(FirstTlv(OneByte(3))));
        assert_eq!(
            tlv.decode::<SecondTlv>(),
            Err(DecodeError::WrongType {
                expected: TlvType(0xF9),
                found: TlvType(0xF8),
            })
        );
    }

    #[test]
    fn tlv_newtype_with_a_lifetime_borrows_the_value() {
        let value = [1, 2, 3];
        let tlv = Tlv {
            tlv_type: TlvType(0xFA),
            value: &value,
        };
        let body = tlv.decode::<BorrowingTlv<'_>>().unwrap();
        assert_eq!(body.0.0, &value);
        assert_eq!(body.encode_value(), Ok(vec![3, 1, 2, 3]));
    }
}
