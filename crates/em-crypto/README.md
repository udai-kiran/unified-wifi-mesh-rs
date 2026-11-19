# em-crypto

Cryptographic primitives for the unified-wifi-mesh port: AES-SIV, AES key wrap, AES-128-CBC,
hashes and HMAC, the 802.11/WPS/HKDF/PBKDF2 key derivations, DH group 5, base64 and base64url,
and EC (P-256/384/521), SPKI and ECDSA. The crate is stateless and synchronous, with no `unsafe`
code and no clones. Design: [`docs/0006-em-crypto-scope.md`](../../docs/0006-em-crypto-scope.md).

Upstream: `rdkcentral/unified-wifi-mesh` @ `ba4180b`
(`src/em/crypto/em_crypto.cpp`, `inc/em_crypto.h`, `src/util_crypto/aes_siv.c`). AES-SIV is an
independent implementation on RustCrypto, so the third-party licence of `aes_siv.c` does not apply.

## Status

Complete for note 0006 step 2: `error`, `secret`, `rand`, `ct`, `hash`, `kdf`, `cbc`, `keywrap`,
`siv`, `encoding`, `dh` and `ec`. `tests/fixtures.rs` consumes all 155 rows of the oracle
fixture. See [STATUS.md](../../STATUS.md).

## Modules

| Module | Contents |
|---|---|
| `rand` | `RandomSource` (fallible), `OsRandom` (getrandom), `FailingRandom` (tests), `random_secret`, `Nonce` |
| `secret` | `SecretBytes`, `SecretArray<N>`: wiped on drop, no `Clone`/`Copy`, redacted `Debug` |
| `ct` | `eq`: constant-time equality; a length mismatch is a mismatch |
| `hash` | `digest` and `hmac` over a list of pieces, SHA-256/384/512 |
| `kdf` | `kdf_80211`, `wps_kdf`, `hkdf`, `pbkdf2_hmac_sha1` |
| `cbc` | AES-128-CBC with PKCS#7; decryption returns the whole plaintext |
| `keywrap` | RFC 3394 with a 16/24/32-byte KEK; `unwrap` checks the integrity value in constant time |
| `siv` | RFC 5297 with ordered associated data, 32/48/64-byte keys, `tag || ciphertext` |
| `encoding` | base64; base64url (strict encode, lenient decode) |
| `dh` | group 5: `DhPrivate`, `DhPublic` (`from_be_bytes` for a peer value), 192-byte values |
| `ec` | `Curve`, `Point` (fixed-width X and Y), `Scalar`, `PrivateKey`, `PublicKey` (SPKI, ECDSA), `boot_key_hash` |

Rust replacements of `em_base.h` types: `em_nonce_t` is `rand::Nonce` (16 bytes, public, `Copy`;
use `rand::random_secret::<16>` for a nonce that must stay secret) and `em_dh5_key_t` is
`dh::DhPublic` (192 bytes, big-endian, left-padded) with `dh::DhPrivate` for the exponent.

## Pinned dependencies

Every version is pinned with `=` in `Cargo.toml`. The set is chosen to build with `--locked` on
Rust 1.85.0, the workspace MSRV. `elliptic-curve` 0.14 and `crypto-bigint` 0.7 need Rust 1.87 and
are not used.

