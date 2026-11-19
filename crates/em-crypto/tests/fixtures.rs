// SPDX-License-Identifier: Apache-2.0

//! Consumes every row of the upstream oracle fixture (`data/upstream_crypto_vectors.tsv`).
//!
//! The dispatcher works per (kind, case): an unknown kind or case, a column or field that a
//! handler does not read, or a different row count fails the test. Rust outcomes are asserted
//! equal to upstream's except where note 0006 D3 declares a difference (CBC decryption, DH
//! peer range, the 8192-byte 802.11 KDF, and the P-192/P-224 refusals); those rows carry the
//! specified Rust outcome in their own fields.

mod common;

use common::{Row, hex, row, rows};
use em_crypto::Error;
use em_crypto::cbc;
use em_crypto::dh::{DH_VALUE_LEN, DhPrivate};
use em_crypto::ec::{self, Curve, Point, PointForm, PrivateKey, PublicKey, Scalar};
use em_crypto::encoding::{base64_decode, base64_encode, base64url_decode, base64url_encode};
use em_crypto::hash::{self, HashAlgorithm};
use em_crypto::kdf;
use em_crypto::keywrap;
use em_crypto::siv;

const EXPECTED_ROWS: usize = 155;

fn algorithm_from_case(case: &str) -> HashAlgorithm {
    if case.starts_with("sha256") {
        HashAlgorithm::Sha256
    } else if case.starts_with("sha384") {
        HashAlgorithm::Sha384
    } else if case.starts_with("sha512") {
        HashAlgorithm::Sha512
    } else {
        panic!("case {case} names no hash")
    }
}

fn pieces_as_slices(pieces: &[Vec<u8>]) -> Vec<&[u8]> {
    pieces.iter().map(Vec::as_slice).collect()
}

fn text(bytes: &[u8]) -> &str {
    std::str::from_utf8(bytes).expect("fixture text is ASCII")
}

#[test]
fn every_fixture_row_is_consumed() {
    let all_rows = rows();
    assert_eq!(all_rows.len(), EXPECTED_ROWS);

    let mut consumed = 0;
    for mut row in all_rows {
        match row.kind.as_str() {
            "kdf80211" => kdf80211(&mut row),
            "wps" => wps(&mut row),
            "hkdf" => hkdf(&mut row),
            "pbkdf2" => pbkdf2(&mut row),
            "hash" => hash_row(&mut row),
            "hmac" => hmac_row(&mut row),
            "cbc" => cbc_row(&mut row),
            "keywrap" => keywrap_row(&mut row),
            "siv" => siv_row(&mut row),
            "dh" => dh_row(&mut row),
            "base64" | "base64url" => encoding_row(&mut row),
            "wsc" => wsc_row(&mut row),
            "spki" => spki_row(&mut row),
            "bootkey" => bootkey_row(&mut row),
            "ecdh" => ecdh_row(&mut row),
            "ec-point" => ec_point_row(&mut row),
            "ec-scalar" => ec_scalar_row(&mut row),
            "signature" => signature_row(&mut row),
            other => panic!("fixture kind {other} has no handler"),
        }
        row.assert_consumed();
        consumed += 1;
    }
    assert_eq!(consumed, EXPECTED_ROWS);
}

