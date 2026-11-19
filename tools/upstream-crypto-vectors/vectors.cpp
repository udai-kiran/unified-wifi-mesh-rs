/*
 * SPDX-License-Identifier: Apache-2.0
 *
 * Fixed-input oracle for rdkcentral/unified-wifi-mesh @
 * ba4180ba36748629f10c20babc4320dd69c9db4f.
 *
 * regen extracts every upstream definition at build time into definitions.inc
 * and the inline includes in shim.hpp. Those bodies are not transcribed here.
 * The point/scalar operations follow ec_crypto.cpp:122-135 and
 * ec_ctrl_configurator.cpp:869-871 and point decoding follows
 * ec_crypto.cpp:670-685; public DH export follows
 * em_crypto.cpp:206-210. The WSC chain follows em_configuration.cpp:4077-4130.
 * The URI SPKI is the literal example from ec_util.cpp:760.
 */
#include "shim.hpp"
#include <array>
#include <fstream>
#include <memory>
#include <sstream>
#include <stdexcept>
#include "definitions.inc"

using Bytes = std::vector<uint8_t>;
using Pkey = std::unique_ptr<EVP_PKEY, decltype(&EVP_PKEY_free)>;
using BN = std::unique_ptr<BIGNUM, decltype(&BN_free)>;
using Group = std::unique_ptr<EC_GROUP, decltype(&EC_GROUP_free)>;
using Point = std::unique_ptr<EC_POINT, decltype(&EC_POINT_free)>;
using EcKey = std::unique_ptr<EC_KEY, decltype(&EC_KEY_free)>;
using Spki = std::unique_ptr<X509_PUBKEY, decltype(&X509_PUBKEY_free)>;
using Provider = std::unique_ptr<OSSL_PROVIDER, decltype(&OSSL_PROVIDER_unload)>;
struct OpenSslFree { void operator()(uint8_t *p) const { OPENSSL_free(p); } };
struct SivFree { void operator()(siv_ctx *p) const { siv_free(p); } };

static void require(bool ok, const std::string &message) {
    if (!ok) throw std::runtime_error(message);
}
static std::string hex(const uint8_t *data, size_t len) {
    static const char digits[] = "0123456789abcdef";
    if (!len) return "-";
    std::string out;
    out.reserve(len * 2);
    for (size_t i = 0; i < len; ++i) {
        out += digits[data[i] >> 4];
        out += digits[data[i] & 15];
    }
    return out;
}
static std::string hex(const Bytes &b) { return hex(b.data(), b.size()); }
static std::string text_hex(const std::string &s) {
    return hex(reinterpret_cast<const uint8_t *>(s.data()), s.size());
}
static Bytes unhex(const std::string &s) {
    require(s.size() % 2 == 0, "bad input hex");
    Bytes b;
    for (size_t i = 0; i < s.size(); i += 2) {
        auto digit = [](char c) -> unsigned {
            if (c >= '0' && c <= '9') return static_cast<unsigned>(c - '0');
            if (c >= 'a' && c <= 'f') return static_cast<unsigned>(c - 'a' + 10);
            throw std::runtime_error("bad input hex digit");
        };
        b.push_back(static_cast<uint8_t>((digit(s[i]) << 4) | digit(s[i + 1])));
    }
    return b;
}
static Bytes seq(size_t len, uint8_t first = 0) {
    Bytes b(len);
    for (size_t i = 0; i < len; ++i) b[i] = static_cast<uint8_t>(first + i);
    return b;
}
static Bytes bytes(const std::string &s) { return Bytes(s.begin(), s.end()); }
static BN bn(unsigned long n) {
    BN b(BN_new(), BN_free);
    require(b && BN_set_word(b.get(), n), "BN_set_word failed");
    return b;
}
static Bytes bn_bytes(const BIGNUM *n, int width = 0) {
    int len = width ? width : BN_num_bytes(n);
    if (!len) return Bytes{0};
    Bytes out(static_cast<size_t>(len));
    require(BN_bn2binpad(n, out.data(), len) == len, "BN_bn2binpad failed");
    return out;
}
static void row(std::ostream &out, const std::string &category, const std::string &name,
                const std::string &inputs, long result, const std::string &output) {
    out << category << '\t' << name << '\t' << inputs << '\t' << result << '\t' << output << '\n';
    require(static_cast<bool>(out), "fixture write failed");
}
static std::string provider_name() {
    std::unique_ptr<EVP_MD, decltype(&EVP_MD_free)> md(EVP_MD_fetch(nullptr, "SHA256", nullptr), EVP_MD_free);
    require(md != nullptr, "cannot fetch SHA256");
    const OSSL_PROVIDER *provider = EVP_MD_get0_provider(md.get());
    require(provider != nullptr, "SHA256 has no provider");
    const char *name = OSSL_PROVIDER_get0_name(provider);
    require(name != nullptr, "provider has no name");
    return std::string(name);
}
static void provenance(std::ostream &out) {
    out << "# OpenSSL: " << OpenSSL_version(OPENSSL_VERSION) << '\n';
    out << "# Provider: " << provider_name() << '\n';
}