| Crate | Version | Features (default features off unless noted) |
|---|---|---|
| aes | 0.8.4 | default, `zeroize` |
| aes-siv | 0.7.0 | `alloc` (the `getrandom` default is off) |
| aes-kw | 0.2.1 | `alloc` (wrap only; unwrap is implemented here on `aes`) |
| cbc | 0.1.2 | `alloc`, `block-padding`, `zeroize` |
| sha1 | 0.10.6 | none |
| sha2 | 0.10.9 | none |
| hmac | 0.12.1 | none |
| hkdf | 0.12.4 | none |
| pbkdf2 | 0.12.2 | `hmac` |
| elliptic-curve | 0.13.8 | `alloc`, `arithmetic`, `sec1`, `pkcs8`, `ecdh` |
| p256 | 0.13.2 | `alloc`, `arithmetic`, `ecdsa`, `ecdh`, `pkcs8` |
| p384 | 0.13.1 | `alloc`, `arithmetic`, `ecdsa`, `ecdh`, `pkcs8` |
| p521 | 0.13.3 | `alloc`, `arithmetic`, `ecdsa`, `ecdh`, `pkcs8` |
| ecdsa | 0.16.9 | `alloc`, `der`, `signing`, `verifying` |
| spki | 0.7.3 | `alloc` |
| der | 0.7.10 | `alloc` |
| crypto-bigint | 0.5.5 | `zeroize` |
| base64ct | 1.6.0 | `alloc` |
| subtle | 2.6.1 | none |
| zeroize | 1.8.1 | `alloc` |
| getrandom | 0.2.15 | none |

`rand_core` is not a direct dependency; it comes in through `elliptic-curve` and is built without its
`getrandom` or `std` features, so no `OsRng` is compiled. The only OS randomness is
`rand::OsRandom`.

## Random numbers

Every key, nonce, IV, DH exponent, EC scalar and ECDSA `k` comes from a `RandomSource`; a failing
source is an `Error::Random`. 0006 asks for failure propagation "through the EC crates' RNG
integration"; this crate meets that by not using that integration: the curve crates'
`random` constructors are never called, `OsRng` is not compiled, and scalars are sampled from a
`RandomSource` by rejection sampling (`DhPrivate::generate` does the same for DH, with at most 64
draws). Tests inject `FailingRandom` into every generator, including EC key generation and
ECDSA signing.
EC scalars are drawn as one field-width big-endian value and accepted when they are nonzero and
below the group order (P-521: the top 7 bits of the 66-byte value are cleared first), at most 64
draws.

## EC (`ec`)

- Curves: P-256, P-384, P-521. P-192 and P-224 SubjectPublicKeyInfo (and any other named curve)
  fail with `Error::UnsupportedCurve`, including through `boot_key_hash` (user decision, 0006 D3).
- `Point::from_xy` decodes fixed-width big-endian `X || Y` (64, 96, 132 bytes, no SEC1 prefix)
  and checks that the point is on the curve; the point at infinity is never representable, so
  `add` and `multiply` return `Error::InvalidPoint` when the result would be infinity.
- `PublicKey::from_spki_der` takes a SubjectPublicKeyInfo with `id-ecPublicKey`, a named-curve
  OID and a compressed (`02`/`03`) or uncompressed (`04`) point, with no trailing data. The key
  remembers its `PointForm`, and `to_spki_der` rebuilds the DER with `spki`/`der` from that
  form, so a compressed URI key (`ec_util.cpp:760`) exports to the same bytes it came from. A key
  from `PrivateKey::public_key` is uncompressed, like OpenSSL's generated keys.
- Boot-key hash (`ec_crypto.cpp:26-50`): `boot_key_hash(der, prefix)` imports, re-serializes and
  returns `SHA-256(prefix || re-serialized DER)`. It never hashes the input bytes, so input that
  is not canonical DER is an error, not a different hash. The prefix is text without NUL
  (`Error::InvalidLabel`), optional.
- `PrivateKey::ecdh` returns the x-coordinate at the full field width (leading zeros kept).
  `Point::multiply`, `Point::add` and `Scalar::add` (mod the group order) cover
  `ec_crypto.cpp:122-138` and `ec_ctrl_configurator.cpp:871, 1198`. `PrivateKey::scalar` gives
  the private scalar for `Scalar::add`.
- ECDSA: one path for all three curves. `z = SHA-256(message)` left-padded with zeros to
  the field width goes to `ecdsa::hazmat::sign_prehashed` / `verify_prehashed`; `k` is drawn from
  the `RandomSource`, a fresh draw when `r` or `s` is zero, and wiped after use. The curve
  crates' `random` constructors are never called (their generator interface cannot report
  failure). Their `Signer` and `PrehashSigner` paths are not used, because they derive `k`
  themselves (RFC 6979). `verify_prehash` with a 32-byte digest fails for P-521 in `p521` 0.13.
  Signatures are DER. Verification accepts any valid `s` (no low-s rule), like OpenSSL.