fn kdf80211(row: &mut Row) {
    let algorithm = algorithm_from_case(&row.case);
    let key = row.inputs.bytes("key");
    let label = hex(row.inputs.get("label"));
    let context = row.inputs.bytes("context");
    let length = row.inputs.number("length");
    let result = kdf::kdf_80211(algorithm, &key, text(&label), &context, length);
    assert_eq!(row.upstream_return(), "1", "{}", row.name());

    match row.case.as_str() {
        "sha256" | "sha384" | "sha512" => {
            let expected = row.output_hex();
            assert_eq!(result.expect("kdf").expose(), expected.as_slice());
        }
        // 8191 bytes: both succeed with the same bytes.
        "sha256-8191" => {
            let fields = row.output_fields();
            let upstream = fields.bytes("upstream");
            assert_eq!(fields.get("rust"), "accept");
            assert_eq!(upstream.len(), 8191);
            assert_eq!(result.expect("kdf").expose(), upstream.as_slice());
        }
        // 8192 bytes: upstream succeeds with a truncated 16-bit length field, Rust refuses.
        "sha256-8192" => {
            let fields = row.output_fields();
            let upstream = fields.bytes("upstream");
            assert_eq!(fields.get("rust"), "reject");
            assert_eq!(upstream.len(), 8192);
            assert_eq!(result.expect_err("kdf"), Error::OutputLength);
        }
        other => panic!("unknown kdf80211 case {other}"),
    }
}

fn wps(row: &mut Row) {
    match row.case.as_str() {
        "prefix-80" => {
            let key = row.inputs.bytes("key");
            let prefix = row.inputs.bytes("prefix");
            let label = hex(row.inputs.get("label"));
            let length = row.inputs.number("length");
            assert_eq!(row.upstream_return(), "1");
            let expected = row.output_hex();
            let derived = kdf::wps_kdf(&key, &prefix, text(&label), length).expect("wps kdf");
            assert_eq!(derived.expose(), expected.as_slice());
        }
        other => panic!("unknown wps case {other}"),
    }
}

fn hkdf(row: &mut Row) {
    // Each hash with a short output, its 255-block limit, and one byte over.
    assert!(
        matches!(
            row.case.as_str(),
            "sha256-42"
                | "sha256-8160"
                | "sha256-8161"
                | "sha384-42"
                | "sha384-12240"
                | "sha384-12241"
                | "sha512-42"
                | "sha512-16320"
                | "sha512-16321"
        ),
        "unknown hkdf case {}",
        row.case
    );
    let algorithm = algorithm_from_case(&row.case);
    let ikm = row.inputs.bytes("ikm");
    let salt = row.inputs.bytes("salt");
    let info = row.inputs.bytes("info");
    let length = row.inputs.number("length");
    let result = kdf::hkdf(algorithm, &ikm, &salt, &info, length);
    // Upstream returns the output length, or 0 when it refuses (more than 255 blocks).
    let upstream_return: usize = row.upstream_return().parse().expect("numeric return");
    let expected = row.output_hex();
    if upstream_return == 0 {
        assert_eq!(expected, Vec::<u8>::new());
        assert_eq!(
            result.expect_err("hkdf over 255 blocks"),
            Error::OutputLength
        );
    } else {
        assert_eq!(upstream_return, length);
        assert_eq!(result.expect("hkdf").expose(), expected.as_slice());
    }
}

fn pbkdf2(row: &mut Row) {
    match row.case.as_str() {
        "gen-psk" => {
            let password = row.inputs.bytes("pass");
            let ssid = row.inputs.bytes("ssid");
            let iterations = u32::try_from(row.inputs.number("iterations")).expect("iterations");
            let length = row.inputs.number("length");
            assert_eq!(row.upstream_return(), "1");
            let expected = row.output_hex();
            let derived =
                kdf::pbkdf2_hmac_sha1(&password, &ssid, iterations, length).expect("pbkdf2");
            assert_eq!(derived.expose(), expected.as_slice());
        }
        other => panic!("unknown pbkdf2 case {other}"),
    }
}

fn hash_row(row: &mut Row) {
    match row.case.as_str() {
        "sha256-split" | "sha256-concat" | "sha384-split" | "sha384-concat" | "sha512-split"
        | "sha512-concat" => {
            let pieces = row.inputs.pieces("pieces");
            assert_eq!(row.upstream_return(), "1");
            let expected = row.output_hex();
            let digest = hash::digest(algorithm_from_case(&row.case), &pieces_as_slices(&pieces));
            assert_eq!(digest.expose(), expected.as_slice());
        }
        other => panic!("unknown hash case {other}"),
    }
}

