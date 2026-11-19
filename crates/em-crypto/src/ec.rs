// SPDX-License-Identifier: Apache-2.0

//! Elliptic curves for DPP: P-256, P-384 and P-521 only (P-192 and P-224 are refused).
//!
//! - Points travel as fixed-width big-endian `X || Y` without a SEC1 prefix ([`Point`]); decoding
//!   checks that the point is on the curve.
//! - Bootstrapping keys are imported from and exported to `SubjectPublicKeyInfo` DER
//!   ([`PublicKey`]). A key keeps its point form and is exported in that form: the form it was
//!   imported with, or uncompressed for a key from [`PrivateKey::public_key`]. The boot-key hash
//!   is taken over that re-serialized DER, never over the raw input ([`boot_key_hash`]).
//! - Secret scalars ([`Scalar`], [`PrivateKey`]) are wiped on drop, are neither `Clone` nor
//!   `Copy`, and have a redacted `Debug`. This module wipes its own byte buffers and working
//!   copies of them; the curve crates' by-value copies are not wiped.
//! - Every secret scalar, including the ECDSA `k`, is drawn from a [`RandomSource`] by rejection
//!   sampling, so a failing source is an error. The curve crates' `random` constructors are not
//!   used: their generator interface (`RngCore::fill_bytes`) cannot report failure. Their
//!   `Signer` and `PrehashSigner` paths are not used either, because they derive `k` themselves
//!   (RFC 6979) instead of taking it from the source.
//! - ECDSA signs and verifies the SHA-256 of the message for every curve: the digest is padded
//!   with leading zeros to the field width and passed to the `ecdsa::hazmat` primitives. This
//!   matches OpenSSL's integer conversion of a short digest, and it works around `p521` 0.13
//!   refusing a 32-byte digest through its high-level API. Signatures are DER encoded.

use core::fmt;

use ecdsa::Signature;
use ecdsa::hazmat::{sign_prehashed, verify_prehashed};
use elliptic_curve::ff::{Field, PrimeField};
use elliptic_curve::sec1::{FromEncodedPoint, ModulusSize, ToEncodedPoint};
use elliptic_curve::{
    AffinePoint, CurveArithmetic, FieldBytes, FieldBytesSize, NonZeroScalar,
    PublicKey as CurvePublicKey, Scalar as CurveScalar,
};
use sha2::{Digest, Sha256};
use spki::der::asn1::{AnyRef, BitStringRef, ObjectIdentifier};
use spki::der::{Decode, Encode};
use spki::{AlgorithmIdentifierRef, SubjectPublicKeyInfoRef};
use zeroize::{ZeroizeOnDrop, Zeroizing};

use crate::error::Error;
use crate::rand::RandomSource;
use crate::secret::SecretBytes;

/// Draws before giving up on a source that never yields an acceptable scalar. A draw fails with
/// probability below 2^-31 on every supported curve.
const MAX_SAMPLING_ATTEMPTS: usize = 64;

/// `id-ecPublicKey`.
const EC_PUBLIC_KEY_OID: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.10045.2.1");
const P256_OID: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.10045.3.1.7");
const P384_OID: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.3.132.0.34");
const P521_OID: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.3.132.0.35");

const SEC1_UNCOMPRESSED: u8 = 0x04;
const SEC1_COMPRESSED_EVEN: u8 = 0x02;
const SEC1_COMPRESSED_ODD: u8 = 0x03;

/// Runs `$body` once per supported curve with `$c` aliased to that curve's marker type.
macro_rules! with_curve {
    ($curve:expr, $c:ident => $body:expr) => {
        match $curve {
            Curve::P256 => {
                type $c = p256::NistP256;
                $body
            }
            Curve::P384 => {
                type $c = p384::NistP384;
                $body
            }
            Curve::P521 => {
                type $c = p521::NistP521;
                $body
            }
        }
    };
}

/// A supported curve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Curve {
    /// NIST P-256 (secp256r1, prime256v1).
    P256,
    /// NIST P-384 (secp384r1).
    P384,
    /// NIST P-521 (secp521r1).
    P521,
}

impl Curve {
    /// Width of a coordinate or scalar in bytes: 32, 48 or 66.
    #[must_use]
    pub const fn coordinate_len(self) -> usize {
        match self {
            Curve::P256 => 32,
            Curve::P384 => 48,
            Curve::P521 => 66,
        }
    }