- Secret scalars (`Scalar`, `PrivateKey`) keep their bytes in a `SecretBytes`; `Debug` is redacted
  and there is no `Clone`/`Copy`. Working copies in the curve crates' types are wiped where this
  crate owns them; by-value arguments to `from_repr` and `sign_prehashed` are copies the curve
  crates drop without wiping (listed under Secret handling and limits).

## Behaviour decisions

- CBC decryption returns the complete plaintext (upstream drops the last `len % 16` bytes).
  CBC is not authenticated: callers must check a MAC.
- Tags, MICs and key-wrap integrity values are checked in full. Use `ct::eq` for received
  authenticators; it also rejects a length mismatch. `keywrap::unwrap` implements RFC 3394 W^-1
  on `aes` and compares all eight bytes of the recovered integrity value with `ct::eq`
  (`aes-kw` 0.2.1 compares with `==`, so only its `wrap` is used); intermediate buffers are wiped.
- DH: peers and exponents are accepted only in `2 ..= p - 2`; there is no subgroup test.
  Peer values may have any number of leading zero bytes; more than 192 significant bytes are out
  of range. The shared secret has no leading zeros (OpenSSL 3 derive output). Exponent import
  takes 1 to 192 bytes (length checked before conversion) and applies the same range; it
  left-pads by length without inspecting the value. `DhPublic::from_be_bytes` stores a peer value:
  leading zeros are ignored, up to 192 significant bytes, range `2 ..= p - 2`, fixed 192-byte
  storage.
- 802.11 KDF: 1 to 8191 output bytes, non-empty key. WPS KDF: 32-byte key, output below
  536,870,912 bytes (checked before allocating). HKDF: at most 255 hash blocks.
- KDF labels are text without NUL, as upstream's `strlen` would otherwise truncate them.
- Decoding stops at the first NUL, like upstream's C-string handling.
- Known difference: base64 and base64url decoding is strict beyond the pinned fixture
  set. Inputs with non-canonical trailing bits (`TR==`), whitespace or other noise that OpenSSL's
  BIO decoder may accept are rejected.

## Secret handling and limits

Secrets live in `SecretBytes`, `SecretArray`, `DhPrivate`, `Scalar` or `PrivateKey`, which wipe
on drop. Rust cannot observe freed memory in safe code, so the tests check the
`Zeroize`/`ZeroizeOnDrop` contracts and not the memory itself. Not wiped: the internal state of
the pinned `sha2`/`hmac`/`hkdf`/`pbkdf2` implementations and stack temporaries inside the
RustCrypto crates. `aes` and `cbc` wipe their key schedules (their `zeroize` features are
enabled), and `aes-siv` zeroizes its encryption key on drop. `crypto-bigint`'s `pow` always processes the full
1536-bit width of the exponent type used for DH. The EC scalar representation and `k` passed by
value to `from_repr`/`sign_prehashed` are stack copies that the curve crates do not wipe. This
crate's own byte buffers for scalars and `k` (`Zeroizing<FieldBytes>`) and its working copies of
curve scalars are wiped. Branches on secret values are limited to accept-or-reject decisions. DH exponent import
pads by length, and the EC private-key zero test uses the curve scalar's constant-time `is_zero`.
The DH range checks and rejection sampling branch only on whether a value is acceptable, which
does not depend on the accepted value. One exception: the DH shared secret drops its leading zero
bytes with a branch on the secret, so its length reveals how many leading zero bytes it had (about
one secret in 256 has one).

## Tests

- `tests/fixtures.rs`: dispatches every row of `tests/data/upstream_crypto_vectors.tsv`
  (generated by `tools/upstream-crypto-vectors/regen`) per kind and case, and fails on an
  unknown kind or case (the ecdh, ec-scalar, signature and ec-point cases are matched exactly, not by prefix), an unread column or field, or a row count other than 155.