fn hmac_row(row: &mut Row) {
    match row.case.as_str() {
        "sha256-split" | "sha256-concat" | "sha384-split" | "sha384-concat" | "sha512-split"
        | "sha512-concat" => {
            let key = row.inputs.bytes("key");
            let pieces = row.inputs.pieces("pieces");
            assert_eq!(row.upstream_return(), "1");
            let expected = row.output_hex();
            let mac = hash::hmac(
                algorithm_from_case(&row.case),
                &key,
                &pieces_as_slices(&pieces),
            )
            .expect("hmac");
            assert_eq!(mac.expose(), expected.as_slice());
        }
        other => panic!("unknown hmac case {other}"),
    }
}

fn block_array(bytes: &[u8]) -> [u8; 16] {
    bytes.try_into().expect("16-byte value")
}

fn cbc_row(row: &mut Row) {
    let key = block_array(&row.inputs.bytes("key"));
    let iv = block_array(&row.inputs.bytes("iv"));
    match row.case.as_str() {
        "empty" | "aligned" | "non-aligned" => {
            let plain = row.inputs.bytes("plain");
            // Upstream returns the length of the plaintext it claims to have produced.
            let upstream_len: usize = row.upstream_return().parse().expect("numeric return");
            let fields = row.output_fields();
            assert_eq!(fields.get("encrypt"), "1");
            let cipher = fields.bytes("cipher");
            let span = fields.bytes("span");
            assert_eq!(fields.get("rust"), "accept");
            let rust_plain = fields.bytes("rust_plain");

            // Encryption is byte-identical to upstream.
            assert_eq!(cbc::encrypt(&key, &iv, &plain), cipher);
            // Rust returns the complete plaintext.
            assert_eq!(rust_plain, plain);
            assert_eq!(
                cbc::decrypt(&key, &iv, &cipher).expect("decrypt").expose(),
                plain.as_slice()
            );

            assert_eq!(upstream_len, plain.len());
            assert_eq!(span.len(), upstream_len);
            let tail_len = plain.len() % 16;
            if tail_len == 0 {
                assert_eq!(span, plain, "aligned and empty spans match");
            } else {
                // Upstream's defect: the last `len % 16` bytes stay as ciphertext.
                let head_len = plain.len() - tail_len;
                assert_eq!(span[..head_len], plain[..head_len]);
                let last_block = cipher.len() - 16;
                assert_eq!(span[head_len..], cipher[last_block..last_block + tail_len]);
                assert_ne!(span, plain, "this fixture must expose the upstream defect");
            }
        }
        "malformed-padding" => {
            let cipher = row.inputs.bytes("cipher");
            // Upstream returns 0 for errors (and for an empty plaintext).
            assert_eq!(row.upstream_return(), "0");
            let fields = row.output_fields();
            assert_eq!(fields.bytes("span"), Vec::<u8>::new());
            assert_eq!(fields.get("rust"), "reject");
            assert_eq!(fields.bytes("rust_plain"), Vec::<u8>::new());
            assert_eq!(
                cbc::decrypt(&key, &iv, &cipher).expect_err("padding"),
                Error::InvalidPadding
            );
        }
        other => panic!("unknown cbc case {other}"),
    }
}