static void kdf_vectors(std::ostream &out) {
    Bytes key = seq(32), context = seq(19, 0x50);
    const char *label = "Mesh KDF";
    const std::array<std::pair<const char *, const EVP_MD *(*)()>, 3> hashes{{
        {"sha256", EVP_sha256}, {"sha384", EVP_sha384}, {"sha512", EVP_sha512}}};
    for (const auto &entry : hashes) {
        Bytes result(73);
        bool ok = em_crypto_t::kdf_hash_length(entry.second(), key.data(), key.size(), label,
                                                context.data(), context.size(), result.data(), result.size());
        require(ok, "802.11 KDF failed");
        row(out, "kdf80211", entry.first, "key=" + hex(key) + ";label=" + text_hex(label) +
            ";context=" + hex(context) + ";length=73", ok, hex(result));
    }
    for (size_t length : {size_t(8191), size_t(8192)}) {
        Bytes result(length);
        bool ok = em_crypto_t::kdf_hash_length(EVP_sha256(), key.data(), key.size(), label,
                                                context.data(), context.size(), result.data(), result.size());
        require(ok, "802.11 KDF boundary failed");
        row(out, "kdf80211", "sha256-" + std::to_string(length),
            "key=" + hex(key) + ";label=" + text_hex(label) + ";context=" + hex(context) +
            ";length=" + std::to_string(length), ok,
            "upstream=" + hex(result) + ";rust=" + (length > 8191 ? "reject" : "accept"));
    }
    Bytes prefix = bytes("prefix"), result(80);
    char wps_label[] = "Wi-Fi Easy and Secure Key Derivation";
    int ok = em_crypto_t::wps_key_derivation_function(key.data(), prefix.data(), prefix.size(),
                                                       wps_label, result.data(), result.size());
    require(ok == 1, "WPS KDF failed");
    row(out, "wps", "prefix-80", "key=" + hex(key) + ";prefix=" + hex(prefix) +
        ";label=" + text_hex(wps_label) + ";length=80", ok, hex(result));

    Bytes ikm = seq(22, 0x20), salt = seq(13, 0x40), info = seq(10, 0x60);
    for (const auto &entry : hashes) {
        size_t block = static_cast<size_t>(EVP_MD_size(entry.second()));
        for (size_t length : {size_t(42), block * 255, block * 255 + 1}) {
            Bytes output(length, 0);
            size_t got = ec_crypto::hkdf(entry.second(), false, ikm.data(), ikm.size(), salt.data(), salt.size(),
                                         info.data(), info.size(), output.data(), output.size());
            require(got == (length == block * 255 + 1 ? 0 : length), "unexpected HKDF boundary result");
            if (!got) output.clear();
            row(out, "hkdf", std::string(entry.first) + "-" + std::to_string(length),
                "ikm=" + hex(ikm) + ";salt=" + hex(salt) + ";info=" + hex(info) +
                ";length=" + std::to_string(length), static_cast<long>(got), hex(output));
        }
    }
    std::string pass = "password", ssid = "IEEE";
    Bytes psk = ec_crypto::gen_psk(pass, ssid);
    require(psk.size() == 32, "gen_psk failed");
    row(out, "pbkdf2", "gen-psk", "pass=" + text_hex(pass) + ";ssid=" + text_hex(ssid) +
        ";iterations=4096;length=32", 1, hex(psk));
}

static void digest_vectors(std::ostream &out) {
    const std::array<std::pair<const char *, const EVP_MD *(*)()>, 3> hashes{{
        {"sha256", EVP_sha256}, {"sha384", EVP_sha384}, {"sha512", EVP_sha512}}};
    Bytes a = bytes("piece one"), b = seq(8, 0x90), c = bytes("last piece"), joined = a;
    joined.insert(joined.end(), b.begin(), b.end());
    joined.insert(joined.end(), c.begin(), c.end());
    Bytes key = seq(33, 0x21);
    for (const auto &entry : hashes) {
        for (bool split : {true, false}) {
            uint8_t *pieces[] = {split ? a.data() : joined.data(), b.data(), c.data()};
            size_t sizes[] = {split ? a.size() : joined.size(), b.size(), c.size()};
            uint8_t count = split ? 3 : 1;
            Bytes digest(static_cast<size_t>(EVP_MD_size(entry.second()))), mac(digest.size());
            int hash_ok = em_crypto_t::platform_hash(entry.second(), count, pieces, sizes, digest.data());
            int mac_ok = em_crypto_t::platform_hmac_hash(entry.second(), key.data(), key.size(), count, pieces, sizes, mac.data());
            require(hash_ok == 1 && mac_ok == 1, "hash/HMAC failed");
            std::string input = split ? "pieces=" + hex(a) + "," + hex(b) + "," + hex(c) : "pieces=" + hex(joined);
            std::string suffix = split ? "-split" : "-concat";
            row(out, "hash", std::string(entry.first) + suffix, input, hash_ok, hex(digest));
            row(out, "hmac", std::string(entry.first) + suffix, "key=" + hex(key) + ";" + input, mac_ok, hex(mac));
        }
    }
}

