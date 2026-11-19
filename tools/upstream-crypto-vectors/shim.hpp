/*
 * SPDX-License-Identifier: Apache-2.0
 *
 * Minimal declarations for definitions extracted at build time from
 * rdkcentral/unified-wifi-mesh @ ba4180ba36748629f10c20babc4320dd69c9db4f.
 * The original headers depend on unavailable OneWifi headers. No function
 * bodies or constants from upstream are copied into this file.
 */
#pragma once

#include <algorithm>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <iostream>
#include <optional>
#include <string>
#include <utility>
#include <vector>
#include <openssl/aes.h>
#include <openssl/bio.h>
#include <openssl/bn.h>
#include <openssl/core_names.h>
#include <openssl/ec.h>
#include <openssl/err.h>
#include <openssl/evp.h>
#include <openssl/kdf.h>
#include <openssl/param_build.h>
#include <openssl/provider.h>
#include <openssl/sha.h>
#include <openssl/x509.h>
#include "aes_siv.h"

#if OPENSSL_VERSION_NUMBER < 0x30000000L
#error OpenSSL 3 is required
#endif

using SSL_KEY = EVP_PKEY;
#define em_printfout(...) do { std::fprintf(stderr, __VA_ARGS__); std::fputc('\n', stderr); } while (0)
#include "constants.inc"
#include "endian.inc"

class em_crypto_t {
public:
    static uint8_t g_dh1536_p[], g_dh1536_g[];
    static uint8_t platform_hash(const EVP_MD *, uint8_t, uint8_t **, size_t *, uint8_t *);
    static uint8_t platform_hmac_hash(const EVP_MD *, uint8_t *, size_t, uint8_t, uint8_t **, size_t *, uint8_t *);
#include "em_inline.inc"
    static void append_u32_net(const uint32_t *, uint8_t **);
    static uint8_t wps_key_derivation_function(uint8_t *, uint8_t *, size_t, char *, uint8_t *, uint32_t);
    static uint8_t platform_cipher_encrypt(const EVP_CIPHER *, uint8_t *, uint8_t *, uint8_t *, uint32_t, uint8_t *, uint32_t *, bool = false);
    static uint32_t platform_cipher_decrypt(const EVP_CIPHER *, uint8_t *, uint8_t *, uint8_t *, uint32_t, bool = false);
    static uint8_t aes_key_wrap(uint8_t *, size_t, uint8_t *, uint32_t, uint8_t *, uint32_t *);
    static uint8_t aes_key_unwrap(uint8_t *, size_t, uint8_t *, uint32_t, uint8_t *, uint32_t *);
    static bool kdf_hash_length(const EVP_MD *, const uint8_t *, size_t, const char *, const uint8_t *, size_t, uint8_t *, size_t);
    static EVP_PKEY *create_dh_pkey(BIGNUM *, BIGNUM *, BIGNUM *, BIGNUM *);
    static uint8_t platform_compute_shared_secret(uint8_t **, uint16_t *, uint8_t *, uint16_t, uint8_t *, uint8_t);
    static void cleanup_bignums(BIGNUM *, BIGNUM *, BIGNUM *, BIGNUM *);
    static uint8_t compute_secret_internal(BIGNUM *, BIGNUM *, BIGNUM *, BIGNUM *, uint8_t **, size_t *);
    static std::string base64_encode(const uint8_t *, size_t);
    static std::string base64url_encode(const uint8_t *, size_t);
    static std::optional<std::vector<uint8_t>> base64_decode(const std::string &);
    static std::optional<std::vector<uint8_t>> base64url_decode(const std::string &);
    static SSL_KEY *ec_key_from_base64_der(const std::string &);
    static std::string ec_key_to_base64_der(const SSL_KEY *);
    static SSL_KEY *generate_ec_key(int);
    static std::optional<std::vector<uint8_t>> sign_data_ecdsa(const std::vector<uint8_t> &, EVP_PKEY *, const EVP_MD * = EVP_sha256());
    static bool verify_signature(const std::vector<uint8_t> &, const std::vector<uint8_t> &, EVP_PKEY *, const EVP_MD *);
};

class ec_crypto {
public:
#include "ec_inline.inc"
    static uint8_t *compute_key_hash(const SSL_KEY *, const char *);
    static size_t hkdf(const EVP_MD *, bool, uint8_t *, size_t, uint8_t *, size_t, uint8_t *, size_t, uint8_t *, size_t);
    static std::vector<uint8_t> gen_psk(const std::string &, const std::string &);
};
