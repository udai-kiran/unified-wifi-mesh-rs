// SPDX-License-Identifier: Apache-2.0

//! EC checks beyond the oracle rows (those are in `fixtures.rs`): P-521, generated keys, ECDSA on
//! all curves, SPKI and point rejection, compressed keys, the boot-key hash, arithmetic, and
//! random-source failures.

mod common;

use common::row;
use em_crypto::ec::{Curve, Point, PointForm, PrivateKey, PublicKey, Scalar};
use em_crypto::rand::{FailingRandom, OsRandom};
use em_crypto::{Error, RandomError, RandomSource};

const CURVES: [Curve; 3] = [Curve::P256, Curve::P384, Curve::P521];

/// Replays fixed bytes, repeating them forever.
struct Repeating(Vec<u8>);

impl RandomSource for Repeating {
    fn fill(&mut self, dest: &mut [u8]) -> Result<(), RandomError> {
        for (index, byte) in dest.iter_mut().enumerate() {
            *byte = self.0[index % self.0.len()];
        }
        Ok(())
    }
}

/// Serves one prepared draw per `fill` call, then fails.
struct Draws(Vec<Vec<u8>>);

impl RandomSource for Draws {
    fn fill(&mut self, dest: &mut [u8]) -> Result<(), RandomError> {
        if self.0.is_empty() {
            return Err(RandomError);
        }
        let draw = self.0.remove(0);
        dest.copy_from_slice(&draw);
        Ok(())
    }
}

/// `value` as a big-endian number of `len` bytes.
fn be(len: usize, value: u8) -> Vec<u8> {
    let mut bytes = vec![0; len];
    bytes[len - 1] = value;
    bytes
}

fn generated(curve: Curve) -> (PrivateKey, PublicKey) {
    let private = PrivateKey::generate(curve, &mut OsRandom).expect("keygen");
    let public = private.public_key().expect("public key");
    (private, public)
}

fn flip_last_byte(bytes: &[u8]) -> Vec<u8> {
    let mut changed = bytes.to_vec();
    *changed.last_mut().expect("non-empty") ^= 1;
    changed
}

#[test]
fn p521_sign_verify_ecdh_and_encoding() {
    let curve = Curve::P521;
    assert_eq!((curve.coordinate_len(), curve.point_len()), (66, 132));
    let (alice, alice_public) = generated(curve);
    let (bob, bob_public) = generated(curve);

    // Signing and verification (SHA-256 digest padded to 66 bytes).
    let message = b"eyJ0eXAiOiJkcHBDb24ifQ.eyJzdWIiOiJwNTIxIn0";
    let signature = alice.sign(message, &mut OsRandom).expect("sign");
    alice_public.verify(message, &signature).expect("verify");
    assert_eq!(
        bob_public.verify(message, &signature),
        Err(Error::InvalidSignature)
    );

    // ECDH agrees from both sides and is a fixed 66-byte x-coordinate.
    let alice_secret = alice.ecdh(bob_public.point()).expect("ecdh");
    let bob_secret = bob.ecdh(alice_public.point()).expect("ecdh");
    assert_eq!(alice_secret.expose().len(), 66);
    assert_eq!(alice_secret.expose(), bob_secret.expose());

    // Encoding: X || Y is 132 bytes, round trips, and the SPKI is 158 bytes.
    let xy = alice_public.point().as_xy();
    assert_eq!(xy.len(), 132);
    assert_eq!(Point::from_xy(curve, xy).expect("decode").as_xy(), xy);
    let der = alice_public.to_spki_der().expect("export");
    assert_eq!(der.len(), 158);
    assert_eq!(
        PublicKey::from_spki_der(&der).expect("import"),
        alice_public
    );
}