static void cbc_vectors(std::ostream &out) {
    Bytes key = seq(16), iv = seq(16, 0xa0);
    const std::array<std::pair<const char *, Bytes>, 3> cases{{
        {"empty", {}}, {"aligned", bytes("0123456789abcdef")}, {"non-aligned", bytes("0123456789abcdef--tail")}}};
    Bytes bad;
    for (const auto &entry : cases) {
        Bytes plain = entry.second, cipher(plain.size() + 16);
        uint32_t cipher_len = 0;
        uint8_t dummy = 0;
        int enc = em_crypto_t::platform_cipher_encrypt(EVP_aes_128_cbc(), key.data(), iv.data(),
            plain.empty() ? &dummy : plain.data(), plain.size(), cipher.data(), &cipher_len, false);
        require(enc == 1 && cipher_len <= cipher.size(), "CBC encryption failed");
        cipher.resize(cipher_len);
        Bytes returned = cipher;
        uint32_t len = em_crypto_t::platform_cipher_decrypt(EVP_aes_128_cbc(), key.data(), iv.data(), returned.data(), returned.size(), false);
        require(len == plain.size(), "CBC decryption length changed");
        returned.resize(len);
        require((std::string(entry.first) == "non-aligned") ? returned != plain : returned == plain,
                "CBC returned span did not expose the expected upstream behavior");
        row(out, "cbc", entry.first, "key=" + hex(key) + ";iv=" + hex(iv) + ";plain=" + hex(plain),
            len, "encrypt=" + std::to_string(enc) + ";cipher=" + hex(cipher) + ";span=" + hex(returned) +
            ";rust=accept;rust_plain=" + hex(plain));
        if (std::string(entry.first) == "aligned") bad = cipher;
    }
    // Flip a byte of the previous ciphertext block, making the last padding
    // byte 0; unlike changing the final block, failure is deterministic.
    bad[15] ^= 0x10;
    Bytes returned = bad;
    uint32_t len = em_crypto_t::platform_cipher_decrypt(EVP_aes_128_cbc(), key.data(), iv.data(), returned.data(), returned.size(), false);
    require(len == 0, "CBC malformed padding unexpectedly accepted");
    row(out, "cbc", "malformed-padding", "key=" + hex(key) + ";iv=" + hex(iv) + ";cipher=" + hex(bad), len,
        "span=-;rust=reject;rust_plain=-");
}

static void wrap_vectors(std::ostream &out) {
    Bytes plain = seq(16, 0x30);
    for (size_t key_len : {size_t(16), size_t(24), size_t(32)}) {
        Bytes key = seq(key_len), wrapped(plain.size() + 8), unwrapped(wrapped.size());
        uint32_t wrapped_len = 0, unwrapped_len = 0;
        int enc = em_crypto_t::aes_key_wrap(key.data(), key.size(), plain.data(), plain.size(), wrapped.data(), &wrapped_len);
        require(enc == 1 && wrapped_len == wrapped.size(), "key wrap failed");
        int dec = em_crypto_t::aes_key_unwrap(key.data(), key.size(), wrapped.data(), wrapped.size(), unwrapped.data(), &unwrapped_len);
        require(dec == 1 && unwrapped_len == plain.size(), "key unwrap failed");
        unwrapped.resize(unwrapped_len);
        require(unwrapped == plain, "key unwrap bytes differ");
        std::string inputs = "kek=" + hex(key) + ";plain=" + hex(plain);
        row(out, "keywrap", "wrap-" + std::to_string(key_len * 8), inputs, enc, hex(wrapped));
        row(out, "keywrap", "unwrap-" + std::to_string(key_len * 8), "kek=" + hex(key) + ";wrapped=" + hex(wrapped), dec, hex(unwrapped));
        wrapped[0] ^= 1;
        unwrapped.resize(wrapped.size());
        dec = em_crypto_t::aes_key_unwrap(key.data(), key.size(), wrapped.data(), wrapped.size(), unwrapped.data(), &unwrapped_len);
        require(dec == 0, "corrupt wrap accepted");
        row(out, "keywrap", "tampered-" + std::to_string(key_len * 8), "kek=" + hex(key) + ";wrapped=" + hex(wrapped), dec, "-");
        for (const auto &invalid : std::array<std::pair<const char *, size_t>, 4>{{
                 {"wrap-short", 8}, {"wrap-non-multiple", 17},
                 {"unwrap-short", 16}, {"unwrap-non-multiple", 25}}}) {
            Bytes input = seq(invalid.second, 0x50), output(input.size() + 16);
            uint32_t output_len = 0;
            bool wrapping = std::string(invalid.first).find("wrap-") == 0;
            int result = wrapping ? em_crypto_t::aes_key_wrap(key.data(), key.size(), input.data(), input.size(), output.data(), &output_len) :
                                    em_crypto_t::aes_key_unwrap(key.data(), key.size(), input.data(), input.size(), output.data(), &output_len);
            require(result == 0 && output_len == 0, "invalid key wrap length accepted");
            row(out, "keywrap", std::string(invalid.first) + "-" + std::to_string(key_len * 8),
                "kek=" + hex(key) + (wrapping ? ";plain=" : ";wrapped=") + hex(input), result, "-");
        }
    }
}

