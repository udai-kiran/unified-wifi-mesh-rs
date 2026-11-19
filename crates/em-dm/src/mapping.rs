// SPDX-License-Identifier: Apache-2.0

//! Test-only vocabulary shared by the generated field tables and the golden test.
//!
//! The `record!` and `open_enum!` macros emit a `TABLE` constant next to every struct and enum
//! they define, built from the same declaration, so the table cannot drift from the type.
//! This module defines what those tables contain.

use em_types::{FreqBand, Interface, MacAddr, MediaType, ProfileType};

use crate::bits::UBits;
use crate::bounded::BoundedVec;
use crate::float::Float32;
use crate::secret::Secret;
use crate::text::{CStrBytes, LenBytes, Text};

/// Broad category of a native field type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShapeKind {
    /// Integer, bool, enum, bit-field, MAC address, interface or record.
    Scalar,
    /// Fixed `[T; N]`.
    Array,
    /// [`BoundedVec`].
    List,
    /// [`CStrBytes`].
    CStrBytes,
    /// [`Text`].
    Text,
    /// [`LenBytes`].
    LenBytes,
}

/// What the golden test can observe about a field type without parsing its name.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Shape {
    pub kind: ShapeKind,
    /// Native capacity: payload bytes for string types, element count for arrays and lists.
    pub cap: usize,
    /// For arrays and lists: kind of the element.
    pub elem_kind: ShapeKind,
    /// For arrays and lists of strings: capacity of one element.
    pub elem_cap: usize,
}

impl Shape {
    pub(crate) const SCALAR: Self = Self {
        kind: ShapeKind::Scalar,
        cap: 0,
        elem_kind: ShapeKind::Scalar,
        elem_cap: 0,
    };

    const fn string(kind: ShapeKind, cap: usize) -> Self {
        Self {
            kind,
            cap,
            elem_kind: ShapeKind::Scalar,
            elem_cap: 0,
        }
    }

    const fn collection(kind: ShapeKind, cap: usize, element: Self) -> Self {
        Self {
            kind,
            cap,
            elem_kind: element.kind,
            elem_cap: element.cap,
        }
    }
}

/// Implemented by every type that can be a field of a generated record.
pub(crate) trait NativeShape {
    const SHAPE: Shape;
}

macro_rules! scalar_shapes {
    ($($t:ty),* $(,)?) => { $( impl NativeShape for $t { const SHAPE: Shape = Shape::SCALAR; } )* };
}

scalar_shapes!(
    u8,
    u16,
    u32,
    u64,
    i8,
    i32,
    i64,
    bool,
    Float32,
    MacAddr,
    Interface,
    MediaType,
    ProfileType,
    FreqBand
);

impl<const W: u32> NativeShape for UBits<W> {
    const SHAPE: Shape = Shape::SCALAR;
}

impl<const CAP: usize> NativeShape for CStrBytes<CAP> {
    const SHAPE: Shape = Shape::string(ShapeKind::CStrBytes, CAP);
}

impl<const CAP: usize> NativeShape for Text<CAP> {
    const SHAPE: Shape = Shape::string(ShapeKind::Text, CAP);
}

impl<const CAP: usize> NativeShape for LenBytes<CAP> {
    const SHAPE: Shape = Shape::string(ShapeKind::LenBytes, CAP);
}

/// A credential wrapper has the shape of the byte string it wraps.
impl<S: NativeShape> NativeShape for Secret<S> {
    const SHAPE: Shape = S::SHAPE;
}

impl<T: NativeShape, const N: usize> NativeShape for [T; N] {
    const SHAPE: Shape = Shape::collection(ShapeKind::Array, N, T::SHAPE);
}

impl<T: NativeShape, const N: usize> NativeShape for BoundedVec<T, N> {
    const SHAPE: Shape = Shape::collection(ShapeKind::List, N, T::SHAPE);
}

/// How a string field is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StrKind {
    /// NUL-terminated byte string.
    CStr,
    /// UTF-8 text; the mapping carries the reason.
    Text,
    /// Length-delimited raw bytes with its `*_len` field folded in.
    LenBytes,
}

/// Wire length of a string field when no reader limits it.
pub(crate) const NO_WIRE: &str = "none at pin";