    /// Width of an `X || Y` point in bytes.
    #[must_use]
    pub const fn point_len(self) -> usize {
        2 * self.coordinate_len()
    }

    const fn oid(self) -> ObjectIdentifier {
        match self {
            Curve::P256 => P256_OID,
            Curve::P384 => P384_OID,
            Curve::P521 => P521_OID,
        }
    }

    fn from_oid(oid: ObjectIdentifier) -> Result<Self, Error> {
        if oid == P256_OID {
            Ok(Curve::P256)
        } else if oid == P384_OID {
            Ok(Curve::P384)
        } else if oid == P521_OID {
            Ok(Curve::P521)
        } else {
            Err(Error::UnsupportedCurve)
        }
    }

    /// Mask for the first byte of a random scalar candidate. P-521 scalars have 521 bits in a
    /// 66-byte representation, so the top 7 bits are cleared to keep the rejection rate low.
    const fn top_byte_mask(self) -> u8 {
        match self {
            Curve::P521 => 0x01,
            Curve::P256 | Curve::P384 => 0xff,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Helpers over the curve crates' generic types.

/// Big-endian `bytes` (at most one field width) left-padded with zeros to the field width.
///
/// The array is wiped when it drops. Callers move the bytes out with `mem::take` for `from_repr`,
/// which takes them by value; that argument is the curve crate's and is not wiped.
fn padded_repr<C: CurveArithmetic>(bytes: &[u8]) -> Zeroizing<FieldBytes<C>> {
    let mut repr = Zeroizing::new(FieldBytes::<C>::default());
    let start = repr.len() - bytes.len();
    repr[start..].copy_from_slice(bytes);
    repr
}

/// The curve crate's public key for a point (checks again that it is on the curve).
fn curve_public_key<C>(point: &Point) -> Result<CurvePublicKey<C>, Error>
where
    C: CurveArithmetic,
    AffinePoint<C>: FromEncodedPoint<C> + ToEncodedPoint<C>,
    FieldBytesSize<C>: ModulusSize,
{
    CurvePublicKey::<C>::from_sec1_bytes(&point.sec1()).map_err(|_| Error::InvalidPoint)
}

/// Parses SEC1 bytes in any supported form (compressed or uncompressed) into a [`Point`].
fn point_from_sec1<C>(curve: Curve, sec1: &[u8]) -> Result<Point, Error>
where
    C: CurveArithmetic,
    AffinePoint<C>: FromEncodedPoint<C> + ToEncodedPoint<C>,
    FieldBytesSize<C>: ModulusSize,
{
    let key = CurvePublicKey::<C>::from_sec1_bytes(sec1).map_err(|_| Error::InvalidPoint)?;
    Ok(point_from_curve_key(curve, &key))
}

fn point_from_curve_key<C>(curve: Curve, key: &CurvePublicKey<C>) -> Point
where
    C: CurveArithmetic,
    AffinePoint<C>: FromEncodedPoint<C> + ToEncodedPoint<C>,
    FieldBytesSize<C>: ModulusSize,
{
    let uncompressed = key.to_encoded_point(false);
    // Drop the SEC1 prefix byte.
    Point {
        curve,
        xy: uncompressed.as_bytes()[1..].to_vec(),
    }
}

/// A uniformly random nonzero scalar below the group order, by rejection sampling.
fn sample_nonzero_scalar<C: CurveArithmetic>(
    top_byte_mask: u8,
    source: &mut impl RandomSource,
) -> Result<Zeroizing<CurveScalar<C>>, Error> {
    for _ in 0..MAX_SAMPLING_ATTEMPTS {
        let mut repr = Zeroizing::new(FieldBytes::<C>::default());
        source.fill(&mut repr)?;
        repr[0] &= top_byte_mask;
        let candidate = Option::<CurveScalar<C>>::from(CurveScalar::<C>::from_repr(
            core::mem::take(&mut *repr),
        ));
        if let Some(scalar) = candidate {
            let scalar = Zeroizing::new(scalar);
            if !bool::from(scalar.is_zero()) {
                return Ok(scalar);
            }
        }
    }
    Err(Error::SamplingExhausted)
}

/// SHA-256 of `message`, left-padded with zeros to the field width (see the module docs).
fn prehash<C: CurveArithmetic>(message: &[u8]) -> FieldBytes<C> {
    // The digest of a message is not secret, so the unwiped copy is fine.
    core::mem::take(&mut *padded_repr::<C>(&Sha256::digest(message)))
}

// ---------------------------------------------------------------------------------------------
// Points

/// A point on the curve, other than the point at infinity.
#[derive(Debug, PartialEq, Eq)]
pub struct Point {
    curve: Curve,
    /// `X || Y`, `curve.point_len()` bytes.
    xy: Vec<u8>,
}

impl Point {
    /// Decodes fixed-width big-endian `X || Y` (no SEC1 prefix).
    ///
    /// # Errors
    /// [`Error::InvalidPoint`] for a wrong length or a point that is not on the curve.
    pub fn from_xy(curve: Curve, xy: &[u8]) -> Result<Self, Error> {
        if xy.len() != curve.point_len() {
            return Err(Error::InvalidPoint);
        }
        let mut sec1 = Vec::with_capacity(1 + xy.len());
        sec1.push(SEC1_UNCOMPRESSED);
        sec1.extend_from_slice(xy);
        with_curve!(curve, C => point_from_sec1::<C>(curve, &sec1))
    }

    /// The curve of this point.
    #[must_use]
    pub fn curve(&self) -> Curve {
        self.curve
    }

    /// The point as fixed-width big-endian `X || Y`.
    #[must_use]
    pub fn as_xy(&self) -> &[u8] {
        &self.xy
    }

    fn sec1(&self) -> Vec<u8> {
        let mut sec1 = Vec::with_capacity(1 + self.xy.len());
        sec1.push(SEC1_UNCOMPRESSED);
        sec1.extend_from_slice(&self.xy);
        sec1
    }

    /// `self + other`.
    ///
    /// # Errors
    /// [`Error::CurveMismatch`] for points of different curves, [`Error::InvalidPoint`] when the
    /// sum is the point at infinity.
    pub fn add(&self, other: &Point) -> Result<Point, Error> {
        if self.curve != other.curve {
            return Err(Error::CurveMismatch);
        }
        with_curve!(self.curve, C => {
            let left = curve_public_key::<C>(self)?;
            let right = curve_public_key::<C>(other)?;
            let sum = (left.to_projective() + right.to_projective()).to_affine();
            let sum = CurvePublicKey::<C>::from_affine(sum).map_err(|_| Error::InvalidPoint)?;
            Ok(point_from_curve_key(self.curve, &sum))
        })
    }

    /// `self * scalar`, in constant time.
    ///
    /// # Errors
    /// [`Error::CurveMismatch`] for a scalar of another curve, [`Error::InvalidPoint`] when the
    /// product is the point at infinity (a zero scalar).
    pub fn multiply(&self, scalar: &Scalar) -> Result<Point, Error> {
        if self.curve != scalar.curve {
            return Err(Error::CurveMismatch);
        }
        with_curve!(self.curve, C => {
            let point = curve_public_key::<C>(self)?;
            let factor = scalar.to_curve_scalar::<C>()?;
            let product = (point.to_projective() * *factor).to_affine();
            let product =
                CurvePublicKey::<C>::from_affine(product).map_err(|_| Error::InvalidPoint)?;
            Ok(point_from_curve_key(self.curve, &product))
        })
    }
}

// ---------------------------------------------------------------------------------------------
// Scalars and private keys

/// A secret scalar below the group order (zero is allowed), kept as `coordinate_len` big-endian
/// bytes. Wiped on drop; no `Clone`; `Debug` is redacted.
pub struct Scalar {
    curve: Curve,
    bytes: SecretBytes,
}

impl fmt::Debug for Scalar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Scalar(<redacted>)")
    }
}

// The wrapped `SecretBytes` wipes the scalar when this drops.
impl ZeroizeOnDrop for Scalar {}

impl Scalar {
    /// Imports 1 to `coordinate_len` big-endian bytes.
    ///
    /// # Errors
    /// [`Error::InvalidScalar`] for a wrong length or a value that is not below the group order.
    pub fn from_be_bytes(curve: Curve, bytes: &[u8]) -> Result<Self, Error> {
        if bytes.is_empty() || bytes.len() > curve.coordinate_len() {
            return Err(Error::InvalidScalar);
        }
        with_curve!(curve, C => {
            let mut repr = padded_repr::<C>(bytes);
            let scalar = Option::<CurveScalar<C>>::from(CurveScalar::<C>::from_repr(core::mem::take(&mut *repr)))
                .ok_or(Error::InvalidScalar)?;
            Ok(Self::from_curve_scalar::<C>(curve, &Zeroizing::new(scalar)))
        })
    }

