// SPDX-License-Identifier: Apache-2.0

//! Behaviour of each module beyond the oracle rows: bounds, rejections and round trips.

mod common;

use common::row;
use em_crypto::dh::{DH_VALUE_LEN, DhPrivate, DhPublic};
use em_crypto::encoding::{base64_decode, base64_encode, base64url_decode, base64url_encode};
use em_crypto::hash::HashAlgorithm;
use em_crypto::kdf;
use em_crypto::rand::OsRandom;
use em_crypto::{Error, cbc, ct, keywrap, siv};

#[test]
fn constant_time_compare_checks_length_and_content() {
    assert!(ct::eq(b"", b""));
    assert!(ct::eq(b"abcdefgh", b"abcdefgh"));
    assert!(!ct::eq(b"abcdefgh", b"abcdefgi"));
    assert!(!ct::eq(b"abcdefgh", b"ibcdefgh"));
    // A matching prefix is not enough (the upstream one-byte comparison defect).
    assert!(!ct::eq(b"abcdefgh", b"a"));
    assert!(!ct::eq(b"a", b"abcdefgh"));
    assert!(!ct::eq(b"", b"a"));
}

#[test]
fn kdf_80211_length_bounds() {
    let key = [7_u8; 32];
    let derive = |len| kdf::kdf_80211(HashAlgorithm::Sha256, &key, "label", b"ctx", len);
    assert_eq!(derive(8191).expect("max").len(), 8191);
    assert_eq!(derive(8192).expect_err("one over"), Error::OutputLength);
    assert_eq!(derive(0).expect_err("empty"), Error::OutputLength);
    assert_eq!(derive(usize::MAX).expect_err("huge"), Error::OutputLength);
    assert_eq!(
        kdf::kdf_80211(HashAlgorithm::Sha256, &[], "label", b"", 16).expect_err("empty key"),
        Error::InvalidKeyLength
    );
    assert_eq!(
        kdf::kdf_80211(HashAlgorithm::Sha256, &key, "la\0bel", b"", 16).expect_err("nul"),
        Error::InvalidLabel
    );
    // The output length is part of every HMAC input, so a shorter output is not a prefix of a
    // longer one.
    let short = derive(32).expect("32");
    let longer = derive(33).expect("33");
    assert_ne!(short.expose(), &longer.expose()[..32]);
}

#[test]
fn wps_kdf_bounds() {
    let key = [7_u8; 32];
    assert_eq!(kdf::WPS_KDF_MAX_OUTPUT_LEN, 536_870_911);
    // Refused from 2^29 bytes, before any allocation of that size.
    for len in [536_870_912, 1 << 31, usize::MAX] {
        assert_eq!(
            kdf::wps_kdf(&key, b"", "label", len).expect_err("too long"),
            Error::OutputLength
        );
    }
    assert!(
        kdf::wps_kdf(&key, b"", "label", 0)
            .expect("empty")
            .is_empty()
    );
    assert_eq!(kdf::wps_kdf(&key, b"p", "label", 80).expect("80").len(), 80);
    assert_eq!(
        kdf::wps_kdf(&key[..31], b"", "label", 16).expect_err("key"),
        Error::InvalidKeyLength
    );
    assert_eq!(
        kdf::wps_kdf(&key, b"", "la\0bel", 16).expect_err("nul"),
        Error::InvalidLabel
    );
}

#[test]
fn hkdf_and_pbkdf2_bounds() {
    let derive =
        |algorithm: HashAlgorithm, len| kdf::hkdf(algorithm, b"ikm", b"salt", b"info", len);
    for algorithm in [
        HashAlgorithm::Sha256,
        HashAlgorithm::Sha384,
        HashAlgorithm::Sha512,
    ] {
        let max = 255 * algorithm.output_len();
        assert_eq!(derive(algorithm, max).expect("255 blocks").len(), max);
        assert_eq!(
            derive(algorithm, max + 1).expect_err("256 blocks"),
            Error::OutputLength
        );
    }
    assert!(derive(HashAlgorithm::Sha256, 0).expect("empty").is_empty());
    assert_eq!(
        kdf::pbkdf2_hmac_sha1(b"password", b"salt", 0, 32).expect_err("zero iterations"),
        Error::InvalidInputLength
    );
}

