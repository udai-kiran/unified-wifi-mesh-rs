// SPDX-License-Identifier: Apache-2.0

//! Credential wrapper whose `Debug` output never shows the content.
//!
//! Passphrases, `client_id` and `client_secret` live in records that are logged with `{:?}`.
//! Wrapping the byte string in [`Secret`] keeps the bytes (and even their length) out of that
//! output; the content is available only through [`Secret::expose`].

use std::fmt;

/// A credential value. `Debug` prints `Secret(<redacted>)`.
///
/// `Secret<S>` is `Copy` exactly when `S` is, and never `Clone` otherwise. Equality compares
/// the content and is not constant-time: use it for data-model comparisons, not for
/// authentication.
#[derive(PartialEq, Eq, Hash, Default)]
pub struct Secret<S>(S);

impl<S> Secret<S> {
    /// Wraps `inner`.
    #[must_use]
    pub const fn new(inner: S) -> Self {
        Self(inner)
    }

    /// The wrapped value; the only way to read the credential.
    #[must_use]
    pub const fn expose(&self) -> &S {
        &self.0
    }
}

#[allow(
    clippy::expl_impl_clone_on_copy,
    reason = "Clone exists only when the wrapped value is Copy, which a derive cannot express"
)]
impl<S: Copy> Clone for Secret<S> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<S: Copy> Copy for Secret<S> {}

impl<S> fmt::Debug for Secret<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

#[cfg(test)]
mod tests {
    use super::Secret;
    use crate::{CStrBytes, LenBytes};

    #[test]
    fn debug_hides_content_and_length() {
        let pass = Secret::new(CStrBytes::<63>::new(b"hunter2-passphrase").unwrap());
        let shown = format!("{pass:?}");
        assert_eq!(shown, "Secret(<redacted>)");
        assert_eq!(pass.expose().as_bytes(), b"hunter2-passphrase");

        let raw = Secret::new(LenBytes::<255>::new(&[0xde, 0xad, 0xbe, 0xef]).unwrap());
        assert_eq!(format!("{raw:?}"), "Secret(<redacted>)");
        assert_eq!(format!("{raw:#?}"), "Secret(<redacted>)");
    }

    #[test]
    fn equality_compares_content() {
        let a = Secret::new(CStrBytes::<8>::new(b"ab").unwrap());
        let b = Secret::new(CStrBytes::<8>::new(b"ab").unwrap());
        let c = Secret::new(CStrBytes::<8>::new(b"ac").unwrap());
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn small_secrets_are_copy() {
        fn needs_copy<T: Copy>() {}
        needs_copy::<Secret<CStrBytes<127>>>();
        needs_copy::<Secret<LenBytes<255>>>();
    }
}