// The pinned variadic functions fetch each associated-data pointer as char *.
// A typed adapter makes an accidental unsigned-char pointer a compile error.
static int siv_with_ad(int (*operation)(siv_ctx *, const unsigned char *, unsigned char *, int, unsigned char *, int, ...),
                       siv_ctx *ctx, const uint8_t *input, uint8_t *output, int len, uint8_t *tag,
                       char *ad1, int ad1_len, char *ad2, int ad2_len) {
    return operation(ctx, input, output, len, tag, 2, ad1, ad1_len, ad2, ad2_len);
}
static void siv_vectors(std::ostream &out) {
    Bytes plain = bytes("fixed SIV plaintext with a tail"), ad1 = bytes("first associated piece"), ad2 = seq(17, 0xb0);
    for (size_t key_len : {size_t(32), size_t(48), size_t(64)}) {
        Bytes key = seq(key_len), cipher(plain.size()), decoded(plain.size()), tag(16);
        siv_ctx ctx;
        require(siv_init(&ctx, key.data(), static_cast<int>(key_len * 8)) == 1, "SIV init failed");
        std::unique_ptr<siv_ctx, SivFree> ctx_guard(&ctx);
        int enc = siv_with_ad(siv_encrypt, &ctx, plain.data(), cipher.data(), plain.size(), tag.data(),
                              reinterpret_cast<char *>(ad1.data()), static_cast<int>(ad1.size()),
                              reinterpret_cast<char *>(ad2.data()), static_cast<int>(ad2.size()));
        int dec = siv_with_ad(siv_decrypt, &ctx, cipher.data(), decoded.data(), cipher.size(), tag.data(),
                              reinterpret_cast<char *>(ad1.data()), static_cast<int>(ad1.size()),
                              reinterpret_cast<char *>(ad2.data()), static_cast<int>(ad2.size()));
        require(enc == 1 && dec == 1 && decoded == plain, "SIV round trip failed");
        std::string common = "key=" + hex(key) + ";ad=" + hex(ad1) + "," + hex(ad2);
        row(out, "siv", "encrypt-" + std::to_string(key_len * 8), common + ";plain=" + hex(plain), enc, "tag=" + hex(tag) + ";cipher=" + hex(cipher));
        row(out, "siv", "decrypt-" + std::to_string(key_len * 8), common + ";tag=" + hex(tag) + ";cipher=" + hex(cipher), dec, hex(decoded));
        tag[0] ^= 1;
        dec = siv_with_ad(siv_decrypt, &ctx, cipher.data(), decoded.data(), cipher.size(), tag.data(),
                          reinterpret_cast<char *>(ad1.data()), static_cast<int>(ad1.size()),
                          reinterpret_cast<char *>(ad2.data()), static_cast<int>(ad2.size()));
        require(dec == -1, "SIV accepted altered tag");
        row(out, "siv", "tampered-" + std::to_string(key_len * 8), common + ";tag=" + hex(tag) + ";cipher=" + hex(cipher), dec, hex(decoded));
    }
}

static std::pair<int, Bytes> dh(Bytes peer, Bytes priv) {
    uint8_t *secret = nullptr;
    uint16_t len = 0;
    int ok = em_crypto_t::platform_compute_shared_secret(&secret, &len, peer.data(), peer.size(), priv.data(), priv.size());
    std::unique_ptr<uint8_t, OpenSslFree> guard(secret);
    Bytes result;
    if (secret) result.assign(secret, secret + len);
    return {ok, result};
}
struct DhInputs { Bytes private_key, peer_private, public_key, peer_public; };
static DhInputs fixed_dh_inputs() {
    DhInputs result{seq(32, 0x21), seq(32, 0x61), {}, {}};
    BN p(BN_bin2bn(em_crypto_t::g_dh1536_p, 192, nullptr), BN_free);
    BN generator = bn(2), local(BN_bin2bn(result.private_key.data(), result.private_key.size(), nullptr), BN_free);
    BN remote(BN_bin2bn(result.peer_private.data(), result.peer_private.size(), nullptr), BN_free);
    BN pub(BN_new(), BN_free), peer(BN_new(), BN_free);
    std::unique_ptr<BN_CTX, decltype(&BN_CTX_free)> ctx(BN_CTX_new(), BN_CTX_free);
    require(p && local && remote && pub && peer && ctx &&
            BN_mod_exp(pub.get(), generator.get(), local.get(), p.get(), ctx.get()) == 1 &&
            BN_mod_exp(peer.get(), generator.get(), remote.get(), p.get(), ctx.get()) == 1,
            "fixed DH public keys failed");
    result.public_key = bn_bytes(pub.get(), 192);
    result.peer_public = bn_bytes(peer.get(), 192);
    return result;
}
// Compute the Rust result with BN arithmetic, independently of upstream's
// provider-based derive. The secret uses the same minimal big-endian span.
static void dh_row(std::ostream &out, const std::string &name, const Bytes &remote,
                   const Bytes &priv, const std::pair<int, Bytes> &upstream, const BIGNUM *p) {
    BN peer(BN_bin2bn(remote.data(), remote.size(), nullptr), BN_free);
    BN exponent(BN_bin2bn(priv.data(), priv.size(), nullptr), BN_free);
    BN two = bn(2), upper(BN_dup(p), BN_free), secret(BN_new(), BN_free);
    std::unique_ptr<BN_CTX, decltype(&BN_CTX_free)> ctx(BN_CTX_new(), BN_CTX_free);
    require(peer && exponent && upper && secret && ctx && BN_sub_word(upper.get(), 2) == 1,
            "DH expected result setup failed");
    bool accepted = BN_cmp(peer.get(), two.get()) >= 0 && BN_cmp(peer.get(), upper.get()) <= 0;
    Bytes expected;
    if (accepted) {
        require(BN_mod_exp(secret.get(), peer.get(), exponent.get(), p, ctx.get()) == 1,
                "independent DH exponentiation failed");
        expected = bn_bytes(secret.get());
        if (name == "p-minus-2") {
            BN p_minus_8(BN_dup(p), BN_free);
            require(p_minus_8 && BN_sub_word(p_minus_8.get(), 8) == 1 &&
                    expected == bn_bytes(p_minus_8.get(), 192), "DH p-minus-8 expectation failed");
        }
        if (name == "two") require(expected == Bytes{8}, "DH peer 2 expectation failed");
        if (upstream.first == 1) require(upstream.second == expected, "DH upstream/expected bytes differ");
    }
    require((upstream.first == 1 && !upstream.second.empty() && upstream.second.size() <= 192) ||
            (upstream.first == 0 && upstream.second.empty()), "invalid upstream DH outcome");
    row(out, "dh", name, "private=" + hex(priv) + ";peer=" + hex(remote), upstream.first,
        "upstream=" + hex(upstream.second) + ";rust=" + (accepted ? "accept" : "reject") +
        ";rust_secret=" + hex(expected));
}
static void dh_vectors(std::ostream &out) {
    BN p(BN_bin2bn(em_crypto_t::g_dh1536_p, 192, nullptr), BN_free);
    require(p != nullptr, "DH prime failed");
    Bytes priv{3};
    for (const std::string &name : {"zero", "one", "p-minus-1", "p", "p-plus-1", "p-minus-2", "two"}) {
        BN peer(BN_dup(p.get()), BN_free);
        require(peer != nullptr, "BN_dup failed");
        if (name == "zero") BN_zero(peer.get());
        else if (name == "one") require(BN_one(peer.get()) == 1, "BN_one failed");
        else if (name == "two") require(BN_set_word(peer.get(), 2) == 1, "BN_set_word failed");
        else if (name == "p-minus-1") require(BN_sub_word(peer.get(), 1) == 1, "BN_sub_word failed");
        else if (name == "p-minus-2") require(BN_sub_word(peer.get(), 2) == 1, "BN_sub_word failed");
        else if (name == "p-plus-1") require(BN_add_word(peer.get(), 1) == 1, "BN_add_word failed");
        Bytes remote = bn_bytes(peer.get());
        auto got = dh(remote, priv);
        dh_row(out, name, remote, priv, got, p.get());
    }
    for (uint8_t exp : {uint8_t(8), uint8_t(17)}) {
        Bytes remote{2}, local{exp};
        auto got = dh(remote, local);
        dh_row(out, "leading-zero-exp-" + std::to_string(exp), remote, local, got, p.get());
    }
    DhInputs fixed = fixed_dh_inputs();
    auto full = dh(fixed.peer_public, fixed.private_key);
    auto reverse = dh(fixed.public_key, fixed.peer_private);
    if (full.first && reverse.first) require(full.second == reverse.second, "fixed DH agreement failed");
    dh_row(out, "fixed-agreement", fixed.peer_public, fixed.private_key, full, p.get());
    BN generator = bn(2), exponent = bn(3), pub(BN_new(), BN_free);
    std::unique_ptr<BN_CTX, decltype(&BN_CTX_free)> ctx(BN_CTX_new(), BN_CTX_free);
    require(pub && ctx && BN_mod_exp(pub.get(), generator.get(), exponent.get(), p.get(), ctx.get()) == 1,
            "DH public exponentiation failed");
    Pkey pkey(em_crypto_t::create_dh_pkey(p.get(), generator.get(), exponent.get(), pub.get()), EVP_PKEY_free);
    BIGNUM *exported_raw = nullptr;
    require(pkey && EVP_PKEY_get_bn_param(pkey.get(), OSSL_PKEY_PARAM_PUB_KEY, &exported_raw) == 1,
            "DH public key export failed");
    BN exported(exported_raw, BN_free);
    Bytes padded = bn_bytes(exported.get(), 192);
    require(padded.back() == 8 && std::count(padded.begin(), padded.end(), 0) == 191, "DH padded export failed");
    row(out, "dh", "public-padded-8", "generator=02;private=03;width=192", 192, hex(padded));
}