#[test]
fn keywrap_input_sizes() {
    for kek_len in [16, 24, 32] {
        let kek = vec![9_u8; kek_len];
        // 16-byte wrap input and 24-byte unwrap input are the smallest accepted.
        let wrapped = keywrap::wrap(&kek, &[1; 16]).expect("16 bytes");
        assert_eq!(wrapped.len(), 24);
        assert_eq!(
            keywrap::unwrap(&kek, &wrapped).expect("24 bytes").expose(),
            &[1; 16]
        );

        for bad in [0, 8, 15, 17, 23] {
            assert_eq!(
                keywrap::wrap(&kek, &vec![1; bad]).expect_err("wrap"),
                Error::InvalidInputLength
            );
        }
        for bad in [0, 8, 16, 23, 25] {
            assert_eq!(
                keywrap::unwrap(&kek, &vec![1; bad]).expect_err("unwrap"),
                Error::InvalidInputLength
            );
        }
        // Any flipped bit breaks the integrity value.
        for index in 0..wrapped.len() {
            let mut tampered = wrapped.as_slice().to_vec();
            tampered[index] ^= 0x01;
            assert_eq!(
                keywrap::unwrap(&kek, &tampered).expect_err("tampered"),
                Error::AuthenticationFailed
            );
        }
    }
    for bad_kek in [0, 15, 20, 33] {
        assert_eq!(
            keywrap::wrap(&vec![0; bad_kek], &[1; 16]).expect_err("kek"),
            Error::InvalidKeyLength
        );
        assert_eq!(
            keywrap::unwrap(&vec![0; bad_kek], &[1; 24]).expect_err("kek"),
            Error::InvalidKeyLength
        );
    }
}

#[test]
fn cbc_round_trips_and_rejects_bad_input() {
    let key = [3_u8; 16];
    let iv = [4_u8; 16];
    for len in 0..=40 {
        let plain: Vec<u8> = (0..len).map(|i| u8::try_from(i).expect("small")).collect();
        let cipher = cbc::encrypt(&key, &iv, &plain);
        assert_eq!(cipher.len(), (len / 16 + 1) * 16);
        assert_eq!(
            cbc::decrypt(&key, &iv, &cipher).expect("decrypt").expose(),
            plain.as_slice()
        );
    }
    // An empty plaintext is a success, distinct from malformed padding.
    let empty = cbc::decrypt(&key, &iv, &cbc::encrypt(&key, &iv, b"")).expect("empty plaintext");
    assert!(empty.is_empty());
    assert_eq!(
        cbc::decrypt(&key, &iv, &[]).expect_err("no blocks"),
        Error::InvalidPadding
    );
    assert_eq!(
        cbc::decrypt(&key, &iv, &[0; 15]).expect_err("partial block"),
        Error::InvalidPadding
    );
    assert_eq!(
        cbc::decrypt(&key, &iv, &[0; 17]).expect_err("partial block"),
        Error::InvalidPadding
    );
}

