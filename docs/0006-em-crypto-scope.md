# 0006: Scope and design rules for `em-crypto`

| | |
|---|---|
| Status | **Accepted** (rev 3; final fixes after review round 2 checked against the pinned source by an independent check, not reviewed: two-round limit) |
| Date | 2026-11-19 |
| Upstream | `rdkcentral/unified-wifi-mesh` @ `ba4180ba36748629f10c20babc4320dd69c9db4f` |
| Affects | `em-crypto`; `em-easyconnect` (DPP protocol crypto); `em-core` (WSC session state); `docs/em_base-coverage.md` |

Rev 3 changes:

- Record the user's decision to fix upstream defects and the answers to the remaining questions.
- Complete the oracle and acceptance contracts for DH, EC encoding, CBC, KDFs and WSC.
- Curves limited to P-256/384/521 by the user; P-192 and P-224 are refused (D3).
- Lenient base64url decoding kept by the user, pinned against the oracle (D3).
- WSC session state moves to `em-core`, decided by the user (D5 item 3).
- All intentional differences in D3 accepted by the user (D5 item 4).

Rev 3, review round 1 fixes:

- Complete the decoder, DH, EC and KDF oracle and acceptance contracts.
- Record WSC receive-boundary and session-lifecycle requirements for `em-core`, and the
  `calculate_Lx` defect and follow-up for `em-easyconnect`.
- Correct the decision references and pinned-source inventory and attributions.

Rev 3, review round 2 fixes:

- Require final authenticators and complete authenticated coverage at both WSC receive boundaries.
- Give complete definitions for oracle extraction and distinguish operation references.
- Check the point form of upstream-generated SPKI separately from fixed-key fixtures.

## Context

`STATUS.md` maps `em-crypto` to `src/util_crypto/` and `src/em/crypto/`, and says to prefer an
existing crate over a port.

| Upstream | Size | Licence | What it is |
|---|---|---|---|
| `src/util_crypto/aes_siv.c`, `aes_siv.h` | 741 + 181 lines | Third-party (The Industrial Lounge, 2007), BSD-style with an advertising/acknowledgement clause; "This license and distribution terms cannot be changed" (`src/util_crypto/LICENSE:3-16, 32-34`) | AES-SIV (S2V/CMAC + CTR), keys of 256, 384 or 512 bits |
| `src/em/crypto/em_crypto.cpp`, `inc/em_crypto.h` | 1961 + 1244 lines | Apache-2.0 | Class `em_crypto_t`: 47 static functions in the OpenSSL 3 branch, plus per-session WSC state (`m_crypto_info`) |
| `src/em/prov/easyconnect/ec_crypto.cpp`, `inc/ec_crypto.h` | 1226 + 1161 lines | no per-file notice (repository `LICENSE` is Apache-2.0) | Class `ec_crypto`: DPP protocol crypto. `STATUS.md` assigns this directory to `em-easyconnect`. |

A port derived from `aes_siv.c` would have to carry its licence terms (including the
acknowledgement). This port uses an **independent implementation** instead (D2).

`docs/em_base-coverage.md` assigns three `em_base.h` entries to `em-crypto`: `em_nonce_t`,
`em_dh5_key_t` and `em_crypto_info_t` (the last moves to `em-core`, D1).

### Primitives, their exact contracts, and their users