static void encoding_vectors(std::ostream &out) {
    struct Case { const char *name; std::string input; bool success; std::string expected; };
    const std::vector<Case> cases{
        {"empty", "", true, ""}, {"missing-padding", "TQ", true, "4d"},
        {"existing-padding", "TQ==", true, "4d"}, {"url-alphabet", "-_8", true, "fbff"},
        {"standard-alphabet", "+/8=", true, "fbff"}, {"mixed-alphabets", "-_+/", true, "fbffbf"},
        {"invalid-length", "A", false, ""}, {"malformed-padding", "TQ===", false, ""},
        {"embedded-nul", std::string("TWFu\0TQ==", 9), true, "4d616e"}};
    for (const auto &entry : cases) {
        auto result = em_crypto_t::base64url_decode(entry.input);
        require(result.has_value() == entry.success && (!result || hex(*result) == (entry.expected.empty() ? "-" : entry.expected)),
                std::string("decoder outcome differs from D4: ") + entry.name);
        row(out, "base64url", entry.name, "input=" + text_hex(entry.input) + ";length=" + std::to_string(entry.input.size()),
            result.has_value(), result ? hex(*result) : "-");
    }
    for (const Bytes &input : {Bytes{}, bytes("Man"), Bytes{0xfb, 0xff}}) {
        std::string encoded = em_crypto_t::base64_encode(input.data(), input.size());
        auto decoded = em_crypto_t::base64_decode(encoded);
        require(decoded && *decoded == input, "base64 round trip failed");
        std::string name = input.empty() ? "empty" : input.size() == 3 ? "man" : "alphabet";
        row(out, "base64", name, "input=" + hex(input), 1, "encoded=" + text_hex(encoded) + ";decoded=" + hex(*decoded));
        std::string url = em_crypto_t::base64url_encode(input.data(), input.size());
        auto url_decoded = em_crypto_t::base64url_decode(url);
        require(url_decoded && *url_decoded == input, "base64url round trip failed");
        row(out, "base64url", "encode-" + name, "input=" + hex(input), 1, "encoded=" + text_hex(url) + ";decoded=" + hex(*url_decoded));
    }
}