#[test]
fn siv_round_trips_all_key_sizes_and_authenticates_ordered_ad() {
    for key_len in [32, 48, 64] {
        let key: Vec<u8> = (0..key_len)
            .map(|i| u8::try_from(i).expect("small"))
            .collect();
        let plain = b"hello siv";
        let sealed = siv::encrypt(&key, &[b"first", b"second"], plain).expect("encrypt");
        assert_eq!(sealed.len(), siv::TAG_LEN + plain.len());
        let opened = siv::decrypt(&key, &[b"first", b"second"], &sealed).expect("decrypt");
        assert_eq!(opened.expose(), plain);

        // Order, splitting and presence of associated data all matter.
        for other in [
            &[&b"second"[..], b"first"][..],
            &[b"firstsecond"],
            &[b"first"],
            &[],
        ] {
            assert_eq!(
                siv::decrypt(&key, other, &sealed).expect_err("ad"),
                Error::AuthenticationFailed
            );
        }
        // Every flipped bit is rejected.
        for index in 0..sealed.len() {
            let mut tampered = sealed.as_slice().to_vec();
            tampered[index] ^= 0x80;
            assert_eq!(
                siv::decrypt(&key, &[b"first", b"second"], &tampered).expect_err("tampered"),
                Error::AuthenticationFailed
            );
        }
        // Empty plaintext and no associated data.
        let empty = siv::encrypt(&key, &[], b"").expect("empty");
        assert_eq!(empty.len(), siv::TAG_LEN);
        assert!(siv::decrypt(&key, &[], &empty).expect("empty").is_empty());
    }
}

#[test]
fn siv_rejects_bad_keys_and_inputs() {
    for key_len in [0, 16, 31, 33, 47, 63, 65] {
        let key = vec![1; key_len];
        assert_eq!(
            siv::encrypt(&key, &[], b"x").expect_err("key"),
            Error::InvalidKeyLength
        );
        assert_eq!(
            siv::decrypt(&key, &[], &[0; 32]).expect_err("key"),
            Error::InvalidKeyLength
        );
    }
    let key = [1_u8; 32];
    assert_eq!(
        siv::decrypt(&key, &[], &[0; 15]).expect_err("short"),
        Error::InvalidInputLength
    );
    let many: Vec<&[u8]> = vec![b"a"; 127];
    assert_eq!(
        siv::encrypt(&key, &many, b"x").expect_err("ad count"),
        Error::InvalidInputLength
    );
    assert_eq!(
        siv::decrypt(&key, &many, &[0; 32]).expect_err("ad count"),
        Error::InvalidInputLength
    );
    let allowed: Vec<&[u8]> = vec![b"a"; 126];
    let sealed = siv::encrypt(&key, &allowed, b"x").expect("126 pieces");
    assert_eq!(
        siv::decrypt(&key, &allowed, &sealed)
            .expect("round trip")
            .expose(),
        b"x"
    );
}

#[test]
fn base64_cases() {
    assert_eq!(base64_encode(b""), "");
    assert_eq!(base64_encode(&[0xfb, 0xff]), "+/8=");
    assert_eq!(base64url_encode(&[0xfb, 0xff]), "-_8");
    assert_eq!(base64_decode("+/8=").expect("decode"), [0xfb, 0xff]);
    assert_eq!(base64_decode("").expect("empty"), Vec::<u8>::new());
    // Strict standard decoding: no missing padding, no URL alphabet, no whitespace.
    for bad in ["TQ", "-_8=", "TQ==\n", "TQ == ", "T", "TQ==="] {
        assert_eq!(base64_decode(bad).expect_err(bad), Error::InvalidEncoding);
    }
    // Lenient base64url decoding accepts both alphabets, with or without padding.
    for good in ["TQ", "TQ==", "TQ="] {
        assert_eq!(base64url_decode(good).expect(good), [0x4d]);
    }
    assert_eq!(base64url_decode("-_8").expect("url"), [0xfb, 0xff]);
    assert_eq!(base64url_decode("+/8=").expect("std"), [0xfb, 0xff]);
    // Text after a NUL is ignored, like upstream's C-string handling.
    assert_eq!(base64url_decode("TWFu\0TQ==").expect("nul"), b"Man");
    // Known difference (README): stricter than OpenSSL outside the pinned set.
    for bad in ["TR==", "TQ\n==", "A", "TQ====", "$$$$"] {
        assert_eq!(
            base64url_decode(bad).expect_err(bad),
            Error::InvalidEncoding
        );
    }
}