fn keywrap_row(row: &mut Row) {
    let (operation, bits) = row.case.rsplit_once('-').expect("operation-bits case");
    let kek = row.inputs.bytes("kek");
    assert_eq!(kek.len() * 8, bits.parse::<usize>().expect("bits"));
    let succeeds = row.upstream_return() == "1";
    let expected = row.output_hex();
    match operation {
        "wrap" => {
            let plain = row.inputs.bytes("plain");
            assert_eq!(keywrap::wrap(&kek, &plain).expect("wrap"), expected);
        }
        "unwrap" => {
            let wrapped = row.inputs.bytes("wrapped");
            assert_eq!(
                keywrap::unwrap(&kek, &wrapped).expect("unwrap").expose(),
                expected.as_slice()
            );
        }
        "tampered" => {
            let wrapped = row.inputs.bytes("wrapped");
            assert_eq!(
                keywrap::unwrap(&kek, &wrapped).expect_err("tampered"),
                Error::AuthenticationFailed
            );
        }
        "wrap-short" | "wrap-non-multiple" => {
            let plain = row.inputs.bytes("plain");
            assert_eq!(
                keywrap::wrap(&kek, &plain).expect_err("wrap input"),
                Error::InvalidInputLength
            );
        }
        "unwrap-short" | "unwrap-non-multiple" => {
            let wrapped = row.inputs.bytes("wrapped");
            assert_eq!(
                keywrap::unwrap(&kek, &wrapped).expect_err("unwrap input"),
                Error::InvalidInputLength
            );
        }
        other => panic!("unknown keywrap operation {other}"),
    }
    let rejected = matches!(
        operation,
        "tampered" | "wrap-short" | "wrap-non-multiple" | "unwrap-short" | "unwrap-non-multiple"
    );
    assert_eq!(succeeds, !rejected, "{}: upstream outcome", row.name());
    if rejected {
        assert_eq!(expected, Vec::<u8>::new());
    }
}

fn siv_row(row: &mut Row) {
    // `operation-bits`, or `operation-bits-ad_shape-plaintext_length` for the DPP wrapper shapes.
    let mut parts = row.case.split('-');
    let operation = parts.next().expect("operation");
    let bits = parts.next().expect("bits");
    let shape: Vec<&str> = parts.collect();
    let key = row.inputs.bytes("key");
    assert_eq!(key.len() * 8, bits.parse::<usize>().expect("bits"));
    let associated_data = row.inputs.pieces("ad");
    let ad = pieces_as_slices(&associated_data);
    match shape.as_slice() {
        [] => assert_eq!(ad.len(), 2, "{}", row.name()),
        ["ad0", _] => assert!(ad.is_empty(), "{}", row.name()),
        ["ad1", _] => assert!(ad.len() == 1 && !ad[0].is_empty(), "{}", row.name()),
        ["adempty", _] => assert!(ad.len() == 1 && ad[0].is_empty(), "{}", row.name()),
        _ => panic!("unknown siv shape {}", row.case),
    }
    match operation {
        "encrypt" => {
            let plain = row.inputs.bytes("plain");
            if let [_, length] = shape.as_slice() {
                assert_eq!(format!("p{}", plain.len()), *length);
            }
            assert_eq!(row.upstream_return(), "1");
            let fields = row.output_fields();
            let mut expected = fields.bytes("tag");
            expected.extend_from_slice(&fields.bytes("cipher"));
            assert_eq!(
                siv::encrypt(&key, &ad, &plain).expect("siv encrypt"),
                expected
            );
        }
        "decrypt" => {
            let mut tag_and_cipher = row.inputs.bytes("tag");
            tag_and_cipher.extend_from_slice(&row.inputs.bytes("cipher"));
            assert_eq!(row.upstream_return(), "1");
            let expected = row.output_hex();
            let plain = siv::decrypt(&key, &ad, &tag_and_cipher).expect("siv decrypt");
            assert_eq!(plain.expose(), expected.as_slice());
        }
        "tampered" => {
            let mut tag_and_cipher = row.inputs.bytes("tag");
            tag_and_cipher.extend_from_slice(&row.inputs.bytes("cipher"));
            // Upstream returns -1 and leaves a zeroed buffer; Rust returns an error.
            assert_eq!(row.upstream_return(), "-1");
            let upstream_buffer = row.output_hex();
            assert!(!upstream_buffer.is_empty() && upstream_buffer.iter().all(|&b| b == 0));
            assert_eq!(
                siv::decrypt(&key, &ad, &tag_and_cipher).expect_err("tampered"),
                Error::AuthenticationFailed
            );
        }
        other => panic!("unknown siv operation {other}"),
    }
}