    fn from_curve_scalar<C: CurveArithmetic>(curve: Curve, scalar: &CurveScalar<C>) -> Self {
        let repr = Zeroizing::new(scalar.to_repr());
        Self {
            curve,
            bytes: SecretBytes::from_slice(&repr),
        }
    }

    fn to_curve_scalar<C: CurveArithmetic>(&self) -> Result<Zeroizing<CurveScalar<C>>, Error> {
        let mut repr = padded_repr::<C>(self.bytes.expose());
        let scalar = Option::<CurveScalar<C>>::from(CurveScalar::<C>::from_repr(core::mem::take(
            &mut *repr,
        )))
        .ok_or(Error::InvalidScalar)?;
        Ok(Zeroizing::new(scalar))
    }

    /// The curve of this scalar.
    #[must_use]
    pub fn curve(&self) -> Curve {
        self.curve
    }

    /// The scalar as `coordinate_len` big-endian bytes (copied into a secret).
    #[must_use]
    pub fn to_be_bytes(&self) -> SecretBytes {
        SecretBytes::from_slice(self.bytes.expose())
    }

    /// `self + other` modulo the group order.
    ///
    /// # Errors
    /// [`Error::CurveMismatch`] for scalars of different curves.
    pub fn add(&self, other: &Scalar) -> Result<Scalar, Error> {
        if self.curve != other.curve {
            return Err(Error::CurveMismatch);
        }
        with_curve!(self.curve, C => {
            let left = self.to_curve_scalar::<C>()?;
            let right = other.to_curve_scalar::<C>()?;
            let sum = Zeroizing::new(*left + *right);
            Ok(Self::from_curve_scalar::<C>(self.curve, &sum))
        })
    }
}

/// An EC private key: a scalar in `1 ..= n - 1`. Wiped on drop; no `Clone`; `Debug` is redacted.
#[derive(Debug)]
pub struct PrivateKey {
    scalar: Scalar,
}

impl ZeroizeOnDrop for PrivateKey {}

impl PrivateKey {
    /// Draws a private key from `source` by rejection sampling.
    ///
    /// # Errors
    /// [`Error::Random`] when the source fails, [`Error::SamplingExhausted`] when no draw is
    /// acceptable (a broken source).
    pub fn generate(curve: Curve, source: &mut impl RandomSource) -> Result<Self, Error> {
        let scalar = with_curve!(curve, C => {
            let sampled = sample_nonzero_scalar::<C>(curve.top_byte_mask(), source)?;
            Scalar::from_curve_scalar::<C>(curve, &sampled)
        });
        Ok(Self { scalar })
    }

