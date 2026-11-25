// SPDX-License-Identifier: Apache-2.0

//! Random-source failures propagate as errors through the key-generation paths tested here.
//! EC key generation and ECDSA signing are tested in `ec.rs`.

#![allow(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    reason = "0016 U27: integration test uses OS randomness"
)]

use em_crypto::dh::DhPrivate;
use em_crypto::rand::{FailingRandom, OsRandom, random_secret};
use em_crypto::{Error, Nonce, RandomError, RandomSource};

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

#[test]
fn failing_source_fails_every_generator() {
    assert_eq!(
        random_secret::<32>(&mut FailingRandom).expect_err("secret"),
        RandomError
    );
    assert_eq!(
        Nonce::random(&mut FailingRandom).expect_err("nonce"),
        RandomError
    );
    assert_eq!(
        DhPrivate::generate(&mut FailingRandom).expect_err("dh keygen"),
        Error::Random(RandomError)
    );
}

#[test]
fn os_source_produces_fresh_bytes() {
    let first = random_secret::<32>(&mut OsRandom::new()).expect("os random");
    let second = random_secret::<32>(&mut OsRandom::new()).expect("os random");
    assert_ne!(first.expose(), &[0; 32]);
    assert_ne!(first.expose(), second.expose());
}

#[test]
fn dh_keygen_uses_the_source_and_is_deterministic_for_a_fixed_source() {
    let mut exponent = vec![0_u8; 192];
    exponent[191] = 3;
    let generated = DhPrivate::generate(&mut Repeating(exponent)).expect("keygen");
    let imported = DhPrivate::from_be_bytes(&[3]).expect("import");
    assert_eq!(generated.public_key(), imported.public_key());
}

#[test]
fn dh_keygen_gives_up_on_a_source_that_never_yields_a_valid_exponent() {
    // All zero bytes and all 0xff bytes both fall outside 2 ..= p - 2.
    for fill in [0x00, 0xff] {
        let error = DhPrivate::generate(&mut Repeating(vec![fill])).expect_err("bad source");
        assert_eq!(error, Error::SamplingExhausted);
    }
}

#[test]
fn dh_keygen_retries_an_out_of_range_draw() {
    // 192 zero bytes (out of range) then a valid exponent in the second draw.
    struct ZeroThenThree(usize);
    impl RandomSource for ZeroThenThree {
        fn fill(&mut self, dest: &mut [u8]) -> Result<(), RandomError> {
            dest.fill(0);
            if self.0 > 0 {
                dest[dest.len() - 1] = 3;
            }
            self.0 += 1;
            Ok(())
        }
    }
    let generated = DhPrivate::generate(&mut ZeroThenThree(0)).expect("keygen");
    let imported = DhPrivate::from_be_bytes(&[3]).expect("import");
    assert_eq!(generated.public_key(), imported.public_key());
}
