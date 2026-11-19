// SPDX-License-Identifier: Apache-2.0

//! `keywrap::unwrap` checks all eight bytes of the RFC 3394 integrity value and
//! checks the key length before the input length.
//!
//! Bit-flipped ciphertexts scramble the whole recovered value, so they cannot
//! tell a full check from a partial one. Here the forward wrap (RFC 3394
//! section 2.2.1) is run with an initial value that differs from `A6` x 8 in
//! exactly one byte; unwrapping it must still fail.

use aes::cipher::{BlockEncrypt, BlockSizeUser, KeyInit, consts::U16};
use aes::{Aes128, Aes192, Aes256, Block};
use em_crypto::{Error, keywrap};

/// RFC 3394 W with a chosen initial value.
fn wrap_with_iv<C>(cipher: &C, iv: [u8; 8], plain: &[u8]) -> Vec<u8>
where
    C: BlockEncrypt + BlockSizeUser<BlockSize = U16>,
{
    let blocks = plain.len() / 8;
    let mut integrity = iv;
    let mut data = plain.to_vec();
    for round in 0..6_u64 {
        for index in 1..=blocks {
            let mut block = [0_u8; 16];
            block[..8].copy_from_slice(&integrity);
            block[8..].copy_from_slice(&data[(index - 1) * 8..index * 8]);
            cipher.encrypt_block(Block::from_mut_slice(&mut block));
            let step = u64::try_from(blocks).expect("block count") * round
                + u64::try_from(index).expect("index");
            integrity.copy_from_slice(&block[..8]);
            for (a, t) in integrity.iter_mut().zip(step.to_be_bytes()) {
                *a ^= t;
            }
            data[(index - 1) * 8..index * 8].copy_from_slice(&block[8..]);
        }
    }
    let mut out = integrity.to_vec();
    out.extend_from_slice(&data);
    out
}

fn wrap_for_kek(kek: &[u8], iv: [u8; 8], plain: &[u8]) -> Vec<u8> {
    match kek.len() {
        16 => wrap_with_iv(&Aes128::new_from_slice(kek).expect("kek"), iv, plain),
        24 => wrap_with_iv(&Aes192::new_from_slice(kek).expect("kek"), iv, plain),
        _ => wrap_with_iv(&Aes256::new_from_slice(kek).expect("kek"), iv, plain),
    }
}

#[test]
fn an_integrity_value_wrong_in_any_one_byte_is_rejected() {
    let plain = [5_u8; 24];
    for kek_len in [16_usize, 24, 32] {
        let kek: Vec<u8> = (1_u8..).take(kek_len).collect();
        // Control: with the default value the helper equals `wrap` and unwraps.
        let wrapped = wrap_for_kek(&kek, [0xA6; 8], &plain);
        assert_eq!(wrapped, keywrap::wrap(&kek, &plain).expect("wrap"));
        assert_eq!(
            keywrap::unwrap(&kek, &wrapped).expect("unwrap").expose(),
            plain.as_slice()
        );
        for position in 0..8 {
            let mut iv = [0xA6_u8; 8];
            iv[position] ^= 0x01;
            let wrapped = wrap_for_kek(&kek, iv, &plain);
            assert_eq!(
                keywrap::unwrap(&kek, &wrapped).expect_err("changed integrity value"),
                Error::AuthenticationFailed,
                "KEK of {kek_len} bytes, byte {position}"
            );
        }
    }
}

#[test]
fn key_length_is_checked_before_input_length() {
    assert_eq!(
        keywrap::unwrap(&[0; 15], &[1; 8]).expect_err("kek"),
        Error::InvalidKeyLength
    );
    assert_eq!(
        keywrap::wrap(&[0; 15], &[1; 8]).expect_err("kek"),
        Error::InvalidKeyLength
    );
}