#[test]
fn keywrap_round_trips_every_size_and_kek() {
    for kek_len in [16_usize, 24, 32] {
        let kek: Vec<u8> = (0_u8..).take(kek_len).collect();
        // Distinct bytes in every block, so a wrong block order or round count shows up.
        for plain_len in (16..=64).step_by(8) {
            let plain: Vec<u8> = (0_u8..)
                .take(plain_len)
                .map(|i| i.wrapping_mul(7).wrapping_add(1))
                .collect();
            let wrapped = keywrap::wrap(&kek, &plain).expect("wrap");
            assert_eq!(wrapped.len(), plain_len + 8);
            let unwrapped = keywrap::unwrap(&kek, &wrapped).expect("unwrap");
            assert_eq!(unwrapped.expose(), plain.as_slice());
            // A different KEK recovers a different integrity value.
            let mut other = kek.as_slice().to_vec();
            other[0] ^= 1;
            assert_eq!(
                keywrap::unwrap(&other, &wrapped).expect_err("wrong kek"),
                Error::AuthenticationFailed
            );
        }
    }
}

fn dh_prime() -> Vec<u8> {
    let fixture = row("dh", "p");
    fixture.inputs.bytes("peer")
}

#[test]
fn dh_private_import_checks_length_and_range() {
    let prime = dh_prime();
    assert_eq!(prime.len(), DH_VALUE_LEN);
    assert_eq!(
        DhPrivate::from_be_bytes(&[]).expect_err("empty"),
        Error::InvalidInputLength
    );
    assert_eq!(
        DhPrivate::from_be_bytes(&[0; 193]).expect_err("long"),
        Error::InvalidInputLength
    );
    assert_eq!(
        DhPrivate::from_be_bytes(&[1; 1000]).expect_err("long"),
        Error::InvalidInputLength
    );
    for out_of_range in [vec![0], vec![1], vec![0; 192], prime.as_slice().to_vec()] {
        assert_eq!(
            DhPrivate::from_be_bytes(&out_of_range).expect_err("range"),
            Error::ValueOutOfRange
        );
    }
    assert!(DhPrivate::from_be_bytes(&[2]).is_ok());
    assert!(DhPrivate::from_be_bytes(&[0, 0, 2]).is_ok());

    // p - 2 is the largest exponent; p - 1 is refused.
    let mut max = prime.as_slice().to_vec();
    max[DH_VALUE_LEN - 1] -= 2;
    assert!(DhPrivate::from_be_bytes(&max).is_ok());
    max[DH_VALUE_LEN - 1] += 1;
    assert_eq!(
        DhPrivate::from_be_bytes(&max).expect_err("p-1"),
        Error::ValueOutOfRange
    );
}

#[test]
fn dh_private_import_pads_without_stripping() {
    // The same value, with and without leading zero bytes, is the same exponent.
    let bare = DhPrivate::from_be_bytes(&[0x01, 0x02]).expect("bare");
    let padded = DhPrivate::from_be_bytes(&[0, 0, 0, 0x01, 0x02]).expect("padded");
    let full_width = {
        let mut bytes = vec![0; DH_VALUE_LEN];
        bytes[DH_VALUE_LEN - 2..].copy_from_slice(&[0x01, 0x02]);
        DhPrivate::from_be_bytes(&bytes).expect("full width")
    };
    assert_eq!(bare.to_be_bytes().expose(), padded.to_be_bytes().expose());
    assert_eq!(
        bare.to_be_bytes().expose(),
        full_width.to_be_bytes().expose()
    );
    assert_eq!(
        bare.to_be_bytes().expose()[DH_VALUE_LEN - 2..],
        [0x01, 0x02]
    );
}