#[test]
fn rust_signatures_verify_and_changes_are_rejected_on_every_curve() {
    let message = b"header.payload";
    for curve in CURVES {
        let (private, public) = generated(curve);
        let (_, other_public) = generated(curve);
        let signature = private.sign(message, &mut OsRandom).expect("sign");

        public.verify(message, &signature).expect("verify");
        assert_eq!(
            public.verify(b"header.payloae", &signature),
            Err(Error::InvalidSignature),
            "{curve:?}: changed message"
        );
        assert_eq!(
            public.verify(message, &flip_last_byte(&signature)),
            Err(Error::InvalidSignature),
            "{curve:?}: changed signature"
        );
        assert_eq!(
            other_public.verify(message, &signature),
            Err(Error::InvalidSignature),
            "{curve:?}: wrong key"
        );
        assert_eq!(
            public.verify(message, &signature[..signature.len() - 1]),
            Err(Error::InvalidSignature),
            "{curve:?}: truncated"
        );
        assert_eq!(
            public.verify(message, &[]),
            Err(Error::InvalidSignature),
            "{curve:?}: empty"
        );
        // DER SEQUENCE header: tag 0x30 and a definite length.
        assert_eq!(signature[0], 0x30);
    }
}

#[test]
fn signature_uses_k_from_the_source() {
    let message = b"message";
    for curve in CURVES {
        let private = PrivateKey::from_be_bytes(curve, &[7]).expect("key");
        let public = private.public_key().expect("public key");
        let k = |value: u8| Repeating(be(curve.coordinate_len(), value));

        let first = private.sign(message, &mut k(5)).expect("sign");
        let again = private.sign(message, &mut k(5)).expect("sign");
        let other = private.sign(message, &mut k(6)).expect("sign");
        assert_eq!(first, again, "{curve:?}: same k, same signature");
        assert_ne!(first, other, "{curve:?}: different k");
        public.verify(message, &first).expect("verify");
        public.verify(message, &other).expect("verify");
    }
}

#[test]
fn signing_redraws_an_unusable_k() {
    for curve in CURVES {
        let width = curve.coordinate_len();
        let private = PrivateKey::from_be_bytes(curve, &[7]).expect("key");
        // Zero is refused, then k = 5 is used: same result as signing with 5 directly.
        let mut redraw = Draws(vec![vec![0; width], be(width, 5)]);
        let signature = private.sign(b"m", &mut redraw).expect("sign");
        let direct = private
            .sign(b"m", &mut Repeating(be(width, 5)))
            .expect("sign");
        assert_eq!(signature, direct, "{curve:?}");
    }
}

#[test]
fn random_failure_propagates_through_keygen_and_signing() {
    for curve in CURVES {
        assert_eq!(
            PrivateKey::generate(curve, &mut FailingRandom).expect_err("keygen"),
            Error::Random(RandomError),
            "{curve:?}"
        );
        let private = PrivateKey::from_be_bytes(curve, &[7]).expect("key");
        assert_eq!(
            private.sign(b"m", &mut FailingRandom).expect_err("sign"),
            Error::Random(RandomError),
            "{curve:?}"
        );
    }
}

#[test]
fn a_source_that_never_yields_a_scalar_is_given_up_on() {
    for curve in CURVES {
        let private = PrivateKey::from_be_bytes(curve, &[7]).expect("key");
        let zeros = || Repeating(vec![0]);
        assert_eq!(
            PrivateKey::generate(curve, &mut zeros()).expect_err("keygen"),
            Error::SamplingExhausted
        );
        assert_eq!(
            private.sign(b"m", &mut zeros()).expect_err("sign"),
            Error::SamplingExhausted
        );
    }
}

#[test]
fn keygen_samples_from_the_source_and_retries_out_of_range_draws() {
    for curve in CURVES {
        let width = curve.coordinate_len();
        // 0xff... is not below the group order (also after P-521's top-bit mask): redraw.
        let mut source = Draws(vec![vec![0xff; width], be(width, 3)]);
        let generated = PrivateKey::generate(curve, &mut source).expect("keygen");
        let imported = PrivateKey::from_be_bytes(curve, &[3]).expect("import");
        assert_eq!(
            generated.public_key().expect("public"),
            imported.public_key().expect("public")
        );
        assert_eq!(generated.scalar().to_be_bytes().expose(), be(width, 3));
    }
}