| Primitive (upstream) | Exact contract to reproduce | Used by |
|---|---|---|
| AES-SIV (`siv_init/encrypt/decrypt/free`) | RFC 5297 with an ordered list of associated-data pieces; the primitive returns the tag and ciphertext separately (`aes_siv.c:640-660`), and the DPP wrapper places them as `tag ‖ ciphertext` (`ec_util.cpp:157-185`); keys 256/384/512 bits (AES-128/192/256) | DPP wrapped data (`ec_util.cpp`, hard-coded `SIV_256`) |
| AES key wrap (`aes_key_wrap/unwrap`) | RFC 3394, IV `A6A6A6A6A6A6A6A6`, KEK of 16/24/32 bytes; wrap input at least 16 bytes and a multiple of 8, unwrap input at least 24 bytes and a multiple of 8 (`em_crypto.cpp:646-685`) | 1905 layer security |
| 802.11 KDF (`kdf_hash_length`) | HMAC(key, `LE16(counter) ‖ label ‖ context ‖ LE16(output_bits)`), label without NUL (`strlen`), hash SHA-256/384/512 (`em_crypto.cpp:1900-1961`, `inc/util.h:44-49`). For requests accepted by its iteration-limit check, upstream silently truncates the 16-bit `output_bits` above 8191 output bytes (`em_crypto.cpp:1916-1936`) | 1905 layer security |
| WPS KDF (`wps_key_derivation_function`) | HMAC-SHA256(32-byte key, `BE32(counter) ‖ prefix ‖ label ‖ BE32(output_bits)`), label without NUL; `output_bits` is `res_len * 8` in 32 bits and first wraps at 536,870,912 bytes. The port rejects that length and above; WSC asks for 80 bytes (`em_crypto.cpp:402-473`, `em_configuration.cpp:4077, 4122, 4160, 4808`) | WSC M1/M2 |
| Hash and HMAC over several pieces (`platform_hash`, `platform_hmac_hash`, SHA-256 wrappers) | concatenation of the pieces, SHA-256/384/512 | WSC, 1905 layer |
| AES-128-CBC (`platform_aes_128_cbc_*`) | PKCS#7 padding **on** in these wrappers (the generic helpers default to off) | WSC Encrypted Settings |
| DH group 5 (`platform_compute_shared_secret`, `create_dh_pkey`) | RFC 3526 1536-bit prime, g = 2 (`em_crypto.cpp:65-84`); public keys exported fixed-width (`BN_bn2binpad(.., DH_KEY_SZ)`, `:206-234`); shared secret is the OpenSSL 3 derive output, whose length WSC hashes as-is (`:805-811, 1116-1129`, `em_configuration.cpp:4089-4092`) | WSC |
| HKDF (`ec_crypto::hkdf`) | RFC 5869; the OpenSSL 3 branch (`ec_crypto.cpp:212-297`) is the reference; the only observed caller does extract-and-expand (`skip_extract = false`) | DPP |
| PBKDF2-HMAC-SHA1 (inside `ec_crypto::gen_psk`) | 4096 iterations, 32-byte output (`ec_crypto.cpp:1081-1090`); `em.cpp:2590-2597` hex-encodes it as `psk_hex` | DPP / credentials |
| EC | key pairs; ECDH x-coordinate; scalar add modulo the group order, point × scalar, point + point (`ec_crypto.cpp:122-138`, `ec_ctrl_configurator.cpp:871, 1198`); point codec as fixed-width big-endian `X ‖ Y` with no SEC1 prefix (`ec_crypto.cpp:631-644`) and decode with an on-curve check; SPKI DER (base64) import/export of bootstrapping keys incl. compressed points (`em_crypto.cpp:1215, 1239`, `ec_util.cpp:616, 729, 760`), whose re-serialized DER also feeds the boot-key hash (`ec_crypto.cpp:26-50`) | DPP |
| ECDSA | connector signing: `ES256`, SHA-256 by default, **DER** signature then base64url (`inc/em_crypto.h:579-581`, `ec_crypto.cpp:1062-1070, 1102`, `em_crypto.cpp:979-993`); verification of the original encoded message uses SHA-256 (`ec_crypto.cpp:753-757`, `em_crypto.cpp:1767-1792`). Protocol hashing elsewhere follows the per-curve hash below | DPP connectors |
| base64 / base64url | upstream's base64url decoder adds missing padding and also accepts `+` and `/` (`em_crypto.cpp:907-924`) | DPP, controller data model |
| Random bytes (`RAND_bytes`, 18 call sites) | OS randomness | nonces, IVs, keys |

Curves accepted by the DPP connection setup, with the protocol hash each one selects: P-192, P-224
and P-256 → SHA-256, P-384 → SHA-384, P-521 → SHA-512 (`ec_crypto.cpp:548-574`). The local default key is P-256 (`DPP_KEY_NID`, `inc/ec_base.h:61`).
Brainpool and X25519/X448 appear only in the TLS group table (`inc/ec_crypto.h:575-600`).

With the local OpenSSL 3.5.5 default provider, the upstream key-generation path
(`em_crypto.cpp:1353-1405`) and `i2d_PUBKEY` default (`em_crypto.cpp:1239`) export an uncompressed
SEC1 point (`04 ‖ X ‖ Y`). Importing and re-serializing the compressed point in the URI example
preserves its compressed form (`02` or `03` followed by `X`); the oracle must record its OpenSSL
version and exact DER bytes for both paths.

### Upstream defects found