- `tests/vectors.rs`: RFC 5297, 3394, 4231, 5869 and 6070.
- `tests/wsc.rs`: the WSC chain (DH secret, SHA-256, KDK, WPS KDF, 32/16/32 split) against the
  oracle rows; the split takes the derived buffer by value and borrows slices, with no clone.
- `tests/ec.rs`: P-521 sign/verify/ECDH/encoding, Rust-signed ECDSA on all curves with changed
  message/signature rejection, `k` taken from the source, on-curve and SPKI rejection, compressed
  round trips, boot-key hash, random-source failure through EC key generation and signing.
- `examples/sig_records.rs` with `regen --verify-records`: see below.
- `tests/api.rs`, `tests/secrets.rs`, `tests/random.rs`: bounds, rejections, secret-type traits
  (compile-time `Clone`/`Copy` probe), redaction, and random-source failure.

### Rust signatures against the upstream verifier

The pinned upstream verifier (OpenSSL 3, extracted by `tools/upstream-crypto-vectors`) checks
signatures made by this crate: fixed keys, generated keys and compressed-point keys on all three
curves. Each record must verify, and fail for a changed message and for a changed signature.

```sh
cargo run -p em-crypto --example sig_records -- /tmp/records.tsv
python3 tools/upstream-crypto-vectors/regen --verify-records /tmp/records.tsv
```

The records file is a scratch file (random keys and signatures), not a fixture. The command
prints one line per record and `records=N failures=0`, and exits nonzero on any failure.
`regen --check` regenerates the checked-in fixture byte for byte and is independent of this mode.

## Requirements for consumers

`em-easyconnect` and `em-core` (note 0006, D1, D3, D5):

- Validate the expected length of every tag, MIC or authenticator, compare the full value with
  `ct::eq`, and reject a mismatch. Do not reproduce the one-byte compare in
  `ec_ctrl_configurator.cpp:965`. Test short-tag and full-length-mismatch cases.
- `em-core` (WSC): verify the M2 Authenticator over the received message before decrypting
  Encrypted Settings. Treat `Error::InvalidPadding` from `cbc::decrypt` and a Key Wrap
  Authenticator mismatch as one opaque rejection (no distinguishable error, message or timing)
  and end the exchange. Store a peer's DH public value with `DhPublic::from_be_bytes`.
- `em-core` (WSC): require exactly one final M2 Authenticator and exactly one final Key Wrap
  Authenticator (8 bytes each); reject missing, duplicated or non-final ones, including any
  attribute after them. Compute HMAC-SHA256 with AuthKey over M1 followed by the M2 payload
  before the Authenticator header, and over the decrypted settings before the Key Wrap
  Authenticator; compare the first eight bytes with all eight received bytes. Verify both
  boundaries before applying any configuration. Hold the per-exchange keys (DH private key,
  AuthKey, KeyWrapKey, EMSK) in `em-crypto`'s secret types, end the exchange on success,
  rejection, timeout and replacement, and test that a later exchange cannot reuse them.
- `em-easyconnect`: use `ec::boot_key_hash` on DER only (never hash raw input or PEM text), keep
  the `PointForm` of imported keys when exporting, and handle `Error::UnsupportedCurve` for
  P-192/P-224 peers (upstream accepts them; this port refuses them). Sign connectors with
  `PrivateKey::sign` (SHA-256, DER, then base64url) and verify the original encoded
  `header.payload` text with `PublicKey::verify`. Pass `OsRandom` for production signing and keys.
- `em-easyconnect`: specify and implement `L.x` (the x-coordinate of `((bR + pR) mod q) * BI`)
  with errors for failed operations instead of upstream's NULL result; test mutual
  authentication past `L.x` into `k_e` derivation; read PEM files with both private and public
  blocks; `psk_hex` formatting stays with the caller. This crate takes raw scalars and SPKI DER only.
