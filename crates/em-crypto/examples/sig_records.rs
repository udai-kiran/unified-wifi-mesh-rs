// SPDX-License-Identifier: Apache-2.0

//! Writes ECDSA records signed by this crate, for the upstream verifier (note 0006, D4 item 2).
//!
//! ```sh
//! cargo run -p em-crypto --example sig_records -- /tmp/records.tsv
//! tools/upstream-crypto-vectors/regen --verify-records /tmp/records.tsv
//! ```
//!
//! One line per record: `curve` (`p256`, `p384` or `p521`), `SubjectPublicKeyInfo` DER, message
//! and DER signature, separated by tabs. The byte fields are lowercase hex. The keys are the fixed
//! fixture keys, freshly generated keys, and one compressed-point form of a generated key per
//! curve.

#![allow(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    reason = "0016 U27: example uses OS randomness"
)]

use std::error::Error;
use std::fmt::Write as _;

use em_crypto::RandomSource;
use em_crypto::ec::{Curve, Point, PointForm, PrivateKey, PublicKey};
use em_crypto::rand::OsRandom;

/// The fixed connector text the oracle fixture signs.
const CONNECTOR: &str = "eyJ0eXAiOiJkcHBDb24iLCJhbGciOiJFUzI1NiJ9.eyJzdWIiOiJmaXhlZC1tZXNoIn0";
const MESSAGE_LENGTHS: [usize; 3] = [1, 17, 300];

fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(text, "{byte:02x}").expect("writing to a String cannot fail");
    }
    text
}

fn curve_name(curve: Curve) -> &'static str {
    match curve {
        Curve::P256 => "p256",
        Curve::P384 => "p384",
        Curve::P521 => "p521",
    }
}

fn add_record(
    records: &mut String,
    private: &PrivateKey,
    public: &PublicKey,
    message: &[u8],
) -> Result<(), Box<dyn Error>> {
    let signature = private.sign(message, &mut OsRandom::new())?;
    // The Rust verifier must agree before the upstream one is asked.
    public.verify(message, &signature)?;
    writeln!(
        records,
        "{}\t{}\t{}\t{}",
        curve_name(public.curve()),
        hex(&public.to_spki_der()?),
        hex(message),
        hex(&signature)
    )?;
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("usage: sig_records OUTPUT_FILE")?;
    let mut records = String::new();

    for (curve, scalar) in [(Curve::P256, 3), (Curve::P384, 4), (Curve::P521, 5)] {
        // The keys of the oracle fixture's `signature` rows.
        let fixed = PrivateKey::from_be_bytes(curve, &[scalar])?;
        add_record(
            &mut records,
            &fixed,
            &fixed.public_key()?,
            CONNECTOR.as_bytes(),
        )?;

        for length in MESSAGE_LENGTHS {
            let private = PrivateKey::generate(curve, &mut OsRandom::new())?;
            let public = private.public_key()?;
            let mut message = vec![0; length];
            OsRandom::new().fill(&mut message)?;
            add_record(&mut records, &private, &public, &message)?;
        }

        // The same kind of key, exported with a compressed point.
        let private = PrivateKey::generate(curve, &mut OsRandom::new())?;
        let uncompressed = private.public_key()?;
        let compressed = PublicKey::new(
            Point::from_xy(curve, uncompressed.point().as_xy())?,
            PointForm::Compressed,
        );
        add_record(&mut records, &private, &compressed, CONNECTOR.as_bytes())?;
    }

    std::fs::write(&path, records)?;
    Ok(())
}