fn dh_row(row: &mut Row) {
    if row.case == "public-padded-8" {
        let generator = row.inputs.bytes("generator");
        let private = row.inputs.bytes("private");
        let width = row.inputs.number("width");
        let upstream_width: usize = row.upstream_return().parse().expect("numeric return");
        let expected = row.output_hex();
        assert_eq!(generator, [2]);
        assert_eq!(width, DH_VALUE_LEN);
        assert_eq!(upstream_width, DH_VALUE_LEN);
        let public = DhPrivate::from_be_bytes(&private)
            .expect("private")
            .public_key();
        assert_eq!(public.as_bytes().as_slice(), expected.as_slice());
        return;
    }

    let private = row.inputs.bytes("private");
    let peer = row.inputs.bytes("peer");
    let upstream_accepts = row.upstream_return() == "1";
    let fields = row.output_fields();
    let upstream = fields.bytes("upstream");
    let rust_accepts = fields.get("rust") == "accept";
    let rust_secret = fields.bytes("rust_secret");
    let result = DhPrivate::from_be_bytes(&private)
        .expect("fixture exponent")
        .shared_secret(&peer);

    match row.case.as_str() {
        // Out of range: Rust rejects, and so does upstream (OpenSSL 3).
        "zero" | "one" | "p-minus-1" | "p" | "p-plus-1" => {
            assert!(!upstream_accepts && !rust_accepts);
            assert!(upstream.is_empty() && rust_secret.is_empty());
            assert_eq!(result.expect_err("peer range"), Error::ValueOutOfRange);
        }
        // In range: both accept and derive the same bytes (no leading zeros).
        "p-minus-2" | "two" | "leading-zero-exp-8" | "leading-zero-exp-17" | "fixed-agreement" => {
            assert!(upstream_accepts && rust_accepts);
            assert_eq!(upstream, rust_secret);
            assert_eq!(result.expect("dh").expose(), rust_secret.as_slice());
        }
        other => panic!("unknown dh case {other}"),
    }
    if row.case == "p-minus-2" {
        // Independent expectation: (p - 2)^3 mod p = p - 8, i.e. the peer value with its last
        // byte (0xfd) lowered by 6.
        let mut p_minus_8 = peer;
        assert_eq!(p_minus_8.len(), DH_VALUE_LEN);
        assert_eq!(p_minus_8[DH_VALUE_LEN - 1], 0xfd);
        p_minus_8[DH_VALUE_LEN - 1] -= 6;
        assert_eq!(rust_secret, p_minus_8);
        assert_eq!(upstream, p_minus_8);
    }
}

fn encoding_row(row: &mut Row) {
    let input = row.inputs.bytes("input");
    match (row.kind.as_str(), row.case.as_str()) {
        ("base64", "empty" | "man" | "alphabet") => {
            assert_eq!(row.upstream_return(), "1");
            let fields = row.output_fields();
            let encoded = hex(fields.get("encoded"));
            let decoded = fields.bytes("decoded");
            assert_eq!(base64_encode(&input).as_bytes(), encoded.as_slice());
            assert_eq!(base64_decode(text(&encoded)).expect("decode"), decoded);
            assert_eq!(decoded, input);
        }
        ("base64url", "encode-empty" | "encode-man" | "encode-alphabet") => {
            assert_eq!(row.upstream_return(), "1");
            let fields = row.output_fields();
            let encoded = hex(fields.get("encoded"));
            let decoded = fields.bytes("decoded");
            assert_eq!(base64url_encode(&input).as_bytes(), encoded.as_slice());
            assert_eq!(base64url_decode(text(&encoded)).expect("decode"), decoded);
            assert_eq!(decoded, input);
        }
        (
            "base64url",
            "empty" | "missing-padding" | "existing-padding" | "url-alphabet" | "standard-alphabet"
            | "mixed-alphabets" | "invalid-length" | "malformed-padding" | "embedded-nul",
        ) => {
            // The decode rows give the exact input bytes and their length (embedded NUL).
            assert_eq!(row.inputs.number("length"), input.len());
            let upstream_succeeds = row.upstream_return() == "1";
            let expected = row.output_hex();
            let result = base64url_decode(text(&input));
            if upstream_succeeds {
                assert_eq!(result.expect("decode"), expected);
            } else {
                assert_eq!(expected, Vec::<u8>::new());
                assert_eq!(result.expect_err("decode"), Error::InvalidEncoding);
            }
        }
        (kind, case) => panic!("unknown encoding row {kind}/{case}"),
    }
}

