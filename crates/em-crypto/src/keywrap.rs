// SPDX-License-Identifier: Apache-2.0

//! AES key wrap (RFC 3394) with a 128-, 192- or 256-bit KEK and the default IV `A6A6...A6`.

use aes::cipher::consts::U16;
use aes::cipher::{BlockDecrypt, BlockSizeUser, KeyInit};
use aes::{Aes128, Aes192, Aes256, Block};
use aes_kw::{KekAes128, KekAes192, KekAes256};
use zeroize::Zeroizing;

use crate::ct;
use crate::error::Error;
use crate::secret::SecretBytes;

/// Smallest input to [`wrap`] in bytes.
pub const MIN_WRAP_INPUT_LEN: usize = 16;
/// Smallest input to [`unwrap`] in bytes.
pub const MIN_UNWRAP_INPUT_LEN: usize = 24;
/// Length the wrapped form adds to the key data.
const OVERHEAD_LEN: usize = 8;
/// Length of one RFC 3394 half-block.
const HALF_BLOCK_LEN: usize = 8;
/// The RFC 3394 default initial value, which unwrapping must recover.
const DEFAULT_IV: [u8; HALF_BLOCK_LEN] = [0xa6; HALF_BLOCK_LEN];

enum Kek {
    Aes128(KekAes128),
    Aes192(KekAes192),
    Aes256(KekAes256),
}

impl Kek {
    fn new(kek: &[u8]) -> Result<Self, Error> {
        let invalid = |_| Error::InvalidKeyLength;
        match kek.len() {
            16 => KekAes128::try_from(kek).map(Kek::Aes128).map_err(invalid),
            24 => KekAes192::try_from(kek).map(Kek::Aes192).map_err(invalid),
            32 => KekAes256::try_from(kek).map(Kek::Aes256).map_err(invalid),
            _ => Err(Error::InvalidKeyLength),
        }
    }
}

/// Wraps `plaintext` (at least 16 bytes, a multiple of 8) under `kek` (16, 24 or 32 bytes).
///
/// # Errors
/// [`Error::InvalidKeyLength`] or [`Error::InvalidInputLength`].
pub fn wrap(kek: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, Error> {
    let kek = Kek::new(kek)?;
    if plaintext.len() < MIN_WRAP_INPUT_LEN || plaintext.len() % 8 != 0 {
        return Err(Error::InvalidInputLength);
    }
    let mut wrapped = vec![0; plaintext.len() + OVERHEAD_LEN];
    match kek {
        Kek::Aes128(kek) => kek.wrap(plaintext, &mut wrapped),
        Kek::Aes192(kek) => kek.wrap(plaintext, &mut wrapped),
        Kek::Aes256(kek) => kek.wrap(plaintext, &mut wrapped),
    }
    .map_err(|_| Error::InvalidInputLength)?;
    Ok(wrapped)
}

/// Unwraps `wrapped` (at least 24 bytes, a multiple of 8) and checks the integrity value.
///
/// RFC 3394 section 2.2.2 is implemented here instead of calling `aes-kw`, which compares the
/// recovered integrity value with `==`; this compares all eight bytes with [`ct::eq`].
///
/// # Errors
/// [`Error::InvalidKeyLength`], [`Error::InvalidInputLength`], or
/// [`Error::AuthenticationFailed`] when the integrity check fails (the partial output is wiped).
pub fn unwrap(kek: &[u8], wrapped: &[u8]) -> Result<SecretBytes, Error> {
    let invalid = |_| Error::InvalidKeyLength;
    // Check the key length before the input length, like `wrap`.
    let unwrapper = match kek.len() {
        16 => Unwrapper::Aes128(Aes128::new_from_slice(kek).map_err(invalid)?),
        24 => Unwrapper::Aes192(Aes192::new_from_slice(kek).map_err(invalid)?),
        32 => Unwrapper::Aes256(Aes256::new_from_slice(kek).map_err(invalid)?),
        _ => return Err(Error::InvalidKeyLength),
    };
    if wrapped.len() < MIN_UNWRAP_INPUT_LEN || wrapped.len() % 8 != 0 {
        return Err(Error::InvalidInputLength);
    }
    match unwrapper {
        Unwrapper::Aes128(cipher) => unwrap_blocks(&cipher, wrapped),
        Unwrapper::Aes192(cipher) => unwrap_blocks(&cipher, wrapped),
        Unwrapper::Aes256(cipher) => unwrap_blocks(&cipher, wrapped),
    }
}

/// The AES block cipher for the KEK length. The `aes` crate wipes its key schedule on drop.
enum Unwrapper {
    Aes128(Aes128),
    Aes192(Aes192),
    Aes256(Aes256),
}

/// RFC 3394 W^-1: six passes over the `n` key-data blocks, then the integrity check.
fn unwrap_blocks<C>(cipher: &C, wrapped: &[u8]) -> Result<SecretBytes, Error>
where
    C: BlockDecrypt + BlockSizeUser<BlockSize = U16>,
{
    let (first, rest) = wrapped.split_at(HALF_BLOCK_LEN);
    let block_count = rest.len() / HALF_BLOCK_LEN;
    let mut integrity = Zeroizing::new([0_u8; HALF_BLOCK_LEN]);
    integrity.copy_from_slice(first);
    // Dropping `output` on the error path wipes the partial plaintext.
    let mut output = SecretBytes::from_slice(rest);
    let mut block = Zeroizing::new([0_u8; 2 * HALF_BLOCK_LEN]);
    for round in (0..6_u64).rev() {
        for index in (1..=block_count).rev() {
            let step = round * block_count as u64 + index as u64;
            let half_block =
                &mut output.expose_mut()[(index - 1) * HALF_BLOCK_LEN..][..HALF_BLOCK_LEN];
            for (a, t) in integrity.iter_mut().zip(step.to_be_bytes()) {
                *a ^= t;
            }
            block[..HALF_BLOCK_LEN].copy_from_slice(&*integrity);
            block[HALF_BLOCK_LEN..].copy_from_slice(half_block);
            cipher.decrypt_block(Block::from_mut_slice(&mut block[..]));
            integrity.copy_from_slice(&block[..HALF_BLOCK_LEN]);
            half_block.copy_from_slice(&block[HALF_BLOCK_LEN..]);
        }
    }
    if !ct::eq(&*integrity, &DEFAULT_IV) {
        return Err(Error::AuthenticationFailed);
    }
    Ok(output)
}
