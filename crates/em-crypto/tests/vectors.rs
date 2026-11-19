// SPDX-License-Identifier: Apache-2.0

//! Published test vectors: RFC 5297 (SIV), RFC 3394 (key wrap), RFC 4231 (HMAC), RFC 5869
//! (HKDF) and RFC 6070 (PBKDF2-HMAC-SHA1).

mod common;

use common::hex;
use em_crypto::hash::{self, HashAlgorithm};
use em_crypto::kdf;
use em_crypto::keywrap;
use em_crypto::siv;

// RFC 5297, appendix A.1 (deterministic authenticated encryption).
#[test]
fn rfc5297_a1_deterministic_siv() {
    let key = hex("fffefdfcfbfaf9f8f7f6f5f4f3f2f1f0f0f1f2f3f4f5f6f7f8f9fafbfcfdfeff");
    let ad = hex("101112131415161718191a1b1c1d1e1f2021222324252627");
    let plain = hex("112233445566778899aabbccddee");
    let expected = hex("85632d07c6e8f37f950acd320a2ecc9340c02b9690c4dc04daef7f6afe5c");

    assert_eq!(
        siv::encrypt(&key, &[&ad], &plain).expect("encrypt"),
        expected
    );
    assert_eq!(
        siv::decrypt(&key, &[&ad], &expected)
            .expect("decrypt")
            .expose(),
        plain.as_slice()
    );
}

// RFC 5297, appendix A.2 (nonce-based: the nonce is the last associated-data piece).
#[test]
fn rfc5297_a2_nonce_siv() {
    let key = hex("7f7e7d7c7b7a79787776757473727170404142434445464748494a4b4c4d4e4f");
    let ad1 =
        hex("00112233445566778899aabbccddeeffdeaddadadeaddadaffeeddccbbaa99887766554433221100");
    let ad2 = hex("102030405060708090a0");
    let nonce = hex("09f911029d74e35bd84156c5635688c0");
    let plain = hex(
        "7468697320697320736f6d6520706c61696e7465787420746f20656e6372797074207573696e67205349562d414553",
    );
    let expected = hex(concat!(
        "7bdb6e3b432667eb06f4d14bff2fbd0fcb900f2fddbe404326601965c889bf17",
        "dba77ceb094fa663b7a3f748ba8af829ea64ad544a272e9c485b62a3fd5c0d",
    ));

    let ad: [&[u8]; 3] = [&ad1, &ad2, &nonce];
    assert_eq!(siv::encrypt(&key, &ad, &plain).expect("encrypt"), expected);
    assert_eq!(
        siv::decrypt(&key, &ad, &expected)
            .expect("decrypt")
            .expose(),
        plain.as_slice()
    );
}

// RFC 3394, section 4: the six example wrappings (4.1 to 4.6), with 128-, 192- and 256-bit KEKs.
#[test]
fn rfc3394_key_wrap_vectors() {
    let kek128 = hex("000102030405060708090a0b0c0d0e0f");
    let kek192 = hex("000102030405060708090a0b0c0d0e0f1011121314151617");
    let kek256 = hex("000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f");
    let data128 = hex("00112233445566778899aabbccddeeff");
    let data192 = hex("00112233445566778899aabbccddeeff0001020304050607");
    let data256 = hex("00112233445566778899aabbccddeeff000102030405060708090a0b0c0d0e0f");

    let cases = [
        (
            &kek128,
            &data128,
            "1fa68b0a8112b447aef34bd8fb5a7b829d3e862371d2cfe5",
        ),
        (
            &kek192,
            &data128,
            "96778b25ae6ca435f92b5b97c050aed2468ab8a17ad84e5d",
        ),
        (
            &kek256,
            &data128,
            "64e8c3f9ce0f5ba263e9777905818a2a93c8191e7d6e8ae7",
        ),
        (
            &kek192,
            &data192,
            "031d33264e15d33268f24ec260743edce1c6c7ddee725a936ba814915c6762d2",
        ),
        (
            &kek256,
            &data192,
            "a8f9bc1612c68b3ff6e6f4fbe30e71e4769c8b80a32cb8958cd5d17d6b254da1",
        ),
        (
            &kek256,
            &data256,
            "28c9f404c4b810f4cbccb35cfb87f8263f5786e2d80ed326cbc7f0e71a99f43bfb988b9b7a02dd21",
        ),
    ];
    for (kek, data, wrapped) in cases {
        let wrapped = hex(wrapped);
        assert_eq!(keywrap::wrap(kek, data).expect("wrap"), wrapped);
        assert_eq!(
            keywrap::unwrap(kek, &wrapped).expect("unwrap").expose(),
            data.as_slice()
        );
    }
}