fn wsc_row(row: &mut Row) {
    match row.case.as_str() {
        "dh-secret" => {
            let private = row.inputs.bytes("private");
            let peer = row.inputs.bytes("peer");
            assert_eq!(row.upstream_return(), "1");
            let expected = row.output_hex();
            let secret = DhPrivate::from_be_bytes(&private)
                .expect("private")
                .shared_secret(&peer);
            assert_eq!(secret.expect("dh").expose(), expected.as_slice());
        }
        "dh-hash" => {
            let secret = row.inputs.bytes("secret");
            assert_eq!(row.upstream_return(), "1");
            let expected = row.output_hex();
            let digest = hash::digest(HashAlgorithm::Sha256, &[&secret]);
            assert_eq!(digest.expose(), expected.as_slice());
        }
        "kdk" => {
            let dhkey = row.inputs.bytes("dhkey");
            let enrollee_nonce = row.inputs.bytes("enrollee_nonce");
            let mac = row.inputs.bytes("mac");
            let registrar_nonce = row.inputs.bytes("registrar_nonce");
            assert_eq!(row.upstream_return(), "1");
            let expected = row.output_hex();
            let kdk = hash::hmac(
                HashAlgorithm::Sha256,
                &dhkey,
                &[&enrollee_nonce, &mac, &registrar_nonce],
            )
            .expect("hmac");
            assert_eq!(kdk.expose(), expected.as_slice());
        }
        "keys" => {
            let kdk = row.inputs.bytes("kdk");
            let prefix = row.inputs.bytes("prefix");
            let label = hex(row.inputs.get("label"));
            let length = row.inputs.number("length");
            assert_eq!(row.upstream_return(), "1");
            let fields = row.output_fields();
            let all = fields.bytes("all");
            // The caller-side split is exercised in wsc.rs; here the parts must tile `all`.
            assert_eq!(all.len(), 80);
            assert_eq!(fields.bytes("auth"), all[..32]);
            assert_eq!(fields.bytes("wrap"), all[32..48]);
            assert_eq!(fields.bytes("emsk"), all[48..]);
            let derived = kdf::wps_kdf(&kdk, &prefix, text(&label), length).expect("wps kdf");
            assert_eq!(derived.expose(), all.as_slice());
        }
        other => panic!("unknown wsc case {other}"),
    }
}

/// The curve with exactly this name (`p256`, `p384` or `p521`).
fn curve_by_name(name: &str) -> Curve {
    match name {
        "p256" => Curve::P256,
        "p384" => Curve::P384,
        "p521" => Curve::P521,
        other => panic!("{other} is not a supported curve"),
    }
}

/// The curve a case name starts with (`p256`, `p384-add`, ...).
fn curve_from_case(case: &str) -> Curve {
    curve_by_name(case.split('-').next().expect("case name"))
}