1. **CBC decryption loses the final plaintext bytes.** `platform_cipher_decrypt` decrypts in place
   (`EVP_DecryptUpdate(ctx, data, ...)`) but writes the final block to a scratch `buf` that is
   never copied back, while the returned length includes it (`em_crypto.cpp:593-637`). WSC then
   parses `data` with that length (`em_configuration.cpp:4889-4901`). When the plaintext length is not
   a multiple of 16, its last `len % 16` bytes remain ciphertext and can differ from plaintext. For
   block-aligned plaintext the final block is all padding and nothing is lost. An empty plaintext
   returns 0, the same value upstream uses for errors.
2. **Authentication checks that do not authenticate** (DPP / WSC, consumer side):
   `memcmp(r_auth_prime, resp_auth_tag, sizeof(resp_auth_tag) != 0)` passes the Boolean
   `sizeof(..) != 0`, i.e. 1, as the length, so one byte is compared
   (`ec_ctrl_configurator.cpp:965`); the enrollee compares with the received attribute's length
   (`ec_enrollee.cpp:799-804, 869`); the WSC M2 authenticator mismatch is only logged, its
   `return -1` is commented out (`em_configuration.cpp:4820-4823`). The M2 parser succeeds
   without an Authenticator (`em_configuration.cpp:4593-4632`), and the decrypted-settings parser
   does not require a Key Wrap Authenticator and continues processing settings after one
   (`em_configuration.cpp:4909-4948`).
3. Tags and MICs are compared with `memcmp`, not in constant time (`aes_siv.c:713`,
   `ec_1905_encrypt_layer.cpp:918`, DPP sites).
4. No explicit range check of a DH peer public key; rejection depends on what OpenSSL 3 checks in
   `EVP_PKEY_derive_set_peer` (`em_crypto.cpp:1111`).
5. The legacy (pre-OpenSSL-3) HKDF mixes `calloc` with `delete[]` and uses an 8-bit counter
   (`ec_crypto.cpp:332-353, 379-448`); the OpenSSL 3 branch does not.
6. `exit(1)` in library init (`em_crypto.cpp:97-108`); the WSC session fields are not wiped
   (`inc/em_crypto.h:1203`; `BN_clear_free` and `siv_free` do clean up their own data);
   `RAND_bytes` results ignored in WSC init, the 1905-layer EAPOL handshake nonce (`s_nonce`) and
   `rand_zero*` (`em_crypto.cpp:125`, `ec_1905_encrypt_layer.cpp:1418-1426`, `inc/ec_crypto.h:698, 733`).
7. The SIV and KDF coverage lacks known-answer tests; it uses round-trip/sanity checks. Other
   fixed-output tests include encoding (`tests/test_em_crypto.cpp:100-146`), RFC 3394 and
   RFC 4231 (`tests/test_em_crypto.cpp:494-515, 786-815`).
8. **The enrollee's `calculate_Lx` never returns a coordinate.** `L_x` is initialized to NULL,
   passed as the output to `EC_POINT_get_affine_coordinates` without being allocated, and never
   assigned, so the function always returns NULL. The same call sets its success flag when the
   coordinate operation fails (`ec_crypto.cpp:116-150`). The enrollee aborts mutual authentication
   when it sees the NULL result (`ec_enrollee.cpp:1265-1276`).

## Decision

### D1. What `em-crypto` contains

**Primitives only, no session state**, synchronous (0004 R1), no `unsafe` and no clones in our
code (0004 R2):

| Module | Contents |
|---|---|
| `siv` | encrypt/decrypt with an ordered list of AD slices; 256/384/512-bit keys |
| `keywrap` | RFC 3394 wrap/unwrap, KEK 128/192/256 |
| `hash` | SHA-256/384/512 and HMAC over a list of slices |
| `kdf` | `ieee80211_kdf` and `wps_kdf` with the exact layouts above; `hkdf` (extract-and-expand); `pbkdf2_hmac_sha1` |
| `cbc` | AES-128-CBC with PKCS#7 |
| `dh` | group 5: key pair, fixed-width public key, shared secret (byte contract above), peer check |
| `ec` | curves P-256, P-384, P-521 (P-192 and P-224 refused, D3); key pairs; ECDH x; scalar add mod order, point × scalar, point + point; `X ‖ Y` codec with on-curve check; SPKI DER import/export incl. compressed points; ECDSA sign/verify with DER signatures |
| `encoding` | base64, and base64url with upstream's lenient decoding (decided, D3); strict encoding (URL-safe alphabet, no padding) |
| `rand` | fill a buffer from the OS; failure is an error |
| `ct` | constant-time equality of equal-length byte strings |