struct Curve { const char *name; int nid; int width; unsigned private_scalar; const char *signature; };
// Signatures generated once by the extracted sign_data_ecdsa default SHA-256
// path with the fixed keys below, then retained unchanged as harness inputs.
static const std::array<Curve, 5> curves{{
    {"p256", NID_X9_62_prime256v1, 32, 3, "30450220453cfc60c7f7b9a79589b8b8b0e104b91ce23369a9fe139376f0e1b3968c0803022100fab1ae3b1bbf5fd37ede80ca042eb1678d3ec946766fc6678dd94db6eb9085cc"},
    {"p384", NID_secp384r1, 48, 4, "3065023100e37d7edf1846a12db141807b2ce57a8b00a7626afa3cae2f2f809e8202001d7a3a899dc55058959f14dc556f7f1a6a8102303661020d1af18b5b97114ae17f2ad8b62ec4eb7ca386ecce1b13d4763bae375cfbeaa512cdbf9bc2e93e8c69283e229f"},
    {"p521", NID_secp521r1, 66, 5, "308188024201f1c830bf920b0ed64b7d38919e9da808d06e99784a6b68067180d48b30a70b90cad7ed83fa02b58698e0b63568250d05e990f78245411b6e854fc15245d09caeed0242019c38f2e0e6e27073765903156e4907da8c1d74f50cfeab116528f797b74dd4276ff61b8ec1ab26f8d021e7e6db33abff73f5255f747a8d307249b67c5bed0c85b9"},
    {"p192", NID_X9_62_prime192v1, 24, 6, ""},
    {"p224", NID_secp224r1, 28, 7, ""}}};
static const std::string connector = "eyJ0eXAiOiJkcHBDb24iLCJhbGciOiJFUzI1NiJ9.eyJzdWIiOiJmaXhlZC1tZXNoIn0";