#[test]
fn p521_keygen_masks_the_top_seven_bits() {
    // Without the mask, 0xfe... would be far above the order and rejected; with it the draw is
    // the scalar 0x00..05.
    let mut draw = be(66, 5);
    draw[0] = 0xfe;
    let key = PrivateKey::generate(Curve::P521, &mut Draws(vec![draw])).expect("keygen");
    assert_eq!(key.scalar().to_be_bytes().expose(), be(66, 5));
}

#[test]
fn on_curve_check_rejects_bad_points() {
    for curve in CURVES {
        let (_, public) = generated(curve);
        let xy = public.point().as_xy();
        assert!(Point::from_xy(curve, xy).is_ok());

        let off_curve = flip_last_byte(xy);
        let all_ones = vec![0xff; curve.point_len()];
        let all_zero = vec![0; curve.point_len()];
        for (name, bad) in [
            ("y changed", off_curve.as_slice()),
            ("coordinates not below p", all_ones.as_slice()),
            ("origin", all_zero.as_slice()),
            ("too short", &xy[1..]),
            ("empty", &[]),
        ] {
            assert_eq!(
                Point::from_xy(curve, bad).expect_err(name),
                Error::InvalidPoint,
                "{curve:?}: {name}"
            );
        }
        let mut too_long = xy.to_vec();
        too_long.push(0);
        assert_eq!(
            Point::from_xy(curve, &too_long).expect_err("too long"),
            Error::InvalidPoint
        );
    }
}

#[test]
fn spki_import_rejects_an_off_curve_point_and_malformed_der() {
    for curve in CURVES {
        let (_, public) = generated(curve);
        let der = public.to_spki_der().expect("export");

        let off_curve = flip_last_byte(&der);
        assert_eq!(
            PublicKey::from_spki_der(&off_curve).expect_err("off curve"),
            Error::InvalidPoint
        );
        let trailing = [der.as_slice(), &[0]].concat();
        assert_eq!(
            PublicKey::from_spki_der(&trailing).expect_err("trailing data"),
            Error::InvalidSpki
        );
        assert_eq!(
            PublicKey::from_spki_der(&der[..der.len() - 1]).expect_err("truncated"),
            Error::InvalidSpki
        );
        assert_eq!(
            PublicKey::from_spki_der(&[]).expect_err("empty"),
            Error::InvalidSpki
        );
    }
}

#[test]
fn compressed_keys_round_trip_byte_exact() {
    for curve in CURVES {
        let (_, public) = generated(curve);
        let compressed = PublicKey::new(
            Point::from_xy(curve, public.point().as_xy()).expect("point"),
            PointForm::Compressed,
        );
        let der = compressed.to_spki_der().expect("export");
        // The BIT STRING ends with `02|03 || X` instead of `04 || X || Y`.
        let uncompressed_der = public.to_spki_der().expect("export");
        let xy = public.point().as_xy();
        let (x, _) = xy.split_at(curve.coordinate_len());
        assert!(uncompressed_der.ends_with(xy));
        assert!(der.ends_with(x));
        let tag = der[der.len() - x.len() - 1];
        assert!(tag == 0x02 || tag == 0x03, "{curve:?}: tag {tag:#04x}");
        assert!(uncompressed_der.len() > der.len());

        let imported = PublicKey::from_spki_der(&der).expect("import");
        assert_eq!(imported.form(), PointForm::Compressed);
        assert_eq!(imported, compressed);
        assert_eq!(imported.to_spki_der().expect("export"), der);
    }
}

#[test]
fn boot_key_hash_is_taken_over_the_re_serialized_der() {
    let (_, public) = generated(Curve::P256);
    let der = public.to_spki_der().expect("export");
    let plain = em_crypto::ec::boot_key_hash(&der, None).expect("hash");
    assert_eq!(plain, public.boot_key_hash(None).expect("hash"));
    assert_eq!(plain, public.boot_key_hash(Some("")).expect("hash"));
    let prefixed = public.boot_key_hash(Some("chirp")).expect("hash");
    assert_ne!(plain, prefixed);

    // Raw input that is not canonical DER is refused, never hashed.
    let mut trailing = der;
    trailing.push(0);
    assert_eq!(
        em_crypto::ec::boot_key_hash(&trailing, None).expect_err("trailing data"),
        Error::InvalidSpki
    );
    assert_eq!(
        public.boot_key_hash(Some("a\0b")).expect_err("NUL prefix"),
        Error::InvalidLabel
    );
}