#[test]
fn dh_public_import_accepts_padded_values_in_range() {
    let prime = dh_prime();

    let two = DhPublic::from_be_bytes(&[2]).expect("2");
    assert_eq!(two.as_bytes()[..191], [0; 191]);
    assert_eq!(two.as_bytes()[191], 2);

    // Leading zeros of any count are fine; the stored value is always 192 bytes.
    let mut long_padding = vec![0; 300];
    long_padding[299] = 2;
    assert_eq!(DhPublic::from_be_bytes(&long_padding).expect("padded"), two);
    assert_eq!(DhPublic::from_be_bytes(&[0, 2]).expect("short pad"), two);

    // Round trip of a real public value, with and without its own leading zero bytes.
    let private = DhPrivate::from_be_bytes(&[3]).expect("exponent");
    let public = private.public_key();
    assert_eq!(
        DhPublic::from_be_bytes(public.as_bytes()).expect("round trip"),
        public
    );
    assert_eq!(
        DhPublic::from_be_bytes(&[0; 5]).expect_err("zero"),
        Error::ValueOutOfRange
    );
    let mut shifted = vec![0; 8];
    shifted.extend_from_slice(public.as_bytes());
    assert_eq!(
        DhPublic::from_be_bytes(&shifted).expect("extra zeros"),
        public
    );

    // p - 2 is the largest accepted value.
    let mut max = prime.as_slice().to_vec();
    max[DH_VALUE_LEN - 1] -= 2;
    assert_eq!(
        DhPublic::from_be_bytes(&max)
            .expect("p-2")
            .as_bytes()
            .as_slice(),
        max
    );
}

#[test]
fn dh_public_import_rejects_out_of_range_values() {
    let prime = dh_prime();
    let mut p_minus_1 = prime.as_slice().to_vec();
    p_minus_1[DH_VALUE_LEN - 1] -= 1;
    let mut too_long = vec![0; 193];
    too_long[0] = 1;
    too_long[192] = 2;
    for rejected in [
        vec![],
        vec![0],
        vec![1],
        prime,
        p_minus_1,
        too_long,
        vec![0xff; 193],
    ] {
        assert_eq!(
            DhPublic::from_be_bytes(&rejected).expect_err("range"),
            Error::ValueOutOfRange
        );
    }
}

#[test]
fn dh_export_is_fixed_width() {
    let private = DhPrivate::from_be_bytes(&[3]).expect("exponent");
    let exported = private.to_be_bytes();
    assert_eq!(exported.expose().len(), DH_VALUE_LEN);
    assert_eq!(exported.expose()[..191], [0; 191]);
    assert_eq!(exported.expose()[191], 3);
    assert_eq!(private.public_key().as_bytes()[..191], [0; 191]);
    assert_eq!(private.public_key().as_bytes()[191], 8);
}

#[test]
fn dh_peer_parsing_is_total() {
    let private = DhPrivate::from_be_bytes(&[3]).expect("exponent");
    let reference = private.shared_secret(&[2]).expect("peer 2");
    assert_eq!(reference.expose(), [8]);

    // Leading zeros of any count are ignored; more than 192 significant bytes are out of range.
    let mut padded = vec![0; 300];
    padded[299] = 2;
    assert_eq!(
        private.shared_secret(&padded).expect("padded").expose(),
        reference.expose()
    );
    let mut too_long = vec![0; 193];
    too_long[0] = 1;
    too_long[192] = 2;
    assert_eq!(
        private.shared_secret(&too_long).expect_err("long"),
        Error::ValueOutOfRange
    );
    assert_eq!(
        private.shared_secret(&[]).expect_err("empty"),
        Error::ValueOutOfRange
    );
}

#[test]
fn dh_two_parties_agree() {
    let alice = DhPrivate::generate(&mut OsRandom).expect("alice");
    let bob = DhPrivate::generate(&mut OsRandom).expect("bob");
    let alice_secret = alice
        .shared_secret(bob.public_key().as_bytes())
        .expect("alice side");
    let bob_secret = bob
        .shared_secret(alice.public_key().as_bytes())
        .expect("bob side");
    assert!(!alice_secret.is_empty());
    assert_eq!(alice_secret.expose(), bob_secret.expose());
}