static Pkey fixed_key(const Curve &curve) {
    EcKey ec(EC_KEY_new_by_curve_name(curve.nid), EC_KEY_free);
    require(ec != nullptr, "EC_KEY_new failed");
    const EC_GROUP *group = EC_KEY_get0_group(ec.get());
    BN scalar = bn(curve.private_scalar);
    Point pub(EC_POINT_new(group), EC_POINT_free);
    require(pub && EC_POINT_mul(group, pub.get(), scalar.get(), nullptr, nullptr, nullptr) == 1 &&
            EC_KEY_set_private_key(ec.get(), scalar.get()) == 1 && EC_KEY_set_public_key(ec.get(), pub.get()) == 1,
            "fixed EC key failed");
    Pkey key(EVP_PKEY_new(), EVP_PKEY_free);
    require(key && EVP_PKEY_assign_EC_KEY(key.get(), ec.get()) == 1, "EVP_PKEY_assign failed");
    ec.release(); // EVP_PKEY owns the key after a successful assignment.
    return key;
}
static Bytes decode64(const std::string &encoded) {
    auto value = em_crypto_t::base64_decode(encoded);
    require(value.has_value(), "SPKI base64 decode failed");
    return *value;
}
static Bytes point_bytes(const EC_GROUP *group, const EC_POINT *point, int width) {
    auto xy = ec_crypto::get_ec_x_y(group, point);
    BN x(xy.first, BN_free), y(xy.second, BN_free);
    require(x && y, "coordinates unavailable");
    Bytes result = bn_bytes(x.get(), width), second = bn_bytes(y.get(), width);
    result.insert(result.end(), second.begin(), second.end());
    return result;
}
static Bytes spki_point(const Bytes &der) {
    const unsigned char *cursor = der.data();
    Spki spki(d2i_X509_PUBKEY(nullptr, &cursor, der.size()), X509_PUBKEY_free);
    require(spki && cursor == der.data() + der.size(), "invalid SPKI DER");
    const unsigned char *point = nullptr;
    int len = 0;
    int ok = X509_PUBKEY_get0_param(nullptr, &point, &len, nullptr, spki.get());
    require(ok == 1 && point && len > 0, "missing SPKI subjectPublicKey");
    return Bytes(point, point + len);
}
static void spki_row(std::ostream &out, const std::string &name, const std::string &input,
                     const std::string &description, int width, bool compressed) {
    Pkey imported(em_crypto_t::ec_key_from_base64_der(input), EVP_PKEY_free);
    require(imported != nullptr, "SPKI import failed: " + name);
    std::string exported = em_crypto_t::ec_key_to_base64_der(imported.get());
    require(!exported.empty(), "SPKI export failed: " + name);
    Bytes in_der = decode64(input), out_der = decode64(exported);
    require(in_der == out_der, "SPKI round-trip changed: " + name);
    Bytes point = spki_point(out_der);
    require(compressed ? (point.size() == static_cast<size_t>(1 + width) && (point[0] == 2 || point[0] == 3)) :
            (point.size() == static_cast<size_t>(1 + 2 * width) && point[0] == 4), "unexpected fixed SPKI form: " + name);
    row(out, "spki", name, description + ";der=" + hex(in_der), 1,
        "upstream=" + hex(out_der) + ";rust=" + ((name == "p192" || name == "p224") ? "unsupported-curve" : "accept"));
    for (const char *prefix : {static_cast<const char *>(nullptr), "chirp"}) {
        std::unique_ptr<uint8_t, decltype(&free)> hash(ec_crypto::compute_key_hash(imported.get(), prefix), free);
        require(hash != nullptr, "boot key hash failed");
        row(out, "bootkey", name + (prefix ? "-prefixed" : "-plain"),
            "der=" + hex(out_der) + ";prefix=" + (prefix ? text_hex(prefix) : "-"), 1, hex(hash.get(), 32));
    }
}
static void ec_vectors(std::ostream &out) {
    for (size_t i = 0; i < curves.size(); ++i) {
        const Curve &curve = curves[i];
        Pkey key = fixed_key(curve);
        std::string spki = em_crypto_t::ec_key_to_base64_der(key.get());
        require(!spki.empty(), "fixed SPKI export failed");
        spki_row(out, curve.name, spki, "curve=" + std::string(curve.name) + ";private=" + hex(bn_bytes(bn(curve.private_scalar).get())), curve.width, false);
        if (i >= 3) continue;
        Group group(EC_GROUP_new_by_curve_name(curve.nid), EC_GROUP_free);
        require(group != nullptr, "EC group failed");
        BN two = bn(2), three = bn(3), five = bn(5), seven = bn(7);
        Point p2(EC_POINT_new(group.get()), EC_POINT_free), p5(EC_POINT_new(group.get()), EC_POINT_free);
        Point multiplied(EC_POINT_new(group.get()), EC_POINT_free), sum(EC_POINT_new(group.get()), EC_POINT_free);
        require(p2 && p5 && multiplied && sum &&
                EC_POINT_mul(group.get(), p2.get(), two.get(), nullptr, nullptr, nullptr) == 1 &&
                EC_POINT_mul(group.get(), p5.get(), five.get(), nullptr, nullptr, nullptr) == 1 &&
                EC_POINT_mul(group.get(), multiplied.get(), nullptr, p5.get(), three.get(), nullptr) == 1 &&
                EC_POINT_add(group.get(), sum.get(), p2.get(), p5.get(), nullptr) == 1, "EC point operations failed");
        Bytes peer = point_bytes(group.get(), p5.get(), curve.width);
        // The verbatim upstream helper leaks its temporary EC_POINT, one per
        // call. It is deliberately left process-scoped until the pin is fixed.
        BN x(ec_crypto::compute_ec_ss_x(group.get(), three.get(), p5.get()), BN_free);
        require(x != nullptr, "ECDH x-coordinate failed");
        row(out, "ecdh", curve.name, "private=03;peer_xy=" + hex(peer), 1, hex(bn_bytes(x.get(), curve.width)));
        row(out, "ec-point", std::string(curve.name) + "-multiply", "scalar=03;point_xy=" + hex(peer), 1, hex(point_bytes(group.get(), multiplied.get(), curve.width)));
        row(out, "ec-point", std::string(curve.name) + "-add", "left_xy=" + hex(point_bytes(group.get(), p2.get(), curve.width)) + ";right_xy=" + hex(peer), 1, hex(point_bytes(group.get(), sum.get(), curve.width)));
        BN zero = bn(0);
        Point invalid(EC_POINT_new(group.get()), EC_POINT_free);
        require(invalid != nullptr, "EC point allocation failed");
        int on_curve = EC_POINT_set_affine_coordinates(group.get(), invalid.get(), zero.get(), zero.get(), nullptr);
        require(on_curve == 0, "invalid EC point accepted");
        Bytes invalid_xy(static_cast<size_t>(2 * curve.width));
        row(out, "ec-point", std::string(curve.name) + "-invalid", "point_xy=" + hex(invalid_xy), on_curve, "-");
        BN order(BN_new(), BN_free), left(BN_new(), BN_free), added(BN_new(), BN_free);
        std::unique_ptr<BN_CTX, decltype(&BN_CTX_free)> bn_ctx(BN_CTX_new(), BN_CTX_free);
        require(order && left && added && bn_ctx && EC_GROUP_get_order(group.get(), order.get(), nullptr) == 1 &&
                BN_copy(left.get(), order.get()) && BN_sub_word(left.get(), 1) == 1 &&
                BN_mod_add(added.get(), left.get(), three.get(), order.get(), bn_ctx.get()) == 1, "EC scalar add failed");
        row(out, "ec-scalar", curve.name, "left=" + hex(bn_bytes(left.get(), curve.width)) + ";right=03;order=" + hex(bn_bytes(order.get(), curve.width)), 1, hex(bn_bytes(added.get(), curve.width)));
        Bytes message = bytes(connector), signature = unhex(curve.signature);
        Pkey public_key(em_crypto_t::ec_key_from_base64_der(spki), EVP_PKEY_free);
        require(public_key && !signature.empty() && em_crypto_t::verify_signature(message, signature, public_key.get(), EVP_sha256()), "fixed connector signature failed");
        row(out, "signature", curve.name, "message=" + hex(message) + ";spki=" + hex(decode64(spki)) + ";hash=sha256", 1, hex(signature));
        Bytes changed = message; changed[0] ^= 1;
        require(!em_crypto_t::verify_signature(changed, signature, public_key.get(), EVP_sha256()), "changed connector accepted");
        signature.back() ^= 1;
        require(!em_crypto_t::verify_signature(message, signature, public_key.get(), EVP_sha256()), "changed signature accepted");
    }
    const std::string uri_key = "MDkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDIgADM2206avxHJaHXgLMkq/24e0rsrfMP9K1Tm8gx+ovP0I=";
    spki_row(out, "uri-compressed-p256", uri_key, "curve=p256;source=ec_util.cpp:760", 32, true);
}
static void make_signatures() {
    Bytes message = bytes(connector);
    for (size_t i = 0; i < 3; ++i) {
        Pkey key = fixed_key(curves[i]);
        auto signature = em_crypto_t::sign_data_ecdsa(message, key.get());
        require(signature.has_value() && em_crypto_t::verify_signature(message, *signature, key.get(), EVP_sha256()), "signing failed");
        std::cout << curves[i].name << '\t' << hex(*signature) << '\n';
    }
}
static void generated_form() {
    provenance(std::cout);
    for (size_t i = 0; i < 3; ++i) {
        const Curve &curve = curves[i];
        Pkey key(em_crypto_t::generate_ec_key(curve.nid), EVP_PKEY_free);
        require(key != nullptr, "upstream EC key generation failed");
        Bytes der = decode64(em_crypto_t::ec_key_to_base64_der(key.get()));
        Bytes point = spki_point(der);
        bool valid = point.size() == static_cast<size_t>(1 + 2 * curve.width) && point[0] == 4;
        require(valid, std::string("unexpected generated SEC1 form: ") + curve.name);
        const OSSL_PROVIDER *key_provider = EVP_PKEY_get0_provider(key.get());
        require(key_provider != nullptr, "generated key has no provider");
        const char *key_provider_name = OSSL_PROVIDER_get0_name(key_provider);
        require(key_provider_name != nullptr, "generated key provider has no name");
        std::cout << curve.name << "\tprovider=" << key_provider_name << "\tpoint-form=uncompressed\tder=" << hex(der) << '\n';
    }
}

