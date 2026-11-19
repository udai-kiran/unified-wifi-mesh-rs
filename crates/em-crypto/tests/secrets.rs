// SPDX-License-Identifier: Apache-2.0

//! Secret types: no `Clone`/`Copy`, redacted `Debug`, wiped on drop.

use core::marker::PhantomData;

use em_crypto::dh::{DhPrivate, DhPublic};
use em_crypto::ec::{Curve, Point, PrivateKey, PublicKey, Scalar};
use em_crypto::{Nonce, SecretArray, SecretBytes};
use zeroize::{Zeroize, ZeroizeOnDrop};

// Compile-time trait probe: `Probe::<T>::IS_CLONE` is true only when `T: Clone`. The inherent
// constant (available only under the bound) wins over the blanket trait constant (the
// fallback), so the compiler decides the answer; `black_box` only keeps clippy from folding it.
trait NotClone {
    const IS_CLONE: bool = false;
}
impl<T> NotClone for T {}
trait NotCopy {
    const IS_COPY: bool = false;
}
impl<T> NotCopy for T {}
struct Probe<T>(PhantomData<T>);
impl<T: Clone> Probe<T> {
    const IS_CLONE: bool = true;
}
impl<T: Copy> Probe<T> {
    const IS_COPY: bool = true;
}

macro_rules! is_clone {
    ($t:ty) => {
        std::hint::black_box(Probe::<$t>::IS_CLONE)
    };
}
macro_rules! is_copy {
    ($t:ty) => {
        std::hint::black_box(Probe::<$t>::IS_COPY)
    };
}

#[test]
fn probe_detects_clone_and_copy() {
    // Positive controls: the probe must be able to answer "yes".
    assert!(is_clone!(String));
    assert!(!is_copy!(String));
    assert!(is_copy!(u8) && is_clone!(u8));
}

#[test]
fn secret_types_are_neither_clone_nor_copy() {
    assert!(!is_clone!(SecretBytes) && !is_copy!(SecretBytes));
    assert!(!is_clone!(SecretArray<16>) && !is_copy!(SecretArray<16>));
    assert!(!is_clone!(SecretArray<192>) && !is_copy!(SecretArray<192>));
    assert!(!is_clone!(DhPrivate) && !is_copy!(DhPrivate));
    assert!(!is_clone!(Scalar) && !is_copy!(Scalar));
    assert!(!is_clone!(PrivateKey) && !is_copy!(PrivateKey));
}

#[test]
fn ec_public_values_are_not_cloned_either() {
    // Public EC values own heap data, so they are neither `Copy` nor `Clone` (no-clone rule).
    assert!(!is_clone!(Point) && !is_clone!(PublicKey));
    assert!(is_copy!(Curve));
}

#[test]
fn public_values_are_plain_data() {
    assert!(is_copy!(DhPublic) && is_copy!(Nonce));
}

fn assert_zeroize_on_drop<T: ZeroizeOnDrop>() {}

#[test]
fn secret_types_wipe_on_drop() {
    assert_zeroize_on_drop::<SecretBytes>();
    assert_zeroize_on_drop::<SecretArray<32>>();
    assert_zeroize_on_drop::<DhPrivate>();
    assert_zeroize_on_drop::<Scalar>();
    assert_zeroize_on_drop::<PrivateKey>();
}

#[test]
fn zeroize_wipes_contents() {
    let mut array = SecretArray::<4>::from_slice(&[1, 2, 3, 4]).expect("length");
    array.zeroize();
    assert_eq!(array.expose(), &[0; 4]);

    let mut bytes = SecretBytes::from_slice(&[9, 9, 9]);
    bytes.zeroize();
    assert!(bytes.expose().iter().all(|&b| b == 0));
}

#[test]
fn debug_hides_the_bytes() {
    let bytes = SecretBytes::from_slice(&[0xa5, 0xa5, 0xa5]);
    let array = SecretArray::<3>::from_slice(&[0xa5, 0xa5, 0xa5]).expect("length");
    let private = DhPrivate::from_be_bytes(&[0xa5]).expect("exponent");

    assert_eq!(format!("{bytes:?}"), "SecretBytes(<redacted>)");
    assert_eq!(format!("{array:?}"), "SecretArray<3>(<redacted>)");
    assert_eq!(format!("{private:?}"), "DhPrivate(<redacted>)");
    // 0xa5 = 165; the scalar prints neither as hex nor as decimal.
    let scalar = Scalar::from_be_bytes(Curve::P256, &[0xa5]).expect("scalar");
    let key = PrivateKey::from_be_bytes(Curve::P256, &[0xa5]).expect("key");
    assert_eq!(format!("{scalar:?}"), "Scalar(<redacted>)");
    assert_eq!(
        format!("{key:?}"),
        "PrivateKey { scalar: Scalar(<redacted>) }"
    );
    for text in [
        format!("{bytes:#?}"),
        format!("{array:#?}"),
        format!("{private:#?}"),
        format!("{scalar:#?}"),
        format!("{key:#?}"),
    ] {
        for needle in ["a5", "A5", "165"] {
            assert!(!text.contains(needle), "{text} leaks {needle}");
        }
    }
}

#[test]
fn array_length_is_checked() {
    assert!(SecretArray::<4>::from_slice(&[1, 2, 3]).is_err());
    assert!(SecretArray::<4>::from_slice(&[1, 2, 3, 4, 5]).is_err());
}
