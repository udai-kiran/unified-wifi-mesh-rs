// SPDX-License-Identifier: Apache-2.0

//! Payload capacities of the C string aliases, in bytes.
//!
//! A C `char[N]` holds `N - 1` payload bytes plus the terminator, so `em_long_string_t`
//! (`char[128]`) is [`LONG_STRING_CAP`] = 127. The golden test checks every constant against
//! the `<alias>` rows of `tests/data/dm_inventory.tsv`.

/// `em_string_t`: `char[32]` (`inc/em_base.h:418`).
pub const STRING_CAP: usize = 31;

/// `em_short_string_t`: `char[64]` (`inc/em_base.h:413`).
pub const SHORT_STRING_CAP: usize = 63;

/// `em_long_string_t`: `char[128]` (`inc/em_base.h:414`).
pub const LONG_STRING_CAP: usize = 127;

/// `em_small_string_t`: `char[16]` (`inc/em_base.h:419`).
pub const SMALL_STRING_CAP: usize = 15;

/// `em_tiny_string_t`: `char[4]` (`inc/em_base.h:420`).
pub const TINY_STRING_CAP: usize = 3;

/// `ssid_t`: `char[32]`, so 31 payload bytes in C. The native capacity is 32 bytes, the
/// 802.11 maximum (0007 amendment A1): a 32-byte SSID without a terminator is kept whole.
pub const SSID_CAP: usize = 32;

/// `em_ext_device_info_t.client_id`: `char[EMEX_MAX_CLIENT_ID_LEN + 1]` = `char[256]`
/// (`inc/em_base.h:2563`). The reader copies at most 255 bytes (`client_id_len` is an
/// `unsigned char`, `inc/em_base.h:2562`), then writes a NUL at `[len]`
/// (`src/em/config/em_configuration.cpp:1308`). The member is therefore length-delimited raw
/// bytes whose C payload is `N - 1` = 255; the reader appends the terminator.
pub const CLIENT_ID_CAP: usize = 255;

/// `em_ext_device_info_t.client_secret`: `char[EMEX_MAX_CLIENT_SEC_LEN + 1]` = `char[256]`
/// (`inc/em_base.h:2565`); the NUL is written at `src/em/config/em_configuration.cpp:1321`.
/// See [`CLIENT_ID_CAP`].
pub const CLIENT_SECRET_CAP: usize = 255;

/// `EM_MAX_FRAME_BODY_LEN`: `11454 - 24 - 4` = 11 426 bytes (`inc/em_base.h:74`), the cap of
/// `em_sta_info_t.frame_body`: the largest VHT MPDU less the shortest management header and FCS.
pub const EM_MAX_FRAME_BODY_LEN: usize = 11_426;
