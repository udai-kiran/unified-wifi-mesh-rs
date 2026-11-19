// SPDX-License-Identifier: Apache-2.0

//! Native-constructor limits of the field types the records use. Each test first proves, with a
//! typed closure, that the record field really has the type under test, so the test cannot drift
//! from the record.

use crate::limits::{
    CLIENT_ID_CAP, CLIENT_SECRET_CAP, LONG_STRING_CAP, SHORT_STRING_CAP, SSID_CAP, STRING_CAP,
};
use crate::{
    ApHtCap, ApVhtCap, BoundedVec, CStrBytes, CacCapMethod, CapacityError, ExtDeviceInfo, LenBytes,
    NetworkInfo, NetworkSsidInfo, RadioId, Secret, Ssid2VidMapInfo, StringError, Text, UBits,
};

#[test]
fn ssid_native_capacity_is_32_bytes() {
    let _: fn(&Ssid2VidMapInfo) -> &CStrBytes<SSID_CAP> = |r| &r.ssid;
    assert_eq!(SSID_CAP, 32);
    let full = [b'm'; 32];
    assert_eq!(
        CStrBytes::<SSID_CAP>::new(&full).map(|s| s.len()),
        Ok(32),
        "32 bytes kept whole"
    );
    assert_eq!(
        CStrBytes::<SSID_CAP>::new(&[b'm'; 33]),
        Err(StringError::TooLong {
            len: 33,
            capacity: 32
        })
    );
}

#[test]
fn ssid_keeps_non_utf8_and_rejects_the_terminator() {
    let raw = [0xc3, 0x28, 0xff, 0x80];
    let ssid = CStrBytes::<SSID_CAP>::new(&raw).unwrap();
    assert_eq!(ssid.as_bytes(), &raw);
    assert_eq!(
        CStrBytes::<SSID_CAP>::new(b"mesh\0pad"),
        Err(StringError::EmbeddedNul { position: 4 })
    );
}

#[test]
fn network_id_is_text_with_the_c_payload_limit() {
    let _: fn(&NetworkInfo) -> &Text<LONG_STRING_CAP> = |r| &r.id;
    let _: fn(&RadioId) -> &Text<LONG_STRING_CAP> = |r| &r.net_id;
    assert_eq!(LONG_STRING_CAP, 127, "char[128] holds 127 payload bytes");
    assert!(Text::<LONG_STRING_CAP>::new(&"n".repeat(127)).is_ok());
    assert_eq!(
        Text::<LONG_STRING_CAP>::new(&"n".repeat(128)),
        Err(StringError::TooLong {
            len: 128,
            capacity: 127
        })
    );
    assert_eq!(
        Text::<LONG_STRING_CAP>::from_bytes(&[0xff]),
        Err(StringError::NotUtf8)
    );
    assert_eq!(
        Text::<LONG_STRING_CAP>::new("a\0b"),
        Err(StringError::EmbeddedNul { position: 1 })
    );
}

#[test]
fn extended_device_credentials_are_length_delimited_bytes() {
    let _: fn(&ExtDeviceInfo) -> &Secret<LenBytes<CLIENT_ID_CAP>> = |r| &r.client_id;
    let _: fn(&ExtDeviceInfo) -> &Secret<LenBytes<CLIENT_SECRET_CAP>> = |r| &r.client_secret;
    // char[256]: the reader appends a NUL (em_configuration.cpp:1308), so the payload is 255.
    assert_eq!(CLIENT_ID_CAP, 255);
    assert_eq!(CLIENT_SECRET_CAP, 255);
    // Any byte value is accepted, including NUL and non-UTF-8 bytes.
    let mut full = [0u8; 255];
    full[1] = 0xff;
    assert_eq!(
        LenBytes::<CLIENT_ID_CAP>::new(&full).map(|v| v.len()),
        Ok(255)
    );
    assert_eq!(
        LenBytes::<CLIENT_SECRET_CAP>::new(&full).map(|v| v.len()),
        Ok(255)
    );
    assert_eq!(
        LenBytes::<CLIENT_ID_CAP>::new(&[0u8; 256]),
        Err(StringError::TooLong {
            len: 256,
            capacity: 255
        })
    );
}

#[test]
fn akm_list_holds_ten_entries_of_63_bytes() {
    type Akm = CStrBytes<SHORT_STRING_CAP>;
    let _: fn(&NetworkSsidInfo) -> &BoundedVec<Akm, 10> = |r| &r.akm;
    let _: fn(&NetworkSsidInfo) -> &BoundedVec<Akm, 3> = |r| &r.band;
    assert_eq!(SHORT_STRING_CAP, 63);
    assert!(Akm::new(&[b'a'; 63]).is_ok());
    assert_eq!(
        Akm::new(&[b'a'; 64]),
        Err(StringError::TooLong {
            len: 64,
            capacity: 63
        })
    );

    let mut list = BoundedVec::<Akm, 10>::new();
    for _ in 0..10 {
        list.push(Akm::new(b"SAE").unwrap()).unwrap();
    }
    assert_eq!(
        list.push(Akm::new(b"extra").unwrap()),
        Err(CapacityError {
            capacity: 10,
            requested: 11
        })
    );
    assert_eq!(list.len(), 10, "a rejected push keeps the list");
}

#[test]
fn disallowed_sta_lists_hold_ten_mac_strings() {
    type MacText = CStrBytes<STRING_CAP>;
    let _: fn(&NetworkInfo) -> &BoundedVec<MacText, 10> = |r| &r.mscs_disallowed_sta;
    let _: fn(&NetworkInfo) -> &BoundedVec<MacText, 10> = |r| &r.scs_disallowed_sta;
    assert_eq!(STRING_CAP, 31);
    assert!(MacText::new(&[b'0'; 31]).is_ok());
    assert!(MacText::new(&[b'0'; 32]).is_err());
}

#[test]
fn nested_lists_enforce_their_own_capacity() {
    let _: fn(&CacCapMethod) -> &BoundedVec<crate::CacOpClass, 48> = |r| &r.op_classes;
    let mut list = BoundedVec::<u8, 48>::new();
    for value in 0..48 {
        list.push(value).unwrap();
    }
    assert!(list.push(48).is_err());
}

#[test]
fn bit_fields_reject_values_at_two_to_the_width() {
    let _: fn(&ApHtCap) -> &UBits<2> = |r| &r.max_sprt_rx_streams;
    let _: fn(&ApVhtCap) -> &UBits<3> = |r| &r.max_sprt_rx_streams;
    let _: fn(&CacCapMethod) -> &UBits<24> = |r| &r.cac_duration;
    assert!(UBits::<2>::new(3).is_ok() && UBits::<2>::new(4).is_err());
    assert!(UBits::<3>::new(7).is_ok() && UBits::<3>::new(8).is_err());
    assert!(UBits::<24>::new((1 << 24) - 1).is_ok() && UBits::<24>::new(1 << 24).is_err());
}