// RFC 4231, test cases 1, 2 and 6 (key longer than the block size).
#[test]
fn rfc4231_hmac_vectors() {
    let long_key = [0xaa_u8; 131];
    let cases: [(&[u8], &[u8], [&str; 3]); 3] = [
        (
            &[0x0b; 20],
            b"Hi There",
            [
                "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7",
                "afd03944d84895626b0825f4ab46907f15f9dadbe4101ec682aa034c7cebc59cfaea9ea9076ede7f4af152e8b2fa9cb6",
                "87aa7cdea5ef619d4ff0b4241a1d6cb02379f4e2ce4ec2787ad0b30545e17cdedaa833b7d6b8a702038b274eaea3f4e4be9d914eeb61f1702e696c203a126854",
            ],
        ),
        (
            b"Jefe",
            b"what do ya want for nothing?",
            [
                "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843",
                "af45d2e376484031617f78d2b58a6b1b9c7ef464f5a01b47e42ec3736322445e8e2240ca5e69e2c78b3239ecfab21649",
                "164b7a7bfcf819e2e395fbe73b56e0a387bd64222e831fd610270cd7ea2505549758bf75c05a994a6d034f65f8f0e6fdcaeab1a34d4a6b4b636e070a38bce737",
            ],
        ),
        (
            &long_key,
            b"Test Using Larger Than Block-Size Key - Hash Key First",
            [
                "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54",
                "4ece084485813e9088d2c63a041bc5b44f9ef1012a2b588f3cd11f05033ac4c60c2ef6ab4030fe8296248df163f44952",
                "80b24263c7c1a3ebb71493c1dd7be8b49b46d1f41b4aeec1121b013783f8f3526b56d037e05f2598bd0fd2215d6a1e5295e64f73f63f0aec8b915a985d786598",
            ],
        ),
    ];
    let algorithms = [
        HashAlgorithm::Sha256,
        HashAlgorithm::Sha384,
        HashAlgorithm::Sha512,
    ];
    for (key, data, expected) in cases {
        for (algorithm, expected) in algorithms.into_iter().zip(expected) {
            let mac = hash::hmac(algorithm, key, &[data]).expect("hmac");
            assert_eq!(mac.expose(), hex(expected).as_slice(), "{algorithm:?}");
        }
    }
}

// RFC 5869, appendix A.1 to A.3 (SHA-256).
#[test]
fn rfc5869_hkdf_vectors() {
    let ikm1 = [0x0b_u8; 22];
    let salt1: Vec<u8> = (0x00..=0x0c).collect();
    let info1: Vec<u8> = (0xf0..=0xf9).collect();
    let okm1 =
        hex("3cb25f25faacd57a90434f64d0362f2a2d2d0a90cf1a5a4c5db02d56ecc4c5bf34007208d5b887185865");
    let derived = kdf::hkdf(HashAlgorithm::Sha256, &ikm1, &salt1, &info1, 42).expect("hkdf 1");
    assert_eq!(derived.expose(), okm1.as_slice());

    let ikm2: Vec<u8> = (0x00..=0x4f).collect();
    let salt2: Vec<u8> = (0x60..=0xaf).collect();
    let info2: Vec<u8> = (0xb0..=0xff).collect();
    let okm2 = hex(concat!(
        "b11e398dc80327a1c8e7f78c596a49344f012eda2d4efad8a050cc4c19afa97c",
        "59045a99cac7827271cb41c65e590e09da3275600c2f09b8367793a9aca3db71",
        "cc30c58179ec3e87c14c01d5c1f3434f1d87",
    ));
    let derived = kdf::hkdf(HashAlgorithm::Sha256, &ikm2, &salt2, &info2, 82).expect("hkdf 2");
    assert_eq!(derived.expose(), okm2.as_slice());

    // Test case 3: empty salt and info.
    let okm3 =
        hex("8da4e775a563c18f715f802a063c5a31b8a11f5c5ee1879ec3454e5f3c738d2d9d201395faa4b61a96c8");
    let derived = kdf::hkdf(HashAlgorithm::Sha256, &ikm1, &[], &[], 42).expect("hkdf 3");
    assert_eq!(derived.expose(), okm3.as_slice());
}

struct Pbkdf2Case {
    password: &'static [u8],
    salt: &'static [u8],
    iterations: u32,
    length: usize,
    expected: &'static str,
}

// RFC 6070 (the 16,777,216-iteration vector is skipped as too slow).
#[test]
fn rfc6070_pbkdf2_vectors() {
    let cases = [
        Pbkdf2Case {
            password: b"password",
            salt: b"salt",
            iterations: 1,
            length: 20,
            expected: "0c60c80f961f0e71f3a9b524af6012062fe037a6",
        },
        Pbkdf2Case {
            password: b"password",
            salt: b"salt",
            iterations: 2,
            length: 20,
            expected: "ea6c014dc72d6f8ccd1ed92ace1d41f0d8de8957",
        },
        Pbkdf2Case {
            password: b"password",
            salt: b"salt",
            iterations: 4096,
            length: 20,
            expected: "4b007901b765489abead49d926f721d065a429c1",
        },
        Pbkdf2Case {
            password: b"passwordPASSWORDpassword",
            salt: b"saltSALTsaltSALTsaltSALTsaltSALTsalt",
            iterations: 4096,
            length: 25,
            expected: "3d2eec4fe41c849b80c8d83662c0e44a8b291a964cf2f07038",
        },
        Pbkdf2Case {
            password: b"pass\0word",
            salt: b"sa\0lt",
            iterations: 4096,
            length: 16,
            expected: "56fa6aa75548099dcc37d7f03425e0c3",
        },
    ];
    for case in cases {
        let derived = kdf::pbkdf2_hmac_sha1(case.password, case.salt, case.iterations, case.length)
            .expect("pbkdf2");
        assert_eq!(
            derived.expose(),
            hex(case.expected).as_slice(),
            "{} iterations",
            case.iterations
        );
    }
}