    /// Imports 1 to `coordinate_len` big-endian bytes.
    ///
    /// # Errors
    /// [`Error::InvalidScalar`] for a wrong length, zero, or a value not below the group order.
    pub fn from_be_bytes(curve: Curve, bytes: &[u8]) -> Result<Self, Error> {
        let scalar = Scalar::from_be_bytes(curve, bytes)?;
        // Constant-time zero test: scanning the bytes would stop at the first nonzero one.
        let is_zero = with_curve!(scalar.curve, C => {
            let curve_scalar = scalar.to_curve_scalar::<C>()?;
            bool::from(curve_scalar.is_zero())
        });
        if is_zero {
            return Err(Error::InvalidScalar);
        }
        Ok(Self { scalar })
    }

    /// The curve of this key.
    #[must_use]
    pub fn curve(&self) -> Curve {
        self.scalar.curve
    }

    /// The private scalar, for scalar arithmetic such as DPP's `bR + pR`.
    #[must_use]
    pub fn scalar(&self) -> &Scalar {
        &self.scalar
    }

    /// The public key, with an uncompressed point (the form OpenSSL generates).
    ///
    /// # Errors
    /// [`Error::InvalidScalar`] only if the stored scalar is corrupt, which cannot happen for a
    /// key made by this module.
    pub fn public_key(&self) -> Result<PublicKey, Error> {
        let curve = self.scalar.curve;
        let point = with_curve!(curve, C => {
            let scalar = self.scalar.to_curve_scalar::<C>()?;
            let nonzero = Zeroizing::new(
                Option::<NonZeroScalar<C>>::from(NonZeroScalar::<C>::new(*scalar))
                    .ok_or(Error::InvalidScalar)?,
            );
            let key = CurvePublicKey::<C>::from_secret_scalar(&nonzero);
            point_from_curve_key(curve, &key)
        });
        Ok(PublicKey::new(point, PointForm::Uncompressed))
    }