**Secrets:** functions borrow keys (`&[u8]` or key types) and return owned secrets in types that
zero their memory on drop (or write into caller-owned zeroizing buffers). Secret types implement
neither `Clone` nor `Copy`, and their `Debug` does not print key bytes. Scratch buffers holding
secrets are wiped on every path, including errors.

**Not in `em-crypto`:**

- `em_crypto_info_t` and the `em_crypto_t` accessors (WSC per-session nonces, keys, UUIDs, MACs):
  move to the WSC/configuration actor in `em-core`. `docs/em_base-coverage.md` is updated on
  acceptance (its `em_crypto_info_t` row and the crate counts).
- DPP protocol logic (`ec_crypto`: `compute_ke`, `calculate_Lx`, connectors and JWS, hashing
  contexts, TLS group ids, `gen_psk`'s use of PBKDF2) and key files: `em-easyconnect`, built on
  D1. For parity it must read, and test reading, upstream PEM files containing both private and
  public blocks (`em_crypto.cpp:1423-1451`). The `psk_hex` formatting stays with its caller.
  `em_configuration.cpp:3941-3958` also uses the point and TLS-group helpers.
  The `em-easyconnect` follow-up must fix defect 8: specify and implement the x-coordinate of
  `((bR + pR) mod q) * BI`, with errors for failed operations, rather than copying the NULL
  result. Its consumer tests must exercise successful mutual authentication past L.x and into
  `k_e` derivation, and rejection on computation failure, separately from primitive parity tests.

### D2. Use existing crates, not a port

Accepted by the user: pinned RustCrypto crates (`aes`, `aes-siv` (low-level API, for ordered AD
lists and the AES-192 instantiation needed by 384-bit keys), `aes-kw`, `cbc`, `hmac`, `sha1`,
`sha2`, `hkdf`, `pbkdf2`, `p256`, `p384`, `p521`, `ecdsa`, `elliptic-curve`,
`spki`/`der`, `crypto-bigint` (constant-time Montgomery exponentiation for group 5), `base64ct`,
`subtle`, `zeroize`, `getrandom`). Our glue is small: the multi-piece hash/HMAC, the 802.11 and
WPS KDFs, the group 5 constants and the `X ‖ Y` codec. No OpenSSL dependency.

The exact versions and features are pinned in the workspace, and the dependency gate is a
`--locked` build and test on Rust 1.85.0 with those features. If a crate's current release needs
a newer compiler, an older compatible release is pinned, or the case is brought back to the user.

### D3. Behavioural parity rules

- Same bytes as upstream (OpenSSL 3 build) for the same inputs, for every deterministic operation,
  except for the intentional differences below. Randomized outputs (key generation, ECDSA
  signatures) are checked by cross-verification instead.
- The DH shared secret has upstream's length behaviour (OpenSSL 3 default, no padding) unless the
  oracle shows otherwise; leading-zero cases are tested against the oracle before this is frozen.
  The port accepts a peer public value `y` only in `2 ..= p - 2` (excluding the order-1 and
  order-2 elements of this safe-prime group). It does not add a full subgroup test (`y^q = 1`,
  where `q = (p - 1) / 2`), so it also accepts an in-range value outside that subgroup. The oracle
  tests record upstream and Rust outcomes separately; when both accept, their derived bytes match.
  If upstream rejects an in-range value, Rust still accepts it and checks independently computed
  bytes as in D4. Exponentiation is constant time and uses a fixed public 1536-bit iteration bound,
  independent of the secret exponent.
- **base64url (user decision, 2026-11-19):** decoding matches upstream's leniency exactly: `-`/`_`
  mapped to `+`/`/`, existing `+`/`/` accepted, missing padding added, ordinary existing padding accepted
  (`em_crypto.cpp:907-924`). The accepted and rejected set is pinned against the D4 oracle:
  missing padding, existing padding, `+`/`/`, mixed alphabets, invalid lengths, empty input,
  malformed padding, and embedded NUL. Encoding is strict: URL-safe alphabet, no padding. A JWS
  signature covers the encoded `header.payload` text, so this leniency cannot change what was signed.
- **Intentional differences:**
  - **CBC decryption returns the correct plaintext** (defect 1). It can differ from upstream on
    valid, non-block-aligned input; chosen fixtures must expose the defect. Ciphertext (encryption)
    is byte-identical.
  - Tags, MICs and authenticators are checked for the expected length and compared in full, in
    constant time (defects 2 and 3).
  - A DH peer key outside `2 ..= p - 2` is rejected even if the oracle accepts it (defect 4).
  - Errors are returned, never `exit()`; random-source failures are errors (defect 6).
  - Secrets are zeroed (defect 6).
  - HKDF output over 255 hash blocks is an error (RFC limit; upstream OpenSSL 3 also fails).
  - The 802.11 KDF rejects outputs over 8191 bytes instead of truncating its 16-bit length field;
    the WPS KDF rejects outputs whose bit count does not fit in 32 bits instead of wrapping.
  - **Only P-256, P-384 and P-521 are supported** (user decision, 2026-11-19). A peer key on
    P-192 or P-224, which upstream's DPP setup accepts (`ec_crypto.cpp:561-568`) and its TLS group
    table maps (`inc/ec_crypto.h:587-590, 642-644`), is refused with an "unsupported curve" error,
    tested for both curves. Reasons: 96- and 112-bit security, probably outside the Easy Connect
    curve list (not verified here), upstream's own key is P-256, and it avoids two less mature
    crates. They can be added later behind a feature flag if a real peer needs them.
  - Hashes are limited to SHA-256/384/512 for the KDFs (SHA-1 only for PBKDF2); upstream's generic
    KDF also accepts SHA-1, used only in its tests (`tests/test_em_crypto.cpp:671-675`).
  - `em-easyconnect` fixes the NULL `calculate_Lx` result (defect 8), as specified in its
    consumer follow-up in D1.
- **Consumer requirements** (defects 2 and 3): `em-easyconnect` and `em-core` must validate the
  expected length of every tag, MIC or authenticator, compare the full value with `ct::eq`, and
  reject a mismatch. The one-byte comparison in `ec_ctrl_configurator.cpp:965` is not reproduced.
  Their notes must include short-tag and full-length-mismatch tests.
  The `em-core` follow-up must cover both the M2 Authenticator and the Key Wrap Authenticator.
  Require exactly one final M2 Authenticator in each M2 payload and exactly one final Key Wrap
  Authenticator in each decrypted settings payload, as the pinned senders emit
  (`em_configuration.cpp:3155-3165, 5982-5996`). Reject a missing, duplicated or non-final
  authenticator, including any attribute or setting after it. Validate each attribute's enclosing
  bounds and exact eight-byte authenticator length before copying or comparing, derive the covered
  spans from validated authenticator-header offsets, and propagate parser errors to the receive
  handler. For M2, compute HMAC-SHA256 with the session's AuthKey over the complete M1 followed by
  the M2 payload before the Authenticator header (`em_configuration.cpp:6034-6048`). For Key Wrap,
  use the same key over the decrypted settings before the Key Wrap Authenticator header. At each
  boundary compare the first eight HMAC bytes with all eight received bytes. Verify both boundaries
  before applying any configuration from the message, including when it contains multiple M2
  payloads; only covered settings may be used. Upstream copies M2 by the declared length and
  ignores the parser result (`em_configuration.cpp:4624-4625, 4805`); it emits eight Key Wrap
  Authenticator bytes (`em_configuration.cpp:5982-5993`, `inc/em_base.h:280`) but reads only one
  (`em_configuration.cpp:4941-4943`).
  Test each boundary with a missing, duplicated or non-final authenticator, including a trailing
  setting after an otherwise valid authenticator, as well as a short value and a full-length value
  whose prefix matches and final byte differs. Each rejection must prevent any configuration from
  being applied. Key Wrap tests must have a valid outer M2 Authenticator so they exercise the
  inner check.

### D4. Proof of parity (Step 1, golden data)

1. **Published vectors** in Rust tests: RFC 5297 (SIV), RFC 3394 (all three KEK sizes), RFC 4231
   (HMAC), RFC 5869 (HKDF), RFC 6070 PBKDF2-HMAC-SHA1 vectors, and the curve crates' own vectors.
   The pinned oracle below supplies the 802.11 and WPS KDF expected outputs and also checks HKDF
   and the `gen_psk` PBKDF2 parameters. Direct multi-piece SHA-256/384/512 hash and HMAC tests
   compare with the oracle's `platform_hash` and `platform_hmac_hash`, including a split input and
   its concatenation; RFC 4231 also supplies HMAC known answers.
2. **Upstream oracle.** `tools/upstream-crypto-vectors/` holds a C++ harness that, like the em-msg
   vectors, uses upstream code copied verbatim from the pinned files into a minimal declaration
   shim (because `em_crypto.h` includes `em_base.h`, which needs unavailable OneWifi headers).
   **Extract verbatim:** use the following complete definitions and constants, with declarations
   supplied by the shim and the OpenSSL 3 variants where there are alternatives:

   - `SHA256_MAC_LEN`, `AES_BLOCK_SIZE` and the SHA-256 hash and HMAC wrappers
     (`inc/em_crypto.h:45-46, 147-149, 181-183`); `SWAP_LITTLE_ENDIAN` (`inc/util.h:44-50`).
   - `platform_hash`, `platform_hmac_hash`, `append_u32_net`, `wps_key_derivation_function`,
     `platform_cipher_encrypt/decrypt` and `aes_key_wrap/unwrap` (`em_crypto.cpp:260-701`);
     `kdf_hash_length` (`em_crypto.cpp:1900-1961`).
   - The group 5 constants (`em_crypto.cpp:65-84`), `create_dh_pkey`
     (`em_crypto.cpp:706-773`), `platform_compute_shared_secret` (`em_crypto.cpp:776-819`),
     `cleanup_bignums` (`em_crypto.cpp:1048-1053`) and `compute_secret_internal`
     (`em_crypto.cpp:1056-1145`).
   - `base64_encode` (`em_crypto.cpp:828-872`), `base64url_encode` (`em_crypto.cpp:883-904`, added in
     Step 1 for the encoder rows), `base64url_decode` (`em_crypto.cpp:907-925`),
     `base64_decode` (`em_crypto.cpp:1005-1046`), both SPKI import/export definitions
     (`em_crypto.cpp:1190-1254`), `generate_ec_key(int)` (`em_crypto.cpp:1353-1405`),
     the ECDSA signer (`em_crypto.cpp:927-1003`) and verifier (`em_crypto.cpp:1767-1811`).
   - The boot-key hash, OpenSSL 3 HKDF and `gen_psk` (`ec_crypto.cpp:13-60, 214-297,
     1081-1091`), and the ECDH x-coordinate and coordinate helpers
     (`inc/ec_crypto.h:221-234, 249-253, 438-445`). Compile the complete pinned `aes_siv.c`
     as C with its pinned header.

   **Reference only:** provide a logging shim for `em_printfout` (`inc/util.h:396`), and use the
   pinned `BN_mod_add` and `EC_POINT_mul` operations (`ec_crypto.cpp:122-135`) and `EC_POINT_add`
   (`ec_ctrl_configurator.cpp:869-871`) to design fixed-input point and scalar vectors for each
   supported curve; these statement ranges are not verbatim extraction ranges. Use the extracted
   `compute_ec_ss_x` and coordinate helpers with fixed private and peer public keys for ECDH
   x-coordinate vectors on each supported curve. These oracle operations, together with the curve
   crates' published vectors, supply the point and ECDH expected values; do not use the broken
   `calculate_Lx` return as a primitive oracle. Session state, `init` and the constructor are left
   out; DH uses fixed private keys. The harness runs on fixed inputs, including empty,
   block-aligned and non-aligned CBC plaintexts, DH leading-zero and peer-boundary cases, HKDF at
   255 blocks, and the 8192-byte 802.11 KDF case. It also runs the full WSC chain using these pinned
   primitives: hash the DH secret, derive the KDK from the enrollee nonce, enrollee MAC and
   registrar nonce, apply the WPS KDF, and split the result (`inc/em_configuration.h:2136-2143,
   2165-2167`, `em_configuration.cpp:4077-4130`). It writes
   `crates/em-crypto/tests/data/upstream_crypto_vectors.tsv` with the pin, extraction and build
   commands, OpenSSL version, and a byte-for-byte regeneration check.
   The decoder rows record success or failure and exact output bytes for every D3 case, including
   empty input, malformed padding and embedded NUL; Rust's accepted and rejected set and outputs
   match the recorded oracle outcomes, including the decoder's NUL-terminated input behavior.
   With the extracted pinned bodies and local OpenSSL 3.5.5, the fixed cases are: empty input →
   success with empty bytes; `TQ` and `TQ==` → success `4d`; `-_8` and `+/8=` → success `fb ff`;
   mixed `-_+/` → success `fb ff bf`; invalid-length `A` and malformed-padding `TQ===` → failure;
   and the nine bytes `TWFu\0TQ==` → success `4d 61 6e`. Construct the last string with its
   explicit length so the suffix reaches the decoder. Record the OpenSSL version, provider and
   outcomes during fixture regeneration, and investigate any difference before changing a fixture;
   do not assume other malformed inputs behave the same way.
   For byte-for-byte generated-form SPKI comparisons, use a fixed valid named-curve key on each
   supported curve with an uncompressed point; also use the fixed compressed URI key. Include fixed
   valid P-192 and P-224 SPKI keys to verify the two unsupported-curve errors. Separately from fixture
   regeneration, generate a key for each supported curve with the extracted upstream
   `generate_ec_key(int)` EVP keygen path and export it through the extracted SPKI `i2d_PUBKEY`
   path. Parse the DER subjectPublicKey BIT STRING and check that its SEC1 point has the uncompressed
   `04 ‖ X ‖ Y` form with the curve's coordinate width. Record the OpenSSL version, provider and
   generated DER separately from the fixed fixtures, without comparing randomized DER across runs;
   a different generated form fails this check and must be investigated rather than changing the
   fixed byte fixtures.
   For connector signing, create and retain a fixed upstream SHA-256/DER signature over a fixed
   encoded `header.payload` with a fixed key on each supported curve, using the pinned signer
   and its SHA-256 default (`inc/em_crypto.h:579-581`, `ec_crypto.cpp:1062-1064`). Treat each
   signature as a checked-in harness input, verify it with the pinned verifier using SHA-256 as
   the connector does (`ec_crypto.cpp:753-757`), and emit it unchanged during regeneration; Rust
   must verify it. Separately from byte-for-byte fixture regeneration, pass Rust-produced SHA-256/DER
   signatures and the corresponding message and public key to the pinned upstream verifier with
   SHA-256. Check successful verification in both directions and rejection of a changed message
   or signature. Also cross-check keys generated by Rust with the upstream verifier.
3. Fixture rows record upstream outcomes, including return values and output bytes. Rust tests
   assert equality with upstream and published vectors except where D3 declares a difference; in
   those cases they assert the specified Rust outcome separately. For each valid CBC plaintext,
   compare the bytes in upstream's returned span and the Rust plaintext: selected non-aligned cases
   expose the upstream defect, while aligned and empty spans match. For malformed padding, check
   upstream's return and the Rust error. Upstream returns 0 for both an empty plaintext and an
   error; Rust must distinguish successful empty plaintext from malformed padding.
   The 8192-byte 802.11 row records upstream's successful output with a truncated bit count, while
   Rust rejects the length. Test the WPS length bound separately, without generating an upstream
   output of hundreds of MiB. For DH, record upstream's and Rust's outcomes separately for every
   in-range fixture, including `p - 2`. When upstream accepts, compare exact derived bytes; when
   it rejects an in-range fixture, independently compute and assert the Rust secret. In particular,
   with fixed private exponent `3` and peer `p - 2`, Rust must accept and return the 192-byte
   big-endian encoding of `p - 8`, since `(p - 2)^3 mod p = p - 8`, even if the provider rejects
   that peer. With the extracted pinned DH path and the local OpenSSL 3.5.5 default provider,
   exponent `3` and peer `2` produce the one-byte secret `08`, and peer `p - 2` succeeds with
   the 192-byte `p - 8`; provider results remain separate from the required Rust outcomes. Record
   upstream's outcomes for `0`, `1`, `p - 1`, `p`, and `p + 1`; Rust must reject
   each. With the same fixed private exponent, public value `2^3 = 8` must export as exactly
   192 bytes: 191 zero bytes followed by `08` (`em_crypto.cpp:206-210`, `inc/em_base.h:300`).

### D5. Decisions and open questions

Decided by the user (2026-11-19): (a) pinned RustCrypto crates are accepted as dependencies; 0004
R2/R7 keep applying to our own code. (b) `libssl-dev` is installed on the build machine for the
D4 oracle (development only; the Rust crate does not link OpenSSL). (c) The port fixes the listed
upstream defects rather than copying them: CBC decryption returns the full plaintext; tags, MICs
and authenticators use expected-length validation and full constant-time comparison; and the
WSC-handling crate rejects an M2 authenticator mismatch. (d) Every other intentional difference in
D3 is accepted as listed; the full DH subgroup test is not added.

Decided questions (all answered 2026-11-19):

1. *(Decided 2026-11-19: limit to P-256/384/521; P-192 and P-224 refused, see D3.)*
2. *(Decided 2026-11-19: keep upstream's lenient base64url decoding, see D3.)*
3. *(Decided 2026-11-19: yes.)* **WSC session state to `em-core`** (`em_crypto_info_t`): the
   WSC/configuration actor owns one session record per M1/M2 exchange, including the DH private
   keys and the derived AuthKey, KeyWrapKey and EMSK. Upstream currently places the private keys
   in `em_crypto_info_t` (`inc/em_base.h:3119-3137`) and the three derived keys in configuration
   members (`inc/em_configuration.h:2290-2293`). The record's secret fields use `em-crypto`'s
   zeroizing types and are wiped when the session ends; `em-crypto` stays stateless. The `em-core`
   follow-up must end and drop the exchange on success, rejection, timeout or replacement, even
   when the actor remains alive. A lifecycle test must exercise each terminal path, verify secret
   cleanup, and show that a later exchange cannot reuse the prior exchange's keys.
4. *(Decided 2026-11-19: accepted as listed in D3.)* **Other intentional differences in D3.** The
   full DH subgroup test (`y^q = 1`) is not added; the range check stays as in D3.

## Acceptance criteria

1. `crates/em-crypto` builds with the workspace lints; build, test, clippy `-D warnings`, fmt and
   rustdoc pass on stable and on 1.85.0 with `--locked`; dependency versions and features are
   pinned and listed in the crate README.
2. Per API, at least:
   - SIV `tag ‖ ciphertext` with ordered AD, all key sizes, and tamper rejection.
   - Key wrap with all KEK sizes and integrity failure: accept 16-byte wrap and 24-byte unwrap
     inputs; reject shorter inputs and non-multiples of 8. Use eight `0xA6` IV bytes.
   - 802.11 and WPS KDFs against the pinned oracle; HKDF against RFC 5869 and the oracle;
     PBKDF2-HMAC-SHA1 against RFC 6070 and the oracle's `gen_psk` parameters. Include HKDF at
     255 blocks and one byte over, 802.11 KDF at 8191 and 8192 bytes, and rejection of WPS outputs
     starting at 536,870,912 bytes before allocating the output.
   - Direct multi-piece hash and HMAC comparisons for SHA-256, SHA-384 and SHA-512 against the
     pinned oracle, including the equivalent concatenated input, as specified in D4.
   - CBC empty, aligned, non-aligned and malformed-padding cases, comparing the returned spans and
     outcomes as in D4.
   - DH fixed vectors and leading-zero secrets; record separate upstream and Rust outcomes for
     `0`, `1`, `p - 1`, `p`, `p + 1`, and `p - 2`, and assert Rust's range-only acceptance and
     independent `p - 8` secret where required by D4. Check the leading-zero padded public export.
   - EC import/export (`X ‖ Y` and SPKI), point and scalar operations, ECDH, ECDSA
     cross-verification and on-curve rejection. Check exact SPKI DER export bytes, compressed
     point import and re-serialization using the URI example, and the boot-key hash against the
     oracle: SHA-256 over the optional prefix without NUL followed by the re-serialized DER. Use
     the fixed generated-form and refused-curve keys, generated-key point-form check, point and ECDH
     vector sources, fixed upstream signatures and separate upstream verification of Rust signatures
     described in D4.
     Explicitly test P-521 signing, verification, ECDH and encoding. The `em-easyconnect` acceptance
     tests must read a PEM file containing both blocks.
   - base64/base64url including every D3 decoder case and its recorded oracle outcome;
     random-source failure propagation, including through the selected EC crates' RNG integration.
   - The **WSC chain** end to end against the oracle: DH shared secret → SHA-256 → KDK → WPS KDF →
     32/16/32-byte key split (`em_configuration.cpp:4077-4130`). A caller test borrows and moves
     the derived buffer to split it into the three keys (`em_configuration.cpp:4122-4130`), without
     cloning it.
3. The oracle harness regenerates its fixture byte for byte; a bite test shows one changed vector
   fails.
4. Secret types implement neither `Clone` nor `Copy`, and their `Debug` hides key bytes (tested).
5. `STATUS.md`, `docs/em_base-coverage.md`, the crate README and the root README are updated; the
   consumer requirements in D1, D3 and D5 are recorded for `em-easyconnect` and `em-core`,
   including the authenticator, session-lifecycle and mutual-authentication follow-up tests.