/// Inventory fields that one Rust field stands for, and the capacity facts of string fields.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Map {
    /// The main C field.
    pub field: &'static str,
    /// C count field folded into a [`BoundedVec`] length.
    pub count_field: Option<&'static str>,
    /// C length field folded into a [`LenBytes`] length.
    pub len_field: Option<&'static str>,
    pub string: Option<StrKind>,
    /// Wire length accepted by the consuming reader, with its upstream cite.
    pub wire: Option<&'static str>,
    /// Why upstream treats a [`StrKind::Text`] field as text, with a cite.
    pub text_reason: Option<&'static str>,
    /// Decision tag of an intentional native/C capacity difference.
    pub tag: Option<&'static str>,
    /// For a [`StrKind::LenBytes`] field: the reader that writes a NUL after the bytes, with its
    /// upstream cite. The C payload is then `N - 1`, not `N`.
    pub reader_nul: Option<&'static str>,
    /// For a pointer row: the cited constant that bounds the buffer.
    pub bound_cite: Option<&'static str>,
    /// A C member that is a one-field wrapper struct (`field` has the wrapper's type): the
    /// wrapper type and its only field, which the Rust field also stands for.
    pub cross_fold: Option<(&'static str, &'static str)>,
}

impl Map {
    /// One C field for one Rust field.
    pub(crate) const fn field(field: &'static str) -> Self {
        Self {
            field,
            count_field: None,
            len_field: None,
            string: None,
            wire: None,
            text_reason: None,
            tag: None,
            reader_nul: None,
            bound_cite: None,
            cross_fold: None,
        }
    }

    /// A C array and its count field, folded into one bounded list.
    pub(crate) const fn counted(count_field: &'static str, field: &'static str) -> Self {
        let mut map = Self::field(field);
        map.count_field = Some(count_field);
        map
    }

    pub(crate) const fn cstr(mut self) -> Self {
        self.string = Some(StrKind::CStr);
        self
    }

    pub(crate) const fn text(mut self, reason: &'static str) -> Self {
        self.string = Some(StrKind::Text);
        self.text_reason = Some(reason);
        self
    }

    pub(crate) const fn len_bytes(mut self, len_field: &'static str) -> Self {
        self.string = Some(StrKind::LenBytes);
        self.len_field = Some(len_field);
        self
    }

    pub(crate) const fn wire(mut self, wire: &'static str) -> Self {
        self.wire = Some(wire);
        self
    }

    /// The consumer's reader appends a NUL after the length-delimited bytes (cite `file:line`).
    pub(crate) const fn reader_nul(mut self, cite: &'static str) -> Self {
        self.reader_nul = Some(cite);
        self
    }

    /// The cited constant that bounds a pointer row's buffer.
    pub(crate) const fn bound_cite(mut self, cite: &'static str) -> Self {
        self.bound_cite = Some(cite);
        self
    }

    /// The C member is a wrapper struct whose single array field is folded into this Rust list.
    pub(crate) const fn cross_fold(
        mut self,
        wrapper_type: &'static str,
        wrapper_field: &'static str,
    ) -> Self {
        self.cross_fold = Some((wrapper_type, wrapper_field));
        self
    }

    pub(crate) const fn tag(mut self, tag: &'static str) -> Self {
        self.tag = Some(tag);
        self
    }

    /// Every inventory field this mapping accounts for.
    pub(crate) fn c_fields(&self) -> Vec<&'static str> {
        let mut fields = vec![self.field];
        fields.extend(self.count_field);
        fields.extend(self.len_field);
        fields
    }
}

/// One Rust field of a generated record.
#[derive(Debug, Clone, Copy)]
pub(crate) struct FieldMap {
    pub rust_field: &'static str,
    /// The declared type, as written in the source.
    pub rust_type: &'static str,
    pub map: Map,
    pub shape: Shape,
}

/// The generated field table of one record.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RecordTable {
    pub c_type: &'static str,
    pub rust_name: &'static str,
    pub is_copy: bool,
    pub size: usize,
    pub fields: &'static [FieldMap],
}

/// One C enumerator of a generated enum.
#[derive(Debug, Clone, Copy)]
pub(crate) struct EnumEntry {
    pub c_ident: &'static str,
    pub value: u32,
    /// Name of the associated const, for retained values.
    pub rust_const: Option<&'static str>,
    /// Reason a value is not retained as a named const (sentinels).
    pub not_retained: Option<&'static str>,
}

/// The generated table of one open enum.
#[derive(Debug, Clone, Copy)]
pub(crate) struct EnumTable {
    pub c_type: &'static str,
    pub rust_name: &'static str,
    /// Width of the wire integer in bits.
    pub bits: u32,
    pub entries: &'static [EnumEntry],
}