    /// The ECDH shared secret: the x-coordinate of `self * peer` as `coordinate_len` bytes
    /// (leading zeros kept).
    ///
    /// # Errors
    /// [`Error::CurveMismatch`] when `peer` is on another curve. [`Error::InvalidScalar`] or
    /// [`Error::InvalidPoint`] only for a corrupt stored value, which this module cannot create.
    pub fn ecdh(&self, peer: &Point) -> Result<SecretBytes, Error> {
        let curve = self.scalar.curve;
        if curve != peer.curve {
            return Err(Error::CurveMismatch);
        }
        with_curve!(curve, C => {
            let scalar = self.scalar.to_curve_scalar::<C>()?;
            let nonzero = Zeroizing::new(
                Option::<NonZeroScalar<C>>::from(NonZeroScalar::<C>::new(*scalar))
                    .ok_or(Error::InvalidScalar)?,
            );
            let peer_key = curve_public_key::<C>(peer)?;
            let shared = elliptic_curve::ecdh::diffie_hellman(&*nonzero, peer_key.as_affine());
            Ok(SecretBytes::from_slice(shared.raw_secret_bytes()))
        })
    }

    /// ECDSA signature over SHA-256 of `message`, DER encoded. `k` comes from `source`.
    ///
    /// # Errors
    /// [`Error::Random`] when the source fails, [`Error::SamplingExhausted`] when no `k` is
    /// acceptable (a broken source).
    pub fn sign(&self, message: &[u8], source: &mut impl RandomSource) -> Result<Vec<u8>, Error> {
        let curve = self.scalar.curve;
        with_curve!(curve, C => {
            let private = self.scalar.to_curve_scalar::<C>()?;
            let z = prehash::<C>(message);
            for _ in 0..MAX_SAMPLING_ATTEMPTS {
                // The curve crate's scalar is `Copy`, so the copy handed to `sign_prehashed`
                // is not wiped by this crate; `k` is wiped when it drops.
                let k = sample_nonzero_scalar::<C>(curve.top_byte_mask(), source)?;
                // r or s came out zero: draw another `k`.
                if let Ok((signature, _)) = sign_prehashed::<C, CurveScalar<C>>(&private, *k, &z) {
                    return Ok(signature.to_der().as_bytes().to_vec());
                }
            }
            Err(Error::SamplingExhausted)
        })
    }
}

// ---------------------------------------------------------------------------------------------
// Public keys

/// How a public key's point is serialized in `SubjectPublicKeyInfo`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointForm {
    /// SEC1 `04 || X || Y`.
    Uncompressed,
    /// SEC1 `02 || X` or `03 || X`.
    Compressed,
}

/// A public key and the point form it is exported with.
#[derive(Debug, PartialEq, Eq)]
pub struct PublicKey {
    point: Point,
    form: PointForm,
}

impl PublicKey {
    /// A key for `point` that is exported in `form`.
    #[must_use]
    pub fn new(point: Point, form: PointForm) -> Self {
        Self { point, form }
    }

    /// The curve of this key.
    #[must_use]
    pub fn curve(&self) -> Curve {
        self.point.curve
    }

    /// The point.
    #[must_use]
    pub fn point(&self) -> &Point {
        &self.point
    }

    /// The point form that [`PublicKey::to_spki_der`] exports.
    #[must_use]
    pub fn form(&self) -> PointForm {
        self.form
    }