#[test]
fn ecdh_x_matches_point_multiplication_and_agrees_both_ways() {
    for curve in CURVES {
        let (alice, alice_public) = generated(curve);
        let (bob, bob_public) = generated(curve);
        let shared = alice.ecdh(bob_public.point()).expect("ecdh");
        assert_eq!(
            shared.expose(),
            bob.ecdh(alice_public.point()).expect("ecdh").expose()
        );
        let product = bob_public
            .point()
            .multiply(alice.scalar())
            .expect("multiply");
        assert_eq!(shared.expose(), &product.as_xy()[..curve.coordinate_len()]);
    }
}

#[test]
fn private_key_import_checks_length_and_range() {
    for curve in CURVES {
        let width = curve.coordinate_len();
        let order_minus_one = row("ec-scalar", &format!("{curve:?}").to_lowercase())
            .inputs
            .bytes("left");
        assert!(PrivateKey::from_be_bytes(curve, &order_minus_one).is_ok());
        assert!(PrivateKey::from_be_bytes(curve, &[1]).is_ok());
        // A single nonzero byte anywhere in a full-width value makes a valid key (the zero test
        // looks at the whole value, not at a prefix or a suffix).
        for position in [0, width / 2, width - 1] {
            let mut bytes = vec![0; width];
            bytes[position] = 1;
            assert!(
                PrivateKey::from_be_bytes(curve, &bytes).is_ok(),
                "{curve:?}: byte {position}"
            );
        }
        for (name, bad) in [
            ("zero", vec![0]),
            ("zero, full width", vec![0; width]),
            ("empty", vec![]),
            ("too long", vec![1; width + 1]),
            ("above the order", vec![0xff; width]),
        ] {
            assert_eq!(
                PrivateKey::from_be_bytes(curve, &bad).expect_err(name),
                Error::InvalidScalar,
                "{curve:?}: {name}"
            );
        }
    }
}

#[test]
fn point_arithmetic_edge_cases() {
    for curve in CURVES {
        let name = format!("{curve:?}").to_lowercase();
        let generator = PrivateKey::from_be_bytes(curve, &[1])
            .expect("key")
            .public_key()
            .expect("public");
        let order_minus_one = row("ec-scalar", &name).inputs.bytes("left");
        let negated = generator
            .point()
            .multiply(&Scalar::from_be_bytes(curve, &order_minus_one).expect("scalar"))
            .expect("multiply");

        // G + (-G) is the point at infinity, and 0 * G too: neither can be encoded.
        assert_eq!(
            generator.point().add(&negated).expect_err("infinity"),
            Error::InvalidPoint
        );
        let zero = Scalar::from_be_bytes(curve, &[0]).expect("zero is a scalar");
        assert_eq!(
            generator.point().multiply(&zero).expect_err("zero"),
            Error::InvalidPoint
        );
        // Doubling works through the addition path.
        let doubled = generator.point().add(generator.point()).expect("double");
        let by_two = generator
            .point()
            .multiply(&Scalar::from_be_bytes(curve, &[2]).expect("two"))
            .expect("multiply");
        assert_eq!(doubled, by_two);
    }
}

#[test]
fn values_of_different_curves_do_not_mix() {
    let (key256, public256) = generated(Curve::P256);
    let (_, public384) = generated(Curve::P384);
    assert_eq!(
        key256.ecdh(public384.point()).expect_err("ecdh"),
        Error::CurveMismatch
    );
    assert_eq!(
        public256.point().add(public384.point()).expect_err("add"),
        Error::CurveMismatch
    );
    let scalar384 = Scalar::from_be_bytes(Curve::P384, &[2]).expect("scalar");
    assert_eq!(
        public256
            .point()
            .multiply(&scalar384)
            .expect_err("multiply"),
        Error::CurveMismatch
    );
    assert_eq!(
        key256.scalar().add(&scalar384).expect_err("scalar add"),
        Error::CurveMismatch
    );
}
