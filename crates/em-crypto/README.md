# em-crypto

Cryptographic primitives: AES-SIV, key derivation, 1905-layer security helpers. Prefer existing Rust crates (e.g. RustCrypto) over porting `util_crypto`, which is under a modified BSD license.

## Upstream source

- `src/util_crypto/aes_siv.c` (modified-BSD, see upstream `src/util_crypto/LICENSE`)
- `src/em/crypto/em_crypto.cpp`, `inc/em_crypto.h`

Base: `rdkcentral/unified-wifi-mesh` @ `ba4180b`.

## Status

not started. See [STATUS.md](../../STATUS.md).