    /// Imports a `SubjectPublicKeyInfo` DER holding an EC public key on a named curve. Compressed
    /// and uncompressed points are accepted; the form is remembered.
    ///
    /// # Errors
    /// [`Error::InvalidSpki`] for malformed DER (including trailing data) or a non-EC key,
    /// [`Error::UnsupportedCurve`] for a curve other than P-256, P-384 or P-521 (such as P-192
    /// or P-224), [`Error::InvalidPoint`] for a point that is malformed or not on the curve.
    pub fn from_spki_der(der: &[u8]) -> Result<Self, Error> {
        let info = SubjectPublicKeyInfoRef::from_der(der).map_err(|_| Error::InvalidSpki)?;
        if info.algorithm.oid != EC_PUBLIC_KEY_OID {
            return Err(Error::InvalidSpki);
        }
        let curve_oid = info
            .algorithm
            .parameters_oid()
            .map_err(|_| Error::InvalidSpki)?;
        let curve = Curve::from_oid(curve_oid)?;
        let sec1 = info
            .subject_public_key
            .as_bytes()
            .ok_or(Error::InvalidSpki)?;
        let form = match sec1.first() {
            Some(&SEC1_UNCOMPRESSED) => PointForm::Uncompressed,
            Some(&(SEC1_COMPRESSED_EVEN | SEC1_COMPRESSED_ODD)) => PointForm::Compressed,
            _ => return Err(Error::InvalidPoint),
        };
        let point = with_curve!(curve, C => point_from_sec1::<C>(curve, sec1))?;
        Ok(Self { point, form })
    }

    /// Exports the `SubjectPublicKeyInfo` DER, with the point in this key's form.
    ///
    /// # Errors
    /// [`Error::InvalidPoint`] or [`Error::InvalidSpki`] only if the stored point is corrupt,
    /// which cannot happen for a key made by this module.
    pub fn to_spki_der(&self) -> Result<Vec<u8>, Error> {
        let compressed = self.form == PointForm::Compressed;
        let curve = self.point.curve;
        let sec1 = with_curve!(curve, C => {
            let key = curve_public_key::<C>(&self.point)?;
            key.to_encoded_point(compressed).as_bytes().to_vec()
        });
        let curve_oid = curve.oid();
        let info = SubjectPublicKeyInfoRef {
            algorithm: AlgorithmIdentifierRef {
                oid: EC_PUBLIC_KEY_OID,
                parameters: Some(AnyRef::from(&curve_oid)),
            },
            subject_public_key: BitStringRef::from_bytes(&sec1).map_err(|_| Error::InvalidSpki)?,
        };
        info.to_der().map_err(|_| Error::InvalidSpki)
    }

    /// The DPP boot-key hash: SHA-256 over `prefix` (without NUL, may be absent) followed by the
    /// re-serialized `SubjectPublicKeyInfo` DER of this key.
    ///
    /// # Errors
    /// [`Error::InvalidLabel`] for a prefix with a NUL byte, and the errors of
    /// [`PublicKey::to_spki_der`].
    pub fn boot_key_hash(&self, prefix: Option<&str>) -> Result<[u8; 32], Error> {
        let prefix = prefix.unwrap_or_default();
        if prefix.contains('\0') {
            return Err(Error::InvalidLabel);
        }
        let der = self.to_spki_der()?;
        let mut hasher = Sha256::new();
        hasher.update(prefix.as_bytes());
        hasher.update(&der);
        Ok(hasher.finalize().into())
    }

    /// Verifies a DER ECDSA signature over SHA-256 of `message`.
    ///
    /// # Errors
    /// [`Error::InvalidSignature`] for a malformed or non-verifying signature.
    pub fn verify(&self, message: &[u8], signature_der: &[u8]) -> Result<(), Error> {
        with_curve!(self.point.curve, C => {
            let key = curve_public_key::<C>(&self.point)?;
            let signature =
                Signature::<C>::from_der(signature_der).map_err(|_| Error::InvalidSignature)?;
            let z = prehash::<C>(message);
            verify_prehashed::<C>(&key.to_projective(), &z, &signature)
                .map_err(|_| Error::InvalidSignature)
        })
    }
}

/// The DPP boot-key hash of a `SubjectPublicKeyInfo` DER: the key is imported, re-serialized in
/// its own point form, and `SHA-256(prefix || re-serialized DER)` is returned. The input bytes
/// themselves are never hashed.
///
/// # Errors
/// The errors of [`PublicKey::from_spki_der`] and [`PublicKey::boot_key_hash`].
pub fn boot_key_hash(spki_der: &[u8], prefix: Option<&str>) -> Result<[u8; 32], Error> {
    PublicKey::from_spki_der(spki_der)?.boot_key_hash(prefix)
}
