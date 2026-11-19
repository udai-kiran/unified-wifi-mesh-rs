// SPDX-License-Identifier: Apache-2.0

//! The two declaration macros of the crate: `open_enum!` and `record!`.
//!
//! Both generate the type and, under `cfg(test)`, a `pub(crate)` table describing it, so the
//! golden test reads the declaration itself rather than a hand-kept copy.

/// Declares an open enum: a newtype over the C enum's integer with one associated const per
/// retained value. Unknown codes are representable and round-trip unchanged. Sentinel values
/// (`*_max`, `*_unknown`, `*_none`, reserved) are listed with a reason and are not named consts.
macro_rules! open_enum {
    (
        $(#[$meta:meta])*
        pub struct $Name:ident($Int:ty) = $c_type:literal;
        named: [ $( $(#[$cmeta:meta])* $Const:ident = $val:literal => $c_ident:literal ),* $(,)? ];
        sentinels: [ $( $sval:literal => $sident:literal, $reason:literal );* $(;)? ];
    ) => {
        $(#[$meta])*
        #[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $Name(
            /// The raw value.
            pub $Int
        );

        impl $Name {
            $(
                $(#[$cmeta])*
                #[doc = concat!("C `", $c_ident, "` = ", stringify!($val), ".")]
                pub const $Const: Self = Self($val);
            )*

            /// All named values, in declaration order.
            pub const ALL: &'static [Self] = &[ $( Self::$Const, )* ];

            /// The name of a retained value, or `None` for unknown codes and sentinels.
            #[must_use]
            pub const fn name(self) -> Option<&'static str> {
                match self.0 {
                    $( $val => Some(stringify!($Const)), )*
                    _ => None,
                }
            }

            #[cfg(test)]
            pub(crate) const TABLE: $crate::mapping::EnumTable = $crate::mapping::EnumTable {
                c_type: $c_type,
                rust_name: stringify!($Name),
                bits: <$Int>::BITS,
                entries: &[
                    $( $crate::mapping::EnumEntry {
                        c_ident: $c_ident,
                        value: Self::$Const.0 as u32,
                        rust_const: Some(stringify!($Const)),
                        not_retained: None,
                    }, )*
                    $( $crate::mapping::EnumEntry {
                        c_ident: $sident,
                        value: $sval,
                        rust_const: None,
                        not_retained: Some($reason),
                    }, )*
                ],
            };
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

        impl ::core::fmt::Debug for $Name {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                match self.name() {
                    Some(name) => write!(f, "{}({})", name, self.0),
                    None => write!(f, "{}({})", stringify!($Name), self.0),
                }
            }
        }

        #[cfg(test)]
        impl $crate::mapping::NativeShape for $Name {
            const SHAPE: $crate::mapping::Shape = $crate::mapping::Shape::SCALAR;
        }
    };
}

/// Declares a record: a struct with public fields plus, under `cfg(test)`, its field table.
///
/// `copy` records derive `Clone` and `Copy` and must stay within `COPY_SIZE_LIMIT` (checked at
/// compile time). `owned` records derive neither, and carry doctest probes that fail to compile
/// if either trait ever appears.
macro_rules! record {
    (
        $(#[$meta:meta])*
        pub struct $Name:ident = $c_type:literal, copy {
            $( $(#[$fmeta:meta])* pub $field:ident : $ty:ty = $map:expr ),* $(,)?
        }
    ) => {
        record! {
            @emit
            [
                $(#[$meta])*
                #[doc = ""]
                #[doc = "`Copy`: small enough to pass by value (`size_of` at most 256 bytes,"]
                #[doc = "checked at compile time and by the golden test)."]
                #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
            ]
            $Name, $c_type, true,
            { $( $(#[$fmeta])* pub $field : $ty = $map ),* }
        }

        const _: () = assert!(
            ::core::mem::size_of::<$Name>() <= $crate::COPY_SIZE_LIMIT,
            "a copy record must stay within COPY_SIZE_LIMIT; declare it `owned`"
        );
    };
    (
        $(#[$meta:meta])*
        pub struct $Name:ident = $c_type:literal, owned {
            $( $(#[$fmeta:meta])* pub $field:ident : $ty:ty = $map:expr ),* $(,)?
        }
    ) => {
        record! {
            @emit
            [
                $(#[$meta])*
                #[doc = ""]
                #[doc = "Neither `Clone` nor `Copy`: it is larger than 256 bytes or holds a heap"]
                #[doc = "collection or an `em_types::Interface`. Store it as `Box<Self>` where the"]
                #[doc = "inline size matters (0007 D3). The first block below compiles; the next two"]
                #[doc = "fail to compile."]
                #[doc = ""]
                #[doc = "```"]
                #[doc = "fn needs_clone<T: Clone>() {}"]
                #[doc = "fn needs_copy<T: Copy>() {}"]
                #[doc = "needs_clone::<em_dm::HaulType>();"]
                #[doc = "needs_copy::<em_dm::HaulType>();"]
                #[doc = concat!("let _: Option<em_dm::", stringify!($Name), "> = None;")]
                #[doc = "```"]
                #[doc = ""]
                #[doc = "```compile_fail"]
                #[doc = "fn needs_clone<T: Clone>() {}"]
                #[doc = "needs_clone::<em_dm::HaulType>();"]
                #[doc = concat!("needs_clone::<em_dm::", stringify!($Name), ">();")]
                #[doc = "```"]
                #[doc = ""]
                #[doc = "```compile_fail"]
                #[doc = "fn needs_copy<T: Copy>() {}"]
                #[doc = "needs_copy::<em_dm::HaulType>();"]
                #[doc = concat!("needs_copy::<em_dm::", stringify!($Name), ">();")]
                #[doc = "```"]
                #[derive(Debug, PartialEq, Eq, Hash)]
            ]
            $Name, $c_type, false,
            { $( $(#[$fmeta])* pub $field : $ty = $map ),* }
        }
    };
    (
        @emit
        [ $(#[$meta:meta])* ]
        $Name:ident, $c_type:literal, $is_copy:literal,
        { $( $(#[$fmeta:meta])* pub $field:ident : $ty:ty = $map:expr ),* }
    ) => {
        $(#[$meta])*
        #[allow(
            clippy::struct_excessive_bools,
            reason = "mirrors the bool and bit-field members of the C record"
        )]
        pub struct $Name {
            $( $(#[$fmeta])* pub $field : $ty, )*
        }

        #[cfg(test)]
        impl $Name {
            pub(crate) const TABLE: $crate::mapping::RecordTable = $crate::mapping::RecordTable {
                c_type: $c_type,
                rust_name: stringify!($Name),
                is_copy: $is_copy,
                size: ::core::mem::size_of::<$Name>(),
                fields: &[
                    $( $crate::mapping::FieldMap {
                        rust_field: stringify!($field),
                        rust_type: stringify!($ty),
                        map: $map,
                        shape: <$ty as $crate::mapping::NativeShape>::SHAPE,
                    }, )*
                ],
            };
        }

        #[cfg(test)]
        impl $crate::mapping::NativeShape for $Name {
            const SHAPE: $crate::mapping::Shape = $crate::mapping::Shape::SCALAR;
        }
    };
}
