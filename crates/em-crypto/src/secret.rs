// SPDX-License-Identifier: Apache-2.0

//! Zeroizing secret containers.
//!
//! Neither type implements `Clone`, `Copy`, `PartialEq` or `Display`, and `Debug` prints no
//! bytes. Read the bytes with `expose`; compare secrets with [`crate::ct::eq`].

use core::fmt;

use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

use crate::error::Error;

/// Secret bytes of a length known at run time. The memory is wiped on drop.
pub struct SecretBytes(Zeroizing<Vec<u8>>);

impl SecretBytes {
    /// Takes ownership of `bytes`.
    #[must_use]
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(Zeroizing::new(bytes))
    }

    /// Copies `bytes` into a new secret. The caller still owns (and must wipe) the source.
    #[must_use]
    pub fn from_slice(bytes: &[u8]) -> Self {
        Self::new(bytes.to_vec())
    }

    /// Creates `len` zero bytes, to be filled in place.
    #[must_use]
    pub fn zeroed(len: usize) -> Self {
        Self::new(vec![0; len])
    }

    /// The secret bytes.
    #[must_use]
    pub fn expose(&self) -> &[u8] {
        &self.0
    }

    /// The secret bytes, for in-place filling.
    pub fn expose_mut(&mut self) -> &mut [u8] {
        &mut self.0
    }

    /// The backing vector, for APIs that work on a `Vec` in place.
    pub(crate) fn as_vec_mut(&mut self) -> &mut Vec<u8> {
        &mut self.0
    }

    /// Shortens the secret to `len` bytes; the cut-off bytes are wiped when the secret drops.
    pub fn truncate(&mut self, len: usize) {
        self.0.truncate(len);
    }

    /// Number of bytes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// True when there are no bytes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for SecretBytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretBytes(<redacted>)")
    }
}

impl Zeroize for SecretBytes {
    fn zeroize(&mut self) {
        self.0.zeroize();
    }
}

impl ZeroizeOnDrop for SecretBytes {}

/// Secret bytes of a fixed length `N`. The memory is wiped on drop.
pub struct SecretArray<const N: usize>(Zeroizing<[u8; N]>);

impl<const N: usize> SecretArray<N> {
    /// `N` zero bytes, to be filled in place.
    #[must_use]
    pub fn zeroed() -> Self {
        Self(Zeroizing::new([0; N]))
    }

    /// Copies exactly `N` bytes. The caller still owns (and must wipe) the source.
    ///
    /// # Errors
    /// [`Error::InvalidInputLength`] when `bytes.len() != N`.
    pub fn from_slice(bytes: &[u8]) -> Result<Self, Error> {
        let mut secret = Self::zeroed();
        if bytes.len() != N {
            return Err(Error::InvalidInputLength);
        }
        secret.expose_mut().copy_from_slice(bytes);
        Ok(secret)
    }

    /// The secret bytes.
    #[must_use]
    pub fn expose(&self) -> &[u8; N] {
        &self.0
    }

    /// The secret bytes, for in-place filling.
    pub fn expose_mut(&mut self) -> &mut [u8; N] {
        &mut self.0
    }
}

impl<const N: usize> fmt::Debug for SecretArray<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SecretArray<{N}>(<redacted>)")
    }
}

impl<const N: usize> Zeroize for SecretArray<N> {
    fn zeroize(&mut self) {
        self.0.zeroize();
    }
}

impl<const N: usize> ZeroizeOnDrop for SecretArray<N> {}