fn spki_row(row: &mut Row) {
    let curve_name = row.inputs.get("curve").to_string();
    let der = row.inputs.bytes("der");
    assert_eq!(row.upstream_return(), "1");
    let fields = row.output_fields();
    assert_eq!(fields.bytes("upstream"), der);
    let imported = PublicKey::from_spki_der(&der);

    match (row.case.as_str(), fields.get("rust")) {
        ("p192" | "p224", "unsupported-curve") => {
            assert_eq!(curve_name, row.case);
            assert_ne!(row.inputs.bytes("private"), Vec::<u8>::new());
            assert_eq!(
                imported.expect_err("refused curve"),
                Error::UnsupportedCurve
            );
        }
        ("p256" | "p384" | "p521", "accept") => {
            let curve = curve_from_case(&row.case);
            let private = PrivateKey::from_be_bytes(curve, &row.inputs.bytes("private"))
                .expect("fixed private key");
            let key = imported.expect("import");
            assert_eq!(key.curve(), curve);
            assert_eq!(key.form(), PointForm::Uncompressed);
            // Export reproduces the upstream bytes, and so does deriving the public key.
            assert_eq!(key.to_spki_der().expect("export"), der);
            assert_eq!(private.public_key().expect("public key"), key);
        }
        ("uri-compressed-p256", "accept") => {
            // The point form of the imported key is kept when exporting.
            assert_eq!(curve_name, "p256");
            assert_eq!(row.inputs.get("source"), "ec_util.cpp:760");
            let key = imported.expect("import");
            assert_eq!(key.curve(), Curve::P256);
            assert_eq!(key.form(), PointForm::Compressed);
            assert_eq!(key.to_spki_der().expect("export"), der);
            // The same point exported uncompressed differs from the compressed bytes.
            let point = Point::from_xy(Curve::P256, key.point().as_xy()).expect("point");
            let uncompressed = PublicKey::new(point, PointForm::Uncompressed)
                .to_spki_der()
                .expect("export");
            assert_ne!(uncompressed, der);
            assert_eq!(
                PublicKey::from_spki_der(&uncompressed).expect("reimport"),
                PublicKey::new(
                    Point::from_xy(Curve::P256, key.point().as_xy()).expect("point"),
                    PointForm::Uncompressed
                )
            );
        }
        (case, rust) => panic!("unknown spki row {case} with outcome {rust}"),
    }
}

fn bootkey_row(row: &mut Row) {
    let der = row.inputs.bytes("der");
    let prefix_bytes = row.inputs.bytes("prefix");
    let prefix = (!prefix_bytes.is_empty()).then(|| text(&prefix_bytes));
    assert_eq!(row.upstream_return(), "1");
    let expected = row.output_hex();
    let result = ec::boot_key_hash(&der, prefix);

    let (name, variant) = row.case.rsplit_once('-').expect("name-variant case");
    assert_eq!(prefix.is_some(), variant == "prefixed");
    match name {
        "p192" | "p224" => assert_eq!(result.expect_err("refused curve"), Error::UnsupportedCurve),
        "p256" | "p384" | "p521" | "uri-compressed-p256" => {
            assert_eq!(result.expect("boot key hash").as_slice(), expected);
        }
        other => panic!("unknown bootkey case {other}"),
    }
}

fn ecdh_row(row: &mut Row) {
    let curve = curve_by_name(&row.case);
    let private = PrivateKey::from_be_bytes(curve, &row.inputs.bytes("private")).expect("private");
    let peer = Point::from_xy(curve, &row.inputs.bytes("peer_xy")).expect("peer");
    assert_eq!(row.upstream_return(), "1");
    let expected = row.output_hex();
    assert_eq!(expected.len(), curve.coordinate_len());
    assert_eq!(private.ecdh(&peer).expect("ecdh").expose(), expected);
}

