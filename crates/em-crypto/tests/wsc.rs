// SPDX-License-Identifier: Apache-2.0

//! The WSC key chain end to end against the oracle rows (`em_configuration.cpp:4077-4130`):
//! DH shared secret, SHA-256 (the `DHKey`), KDK = HMAC(`DHKey`, enrollee nonce || enrollee
//! MAC || registrar nonce), WPS KDF to 80 bytes, split into `AuthKey` (32), `KeyWrapKey` (16)
//! and EMSK (32).

mod common;

use common::{hex, row};
use em_crypto::dh::DhPrivate;
use em_crypto::hash::{self, HashAlgorithm};
use em_crypto::kdf;
use em_crypto::{Error, SecretArray, SecretBytes};

struct WscKeys {
    auth_key: SecretArray<32>,
    key_wrap_key: SecretArray<16>,
    emsk: SecretArray<32>,
}

/// Takes the derived buffer by value, borrows its slices into the three key types and then
/// drops (wipes) the buffer; nothing is cloned.
fn split_derived_keys(derived: SecretBytes) -> Result<WscKeys, Error> {
    let bytes = derived.expose();
    if bytes.len() != 80 {
        return Err(Error::OutputLength);
    }
    let (auth_key, rest) = bytes.split_at(32);
    let (key_wrap_key, emsk) = rest.split_at(16);
    let keys = WscKeys {
        auth_key: SecretArray::from_slice(auth_key)?,
        key_wrap_key: SecretArray::from_slice(key_wrap_key)?,
        emsk: SecretArray::from_slice(emsk)?,
    };
    drop(derived);
    Ok(keys)
}

#[test]
fn wsc_chain_matches_the_oracle() {
    let dh_row = row("wsc", "dh-secret");
    let hash_row = row("wsc", "dh-hash");
    let kdk_row = row("wsc", "kdk");
    let keys_row = row("wsc", "keys");

    // DH shared secret.
    let private = DhPrivate::from_be_bytes(&dh_row.inputs.bytes("private")).expect("private");
    let secret = private
        .shared_secret(&dh_row.inputs.bytes("peer"))
        .expect("shared secret");
    assert_eq!(secret.expose(), dh_row.output_hex().as_slice());

    // DHKey = SHA-256(secret).
    let dh_key = hash::digest(HashAlgorithm::Sha256, &[secret.expose()]);
    assert_eq!(dh_key.expose(), hash_row.output_hex().as_slice());

    // KDK from the nonces and the enrollee MAC; the row's DHKey input is the chain's DHKey.
    assert_eq!(kdk_row.inputs.bytes("dhkey"), dh_key.expose());
    let enrollee_nonce = kdk_row.inputs.bytes("enrollee_nonce");
    let enrollee_mac = kdk_row.inputs.bytes("mac");
    let registrar_nonce = kdk_row.inputs.bytes("registrar_nonce");
    let kdk = hash::hmac(
        HashAlgorithm::Sha256,
        dh_key.expose(),
        &[&enrollee_nonce, &enrollee_mac, &registrar_nonce],
    )
    .expect("kdk");
    assert_eq!(kdk.expose(), kdk_row.output_hex().as_slice());

    // WPS KDF and the 32/16/32 split.
    assert_eq!(keys_row.inputs.bytes("kdk"), kdk.expose());
    let label = hex(keys_row.inputs.get("label"));
    let derived = kdf::wps_kdf(
        kdk.expose(),
        &keys_row.inputs.bytes("prefix"),
        std::str::from_utf8(&label).expect("ascii label"),
        keys_row.inputs.number("length"),
    )
    .expect("wps kdf");
    let fields = keys_row.output_fields();
    assert_eq!(derived.expose(), fields.bytes("all").as_slice());

    let keys = split_derived_keys(derived).expect("split");
    assert_eq!(
        keys.auth_key.expose().as_slice(),
        fields.bytes("auth").as_slice()
    );
    assert_eq!(
        keys.key_wrap_key.expose().as_slice(),
        fields.bytes("wrap").as_slice()
    );
    assert_eq!(
        keys.emsk.expose().as_slice(),
        fields.bytes("emsk").as_slice()
    );
}

#[test]
fn split_rejects_a_derived_buffer_of_the_wrong_size() {
    let short = SecretBytes::from_slice(&[0; 79]);
    assert_eq!(split_derived_keys(short).err(), Some(Error::OutputLength));
}