static void wsc_vectors(std::ostream &out) {
    DhInputs fixed = fixed_dh_inputs();
    Bytes priv = fixed.private_key, peer = fixed.peer_public;
    Bytes enrollee_nonce = seq(16, 0x10), mac = seq(6, 0x20), registrar_nonce = seq(16, 0x30);
    auto shared = dh(peer, priv);
    require(shared.first == 1, "WSC DH failed");
    row(out, "wsc", "dh-secret", "private=" + hex(priv) + ";peer=" + hex(peer), shared.first, hex(shared.second));
    Bytes dhkey(32), kdk(32), keys(80);
    uint8_t *parts[] = {shared.second.data(), enrollee_nonce.data(), mac.data(), registrar_nonce.data()};
    size_t lengths[] = {shared.second.size(), enrollee_nonce.size(), mac.size(), registrar_nonce.size()};
    int ok = em_crypto_t::platform_SHA256(1, parts, lengths, dhkey.data());
    require(ok == 1, "WSC DH hash failed");
    row(out, "wsc", "dh-hash", "secret=" + hex(shared.second), ok, hex(dhkey));
    ok = em_crypto_t::platform_hmac_SHA256(dhkey.data(), dhkey.size(), 3, parts + 1, lengths + 1, kdk.data());
    require(ok == 1, "WSC KDK failed");
    row(out, "wsc", "kdk", "dhkey=" + hex(dhkey) + ";enrollee_nonce=" + hex(enrollee_nonce) + ";mac=" + hex(mac) + ";registrar_nonce=" + hex(registrar_nonce), ok, hex(kdk));
    char label[] = "Wi-Fi Easy and Secure Key Derivation";
    ok = em_crypto_t::wps_key_derivation_function(kdk.data(), nullptr, 0, label, keys.data(), keys.size());
    require(ok == 1, "WSC KDF failed");
    row(out, "wsc", "keys", "kdk=" + hex(kdk) + ";prefix=-;label=" + text_hex(label) + ";length=80", ok,
        "all=" + hex(keys) + ";auth=" + hex(keys.data(), 32) + ";wrap=" + hex(keys.data() + 32, 16) + ";emsk=" + hex(keys.data() + 48, 32));
}

static void verify_external(const char *message_hex, const char *spki_hex, const char *signature_hex) {
    Bytes message = unhex(message_hex), der = unhex(spki_hex), signature = unhex(signature_hex);
    require(der.size() <= 1023, "SPKI DER exceeds the pinned importer's 1024-byte buffer");
    std::string encoded = em_crypto_t::base64_encode(der.data(), der.size());
    Pkey public_key(em_crypto_t::ec_key_from_base64_der(encoded), EVP_PKEY_free);
    require(public_key != nullptr, "cannot import verification key");
    bool result = em_crypto_t::verify_signature(message, signature, public_key.get(), EVP_sha256());
    std::cout << "verified=" << (result ? 1 : 0) << '\n';
}

int main(int argc, char **argv) {
    try {
        require(argc >= 2, "invalid arguments");
        const std::string mode(argv[1]);
        require(mode == "--verify-signature" ? argc == 5 : argc == 2, "invalid arguments");
        Provider provider(OSSL_PROVIDER_load(nullptr, "default"), OSSL_PROVIDER_unload);
        require(provider != nullptr, "cannot load default provider");
        if (std::string(argv[1]) == "--verify-signature") verify_external(argv[2], argv[3], argv[4]);
        else if (std::string(argv[1]) == "--generated-form") generated_form();
        else if (std::string(argv[1]) == "--make-signatures") make_signatures();
        else {
            std::ofstream out(argv[1], std::ios::binary);
            require(static_cast<bool>(out), "cannot open output");
            provenance(out);
            kdf_vectors(out); digest_vectors(out); cbc_vectors(out); wrap_vectors(out); siv_vectors(out);
            dh_vectors(out); encoding_vectors(out); ec_vectors(out); wsc_vectors(out);
            out.flush();
            require(static_cast<bool>(out), "cannot flush output");
        }
        return 0;
    } catch (const std::exception &e) {
        std::cerr << "oracle error: " << e.what() << '\n';
        return 2;
    }
}