fn ec_point_row(row: &mut Row) {
    let (curve_name, operation) = row.case.split_once('-').expect("curve-operation case");
    let curve = curve_by_name(curve_name);
    match operation {
        "multiply" => {
            let scalar = Scalar::from_be_bytes(curve, &row.inputs.bytes("scalar")).expect("scalar");
            let point = Point::from_xy(curve, &row.inputs.bytes("point_xy")).expect("point");
            assert_eq!(row.upstream_return(), "1");
            let expected = row.output_hex();
            assert_eq!(point.multiply(&scalar).expect("multiply").as_xy(), expected);
        }
        "add" => {
            let left = Point::from_xy(curve, &row.inputs.bytes("left_xy")).expect("left");
            let right = Point::from_xy(curve, &row.inputs.bytes("right_xy")).expect("right");
            assert_eq!(row.upstream_return(), "1");
            let expected = row.output_hex();
            assert_eq!(left.add(&right).expect("add").as_xy(), expected);
        }
        "invalid" => {
            // The all-zero point is not on the curve: upstream returns 0, Rust refuses it.
            let xy = row.inputs.bytes("point_xy");
            assert_eq!(xy.len(), curve.point_len());
            assert_eq!(row.upstream_return(), "0");
            assert_eq!(row.output_hex(), Vec::<u8>::new());
            assert_eq!(
                Point::from_xy(curve, &xy).expect_err("off-curve"),
                Error::InvalidPoint
            );
        }
        other => panic!("unknown ec-point operation {other}"),
    }
}

fn ec_scalar_row(row: &mut Row) {
    let curve = curve_by_name(&row.case);
    let left = Scalar::from_be_bytes(curve, &row.inputs.bytes("left")).expect("left");
    let right = Scalar::from_be_bytes(curve, &row.inputs.bytes("right")).expect("right");
    let order = row.inputs.bytes("order");
    assert_eq!(row.upstream_return(), "1");
    let expected = row.output_hex();
    // The group order itself is not a scalar; `left` is `order - 1`.
    assert_eq!(
        Scalar::from_be_bytes(curve, &order).expect_err("order"),
        Error::InvalidScalar
    );
    assert_eq!(
        left.add(&right).expect("add").to_be_bytes().expose(),
        expected
    );
}

fn signature_row(row: &mut Row) {
    let curve = curve_by_name(&row.case);
    let mut message = row.inputs.bytes("message");
    let spki = row.inputs.bytes("spki");
    assert_eq!(row.inputs.get("hash"), "sha256");
    assert_eq!(row.upstream_return(), "1");
    let signature = row.output_hex();

    let key = PublicKey::from_spki_der(&spki).expect("import");
    assert_eq!(key.curve(), curve);
    key.verify(&message, &signature)
        .expect("upstream signature");

    // Flip one message bit, expect rejection, flip it back.
    message[0] ^= 1;
    assert_eq!(
        key.verify(&message, &signature)
            .expect_err("changed message"),
        Error::InvalidSignature
    );
    message[0] ^= 1;
    let mut changed_signature = signature;
    *changed_signature.last_mut().expect("signature") ^= 1;
    assert_eq!(
        key.verify(&message, &changed_signature)
            .expect_err("changed signature"),
        Error::InvalidSignature
    );
}

/// A row of `kind` whose case is renamed to `case`, to probe the handlers' case matching.
fn renamed_row(kind: &str, original_case: &str, case: &str) -> Row {
    let mut renamed = row(kind, original_case);
    renamed.case = case.to_string();
    renamed
}

#[test]
#[should_panic(expected = "not a supported curve")]
fn ecdh_case_must_match_exactly() {
    ecdh_row(&mut renamed_row("ecdh", "p256", "p256-extra"));
}

#[test]
#[should_panic(expected = "not a supported curve")]
fn ec_scalar_case_must_match_exactly() {
    ec_scalar_row(&mut renamed_row("ec-scalar", "p256", "p256-extra"));
}

#[test]
#[should_panic(expected = "not a supported curve")]
fn signature_case_must_match_exactly() {
    signature_row(&mut renamed_row("signature", "p256", "p256-extra"));
}

#[test]
#[should_panic(expected = "unknown ec-point operation")]
fn ec_point_case_must_match_exactly() {
    ec_point_row(&mut renamed_row("ec-point", "p256-add", "p256-x-add"));
}

#[test]
#[should_panic(expected = "not a supported curve")]
fn ec_point_curve_must_match_exactly() {
    ec_point_row(&mut renamed_row("ec-point", "p256-add", "p256x-add"));
}
