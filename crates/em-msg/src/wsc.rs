// SPDX-License-Identifier: Apache-2.0

//! WSC Message Type selector (upstream `em_configuration_t::get_wsc_msg_type`,
//! `em_configuration.cpp:6053-6092`).
//!
//! An AP-Autoconfiguration WSC message carries a WSC TLV whose attributes
//! (2-byte id, 2-byte length, value; big-endian) include the Message Type
//! attribute `0x1022`. Its first value byte says M1 or M2.
//!
//! The module also has the attribute walker ([`WscAttrIter`]) and writer
//! ([`write_wsc_attr`]) that the WSC bodies are built on (note 0009, D5), and the
//! typed attributes ([`WscAttribute`]) that upstream's M1, M2 and Encrypted Settings
//! plaintext build or read.

use em_types::{MacAddr, TlvType};

use crate::body::{Reader, Writer};
use crate::error::{DecodeError, EncodeError};
use crate::tlv::TlvIter;

/// WSC message type M1 (`em_wsc_msg_type_m1`).
pub const WSC_M1: u8 = 4;
/// WSC message type M2 (`em_wsc_msg_type_m2`).
pub const WSC_M2: u8 = 5;

/// Attribute id of the WSC Message Type attribute (`attr_id_msg_type`).
const ATTR_ID_MSG_TYPE: u16 = 0x1022;
/// Length of an attribute header (id and length), in bytes.
const ATTR_HEADER_LEN: usize = 4;

/// Returns the first WSC Message Type value found in `tlv_area`.
///
/// The walk follows upstream: TLVs are walked until End of Message, and a TLV
/// whose length runs past the buffer ends the walk with `None`. In each WSC
/// TLV the attributes are walked; an attribute that runs past its TLV ends
/// the walk of that TLV only. The first Message Type attribute with a value of
/// at least one byte wins; one with length 0 is skipped.
#[must_use]
pub fn wsc_msg_type(tlv_area: &[u8]) -> Option<u8> {
    for item in TlvIter::new(tlv_area) {
        let Ok(tlv) = item else {
            return None;
        };
        if tlv.tlv_type == TlvType::WSC {
            if let Some(msg_type) = attribute_msg_type(tlv.value) {
                return Some(msg_type);
            }
        }
    }
    None
}

/// One WSC attribute: id and value, without the 4-byte attribute header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WscAttr<'a> {
    /// Attribute id (big-endian on the wire).
    pub id: u16,
    /// Value bytes.
    pub value: &'a [u8],
}

/// Walks the attributes of a WSC TLV value (or of decrypted settings): 2-byte id,
/// 2-byte length, value, all big-endian.
///
/// An empty input yields nothing. An attribute whose header or value is cut
/// short yields [`DecodeError::Truncated`]; after an error the iterator yields
/// nothing more. Unlike [`wsc_msg_type`], which follows upstream and ends a
/// walk quietly at a short attribute, this walker reports the error.
#[derive(Debug)]
pub struct WscAttrIter<'a> {
    reader: Reader<'a>,
    finished: bool,
}

impl<'a> WscAttrIter<'a> {
    /// Starts a walk over `value`.
    #[must_use]
    pub fn new(value: &'a [u8]) -> Self {
        Self {
            reader: Reader::new(value),
            finished: false,
        }
    }

    fn read_attr(&mut self) -> Result<WscAttr<'a>, DecodeError> {
        let id = self.reader.be_u16()?;
        let len = usize::from(self.reader.be_u16()?);
        let value = self.reader.bytes(len)?;
        Ok(WscAttr { id, value })
    }
}

impl<'a> Iterator for WscAttrIter<'a> {
    type Item = Result<WscAttr<'a>, DecodeError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.finished || self.reader.remaining() == 0 {
            return None;
        }
        let result = self.read_attr();
        if result.is_err() {
            self.finished = true;
        }
        Some(result)
    }
}

/// Writes one WSC attribute: id, length, value.
///
/// # Errors
///
/// [`EncodeError::CountTooLarge`] if `value` is longer than 65535 bytes; nothing is written.
pub fn write_wsc_attr(writer: &mut Writer, id: u16, value: &[u8]) -> Result<(), EncodeError> {
    let Ok(len) = u16::try_from(value.len()) else {
        return Err(EncodeError::CountTooLarge {
            len: value.len(),
            max: usize::from(u16::MAX),
        });
    };
    writer.be_u16(id);
    writer.be_u16(len);
    writer.bytes(value);
    Ok(())
}

open_code! {
    /// WSC attribute id (upstream `data_elem_attr_id_t`, `em_base.h:2262-2330`).
    ///
    /// The constants are the 32 attributes that upstream's M1, M2 and Encrypted Settings
    /// plaintext build or read. Other ids are kept as found.
    pub struct WscAttrId(u16);
    const HEX_WIDTH: usize = 4;
    entries: [
        (VERSION, 0x104A, "Version (`attr_id_version`), 1 octet."),
        (MESSAGE_TYPE, 0x1022, "Message Type (`attr_id_msg_type`), 1 octet."),
        (UUID_E, 0x1047, "UUID-E, 16 octets."),
        (UUID_R, 0x1048, "UUID-R, 16 octets."),
        (MAC_ADDRESS, 0x1020, "MAC Address, 6 octets."),
        (ENROLLEE_NONCE, 0x101A, "Enrollee Nonce, 16 octets."),
        (REGISTRAR_NONCE, 0x1039, "Registrar Nonce, 16 octets."),
        (PUBLIC_KEY, 0x1032, "Public Key, variable."),
        (AUTHENTICATION_TYPE_FLAGS, 0x1004, "Authentication Type Flags, big-endian `u16`."),
        (ENCRYPTION_TYPE_FLAGS, 0x1010, "Encryption Type Flags, big-endian `u16`."),
        (CONNECTION_TYPE_FLAGS, 0x100D, "Connection Type Flags, 1 octet."),
        (CONFIGURATION_METHODS, 0x1008, "Configuration Methods, big-endian `u16`."),
        (WSC_STATE, 0x1044, "Wi-Fi Simple Configuration State, 1 octet."),
        (MANUFACTURER, 0x1021, "Manufacturer, variable."),
        (MODEL_NAME, 0x1023, "Model Name, variable."),
        (MODEL_NUMBER, 0x1024, "Model Number, variable."),
        (SERIAL_NUMBER, 0x1042, "Serial Number, variable."),
        (PRIMARY_DEVICE_TYPE, 0x1054, "Primary Device Type, 8 octets."),
        (DEVICE_NAME, 0x1011, "Device Name, variable."),
        (RF_BANDS, 0x103C, "RF Bands, 1 octet of band flags."),
        (ASSOCIATION_STATE, 0x1002, "Association State, big-endian `u16`."),
        (DEVICE_PASSWORD_ID, 0x1012, "Device Password ID, big-endian `u16`."),
        (CONFIGURATION_ERROR, 0x1009, "Configuration Error, big-endian `u16`."),
        (OS_VERSION, 0x102D, "OS Version, 4 octets."),
        (ENCRYPTED_SETTINGS, 0x1018, "Encrypted Settings, variable (IV and ciphertext)."),
        (AUTHENTICATOR, 0x1005, "Authenticator, 8 octets."),
        (VENDOR_EXTENSION, 0x1049, "Vendor Extension: 3-octet vendor id and vendor data."),
        (SSID, 0x1045, "SSID, variable (upstream sends a trailing NUL)."),
        (AUTHENTICATION_TYPE, 0x1003, "Authentication Type, big-endian `u16`."),
        (ENCRYPTION_TYPE, 0x100F, "Encryption Type, big-endian `u16`."),
        (NETWORK_KEY, 0x1027, "Network Key, variable (upstream sends a trailing NUL)."),
        (KEY_WRAP_AUTHENTICATOR, 0x101E, "Key Wrap Authenticator, 8 octets."),
    ]
}

/// Vendor id of the Wi-Fi Alliance (Multi-AP Extension).
pub const WFA_VENDOR_ID: [u8; 3] = [0x00, 0x37, 0x2A];
/// Vendor id of Comcast (haul-type extension, `comcast_vendor_oui`, `em_base.h:3907`).
pub const COMCAST_VENDOR_ID: [u8; 3] = [0xD8, 0x9C, 0x8E];

/// Sub-element id of the Multi-AP Extension (`EM_MULTI_AP_EXT_SUBELEM_ID`, `em_base.h:168`).
const MULTI_AP_SUBELEMENT_ID: u8 = 0x06;
/// Length of the Multi-AP Extension value (`EM_MULTI_AP_EXT_SUBELEM_LEN`, `em_base.h:167`).
const MULTI_AP_SUBELEMENT_LEN: u8 = 0x01;
/// Sub-id of the haul type (`vendor_ext_attr_id_haul_type`, `em_base.h:2334`).
const HAUL_TYPE_SUB_ID: u8 = 0x01;

/// A Vendor Extension attribute value: vendor id and the vendor's bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WscVendorExtension<'a> {
    /// Vendor id (IEEE OUI).
    pub vendor_id: [u8; 3],
    /// Vendor data after the vendor id.
    pub data: &'a [u8],
}

/// One typed WSC attribute.
///
/// [`decode`](Self::decode) turns a [`WscAttr`] from [`WscAttrIter`] into a typed value and
/// [`encode`](Self::encode) writes it back. Fixed-width attributes must have exactly their
/// width; variable-length attributes take any length. Two-octet values are big-endian.
/// Strings and keys are kept as raw bytes, so the trailing NUL that upstream sends on SSID
/// and Network Key stays in place. An id that has no variant decodes as [`Other`](Self::Other).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WscAttribute<'a> {
    /// Version, 1 octet.
    Version(u8),
    /// Message Type, 1 octet ([`WSC_M1`], [`WSC_M2`]).
    MessageType(u8),
    /// UUID-E.
    UuidE([u8; 16]),
    /// UUID-R.
    UuidR([u8; 16]),
    /// MAC Address.
    MacAddress(MacAddr),
    /// Enrollee Nonce.
    EnrolleeNonce([u8; 16]),
    /// Registrar Nonce.
    RegistrarNonce([u8; 16]),
    /// Public Key.
    PublicKey(&'a [u8]),
    /// Authentication Type Flags.
    AuthenticationTypeFlags(u16),
    /// Encryption Type Flags (big-endian, note 0009 A9).
    EncryptionTypeFlags(u16),
    /// Connection Type Flags, 1 octet.
    ConnectionTypeFlags(u8),
    /// Configuration Methods (big-endian, note 0009 A9).
    ConfigurationMethods(u16),
    /// Wi-Fi Simple Configuration State, 1 octet.
    WscState(u8),
    /// Manufacturer.
    Manufacturer(&'a [u8]),
    /// Model Name.
    ModelName(&'a [u8]),
    /// Model Number.
    ModelNumber(&'a [u8]),
    /// Serial Number.
    SerialNumber(&'a [u8]),
    /// Primary Device Type, raw.
    PrimaryDeviceType([u8; 8]),
    /// Device Name.
    DeviceName(&'a [u8]),
    /// RF Bands, 1 octet of band flags.
    RfBands(u8),
    /// Association State.
    AssociationState(u16),
    /// Device Password ID.
    DevicePasswordId(u16),
    /// Configuration Error (big-endian, note 0009 A9).
    ConfigurationError(u16),
    /// OS Version, raw.
    OsVersion([u8; 4]),
    /// Encrypted Settings: IV and ciphertext, raw.
    EncryptedSettings(&'a [u8]),
    /// Authenticator.
    Authenticator([u8; 8]),
    /// Vendor Extension. See [`MultiApExtension`] and [`HaulTypeExtension`].
    VendorExtension(WscVendorExtension<'a>),
    /// SSID, with the trailing NUL if the sender wrote one.
    Ssid(&'a [u8]),
    /// Authentication Type.
    AuthenticationType(u16),
    /// Encryption Type.
    EncryptionType(u16),
    /// Network Key, with the trailing NUL if the sender wrote one.
    NetworkKey(&'a [u8]),
    /// Key Wrap Authenticator.
    KeyWrapAuthenticator([u8; 8]),
    /// An attribute whose id has no variant above.
    Other(WscAttr<'a>),
}

/// Reads a value of exactly `N` octets.
fn exact<const N: usize>(value: &[u8]) -> Result<[u8; N], DecodeError> {
    <[u8; N]>::try_from(value).map_err(|_| DecodeError::InvalidLength { len: value.len() })
}

fn exact_u8(value: &[u8]) -> Result<u8, DecodeError> {
    let [octet] = exact::<1>(value)?;
    Ok(octet)
}

fn exact_be_u16(value: &[u8]) -> Result<u16, DecodeError> {
    Ok(u16::from_be_bytes(exact(value)?))
}

impl<'a> WscAttribute<'a> {
    /// Decodes one attribute.
    ///
    /// # Errors
    ///
    /// [`DecodeError::InvalidLength`] if a fixed-width attribute does not have its width,
    /// or a Vendor Extension has fewer than 3 octets. This is stricter than upstream, which
    /// ignores or overruns these lengths (note 0009 D5 and A9).
    pub fn decode(attr: WscAttr<'a>) -> Result<Self, DecodeError> {
        let value = attr.value;
        Ok(match WscAttrId(attr.id) {
            WscAttrId::VERSION => Self::Version(exact_u8(value)?),
            WscAttrId::MESSAGE_TYPE => Self::MessageType(exact_u8(value)?),
            WscAttrId::UUID_E => Self::UuidE(exact(value)?),
            WscAttrId::UUID_R => Self::UuidR(exact(value)?),
            WscAttrId::MAC_ADDRESS => Self::MacAddress(MacAddr::new(exact(value)?)),
            WscAttrId::ENROLLEE_NONCE => Self::EnrolleeNonce(exact(value)?),
            WscAttrId::REGISTRAR_NONCE => Self::RegistrarNonce(exact(value)?),
            WscAttrId::PUBLIC_KEY => Self::PublicKey(value),
            WscAttrId::AUTHENTICATION_TYPE_FLAGS => {
                Self::AuthenticationTypeFlags(exact_be_u16(value)?)
            }
            WscAttrId::ENCRYPTION_TYPE_FLAGS => Self::EncryptionTypeFlags(exact_be_u16(value)?),
            WscAttrId::CONNECTION_TYPE_FLAGS => Self::ConnectionTypeFlags(exact_u8(value)?),
            WscAttrId::CONFIGURATION_METHODS => Self::ConfigurationMethods(exact_be_u16(value)?),
            WscAttrId::WSC_STATE => Self::WscState(exact_u8(value)?),
            WscAttrId::MANUFACTURER => Self::Manufacturer(value),
            WscAttrId::MODEL_NAME => Self::ModelName(value),
            WscAttrId::MODEL_NUMBER => Self::ModelNumber(value),
            WscAttrId::SERIAL_NUMBER => Self::SerialNumber(value),
            WscAttrId::PRIMARY_DEVICE_TYPE => Self::PrimaryDeviceType(exact(value)?),
            WscAttrId::DEVICE_NAME => Self::DeviceName(value),
            WscAttrId::RF_BANDS => Self::RfBands(exact_u8(value)?),
            WscAttrId::ASSOCIATION_STATE => Self::AssociationState(exact_be_u16(value)?),
            WscAttrId::DEVICE_PASSWORD_ID => Self::DevicePasswordId(exact_be_u16(value)?),
            WscAttrId::CONFIGURATION_ERROR => Self::ConfigurationError(exact_be_u16(value)?),
            WscAttrId::OS_VERSION => Self::OsVersion(exact(value)?),
            WscAttrId::ENCRYPTED_SETTINGS => Self::EncryptedSettings(value),
            WscAttrId::AUTHENTICATOR => Self::Authenticator(exact(value)?),
            WscAttrId::VENDOR_EXTENSION => {
                let mut reader = Reader::new(value);
                let vendor_id = reader
                    .array::<3>()
                    .map_err(|_| DecodeError::InvalidLength { len: value.len() })?;
                Self::VendorExtension(WscVendorExtension {
                    vendor_id,
                    data: reader.rest(),
                })
            }
            WscAttrId::SSID => Self::Ssid(value),
            WscAttrId::AUTHENTICATION_TYPE => Self::AuthenticationType(exact_be_u16(value)?),
            WscAttrId::ENCRYPTION_TYPE => Self::EncryptionType(exact_be_u16(value)?),
            WscAttrId::NETWORK_KEY => Self::NetworkKey(value),
            WscAttrId::KEY_WRAP_AUTHENTICATOR => Self::KeyWrapAuthenticator(exact(value)?),
            _ => Self::Other(attr),
        })
    }

    /// The attribute id of this value.
    #[must_use]
    pub fn id(&self) -> WscAttrId {
        match self {
            Self::Version(_) => WscAttrId::VERSION,
            Self::MessageType(_) => WscAttrId::MESSAGE_TYPE,
            Self::UuidE(_) => WscAttrId::UUID_E,
            Self::UuidR(_) => WscAttrId::UUID_R,
            Self::MacAddress(_) => WscAttrId::MAC_ADDRESS,
            Self::EnrolleeNonce(_) => WscAttrId::ENROLLEE_NONCE,
            Self::RegistrarNonce(_) => WscAttrId::REGISTRAR_NONCE,
            Self::PublicKey(_) => WscAttrId::PUBLIC_KEY,
            Self::AuthenticationTypeFlags(_) => WscAttrId::AUTHENTICATION_TYPE_FLAGS,
            Self::EncryptionTypeFlags(_) => WscAttrId::ENCRYPTION_TYPE_FLAGS,
            Self::ConnectionTypeFlags(_) => WscAttrId::CONNECTION_TYPE_FLAGS,
            Self::ConfigurationMethods(_) => WscAttrId::CONFIGURATION_METHODS,
            Self::WscState(_) => WscAttrId::WSC_STATE,
            Self::Manufacturer(_) => WscAttrId::MANUFACTURER,
            Self::ModelName(_) => WscAttrId::MODEL_NAME,
            Self::ModelNumber(_) => WscAttrId::MODEL_NUMBER,
            Self::SerialNumber(_) => WscAttrId::SERIAL_NUMBER,
            Self::PrimaryDeviceType(_) => WscAttrId::PRIMARY_DEVICE_TYPE,
            Self::DeviceName(_) => WscAttrId::DEVICE_NAME,
            Self::RfBands(_) => WscAttrId::RF_BANDS,
            Self::AssociationState(_) => WscAttrId::ASSOCIATION_STATE,
            Self::DevicePasswordId(_) => WscAttrId::DEVICE_PASSWORD_ID,
            Self::ConfigurationError(_) => WscAttrId::CONFIGURATION_ERROR,
            Self::OsVersion(_) => WscAttrId::OS_VERSION,
            Self::EncryptedSettings(_) => WscAttrId::ENCRYPTED_SETTINGS,
            Self::Authenticator(_) => WscAttrId::AUTHENTICATOR,
            Self::VendorExtension(_) => WscAttrId::VENDOR_EXTENSION,
            Self::Ssid(_) => WscAttrId::SSID,
            Self::AuthenticationType(_) => WscAttrId::AUTHENTICATION_TYPE,
            Self::EncryptionType(_) => WscAttrId::ENCRYPTION_TYPE,
            Self::NetworkKey(_) => WscAttrId::NETWORK_KEY,
            Self::KeyWrapAuthenticator(_) => WscAttrId::KEY_WRAP_AUTHENTICATOR,
            Self::Other(attr) => WscAttrId(attr.id),
        }
    }

    /// Writes the attribute (id, length, value) with [`write_wsc_attr`].
    ///
    /// # Errors
    ///
    /// [`EncodeError::CountTooLarge`] if a variable-length value is longer than 65535 octets;
    /// nothing is written.
    pub fn encode(&self, writer: &mut Writer) -> Result<(), EncodeError> {
        let id = self.id().0;
        match *self {
            Self::Version(octet)
            | Self::MessageType(octet)
            | Self::ConnectionTypeFlags(octet)
            | Self::WscState(octet)
            | Self::RfBands(octet) => write_wsc_attr(writer, id, &[octet]),
            Self::AuthenticationTypeFlags(word)
            | Self::EncryptionTypeFlags(word)
            | Self::ConfigurationMethods(word)
            | Self::AssociationState(word)
            | Self::DevicePasswordId(word)
            | Self::ConfigurationError(word)
            | Self::AuthenticationType(word)
            | Self::EncryptionType(word) => write_wsc_attr(writer, id, &word.to_be_bytes()),
            Self::UuidE(bytes)
            | Self::UuidR(bytes)
            | Self::EnrolleeNonce(bytes)
            | Self::RegistrarNonce(bytes) => write_wsc_attr(writer, id, &bytes),
            Self::MacAddress(mac) => write_wsc_attr(writer, id, &mac.octets()),
            Self::PrimaryDeviceType(bytes)
            | Self::Authenticator(bytes)
            | Self::KeyWrapAuthenticator(bytes) => write_wsc_attr(writer, id, &bytes),
            Self::OsVersion(bytes) => write_wsc_attr(writer, id, &bytes),
            Self::PublicKey(bytes)
            | Self::Manufacturer(bytes)
            | Self::ModelName(bytes)
            | Self::ModelNumber(bytes)
            | Self::SerialNumber(bytes)
            | Self::DeviceName(bytes)
            | Self::EncryptedSettings(bytes)
            | Self::Ssid(bytes)
            | Self::NetworkKey(bytes) => write_wsc_attr(writer, id, bytes),
            Self::VendorExtension(extension) => {
                // Checked before writing, so an oversized value is neither copied nor written.
                let total = extension.vendor_id.len() + extension.data.len();
                let Ok(len) = u16::try_from(total) else {
                    return Err(EncodeError::CountTooLarge {
                        len: total,
                        max: usize::from(u16::MAX),
                    });
                };
                writer.be_u16(id);
                writer.be_u16(len);
                writer.bytes(&extension.vendor_id);
                writer.bytes(extension.data);
                Ok(())
            }
            Self::Other(attr) => write_wsc_attr(writer, attr.id, attr.value),
        }
    }
}

/// The Wi-Fi Alliance Multi-AP Extension sub-element of a Vendor Extension attribute
/// (Table 19 of the Wi-Fi Alliance Multi-AP specification).
///
/// Upstream writes it in the Encrypted Settings (`em_configuration.cpp:5903-5915`) but never
/// reads it. Reserved bits 1 and 0 are ignored on decode and zero on encode.
// Six independent flag bits of one octet; a bool each is the clearest form.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MultiApExtension {
    /// Bit 7: backhaul STA.
    pub backhaul_sta: bool,
    /// Bit 6: backhaul BSS.
    pub backhaul_bss: bool,
    /// Bit 5: fronthaul BSS.
    pub fronthaul_bss: bool,
    /// Bit 4: tear down.
    pub tear_down: bool,
    /// Bit 3: Profile-1 backhaul STA association disallowed.
    pub profile1_bsta_disallowed: bool,
    /// Bit 2: Profile-2 backhaul STA association disallowed.
    pub profile2_bsta_disallowed: bool,
}

impl MultiApExtension {
    const BACKHAUL_STA: u8 = 0x80;
    const BACKHAUL_BSS: u8 = 0x40;
    const FRONTHAUL_BSS: u8 = 0x20;
    const TEAR_DOWN: u8 = 0x10;
    const PROFILE1_BSTA_DISALLOWED: u8 = 0x08;
    const PROFILE2_BSTA_DISALLOWED: u8 = 0x04;

    /// Finds the Multi-AP sub-element in a Vendor Extension.
    ///
    /// Returns `Ok(None)` if the vendor id is not the Wi-Fi Alliance's or no sub-element has
    /// id 0x06. The data is walked as sub-elements of 1 octet id, 1 octet length and value.
    ///
    /// # Errors
    ///
    /// [`DecodeError::Truncated`] if a sub-element runs past the data;
    /// [`DecodeError::InvalidLength`] if the Multi-AP sub-element does not have length 1.
    pub fn from_vendor_extension(
        extension: &WscVendorExtension<'_>,
    ) -> Result<Option<Self>, DecodeError> {
        if extension.vendor_id != WFA_VENDOR_ID {
            return Ok(None);
        }
        let mut reader = Reader::new(extension.data);
        while reader.remaining() > 0 {
            let sub_id = reader.u8()?;
            let len = usize::from(reader.u8()?);
            let value = reader.bytes(len)?;
            if sub_id == MULTI_AP_SUBELEMENT_ID {
                let flags = exact_u8(value)?;
                return Ok(Some(Self::from_flags(flags)));
            }
        }
        Ok(None)
    }

    fn from_flags(flags: u8) -> Self {
        Self {
            backhaul_sta: flags & Self::BACKHAUL_STA != 0,
            backhaul_bss: flags & Self::BACKHAUL_BSS != 0,
            fronthaul_bss: flags & Self::FRONTHAUL_BSS != 0,
            tear_down: flags & Self::TEAR_DOWN != 0,
            profile1_bsta_disallowed: flags & Self::PROFILE1_BSTA_DISALLOWED != 0,
            profile2_bsta_disallowed: flags & Self::PROFILE2_BSTA_DISALLOWED != 0,
        }
    }

    fn flags(self) -> u8 {
        let bits = [
            (self.backhaul_sta, Self::BACKHAUL_STA),
            (self.backhaul_bss, Self::BACKHAUL_BSS),
            (self.fronthaul_bss, Self::FRONTHAUL_BSS),
            (self.tear_down, Self::TEAR_DOWN),
            (
                self.profile1_bsta_disallowed,
                Self::PROFILE1_BSTA_DISALLOWED,
            ),
            (
                self.profile2_bsta_disallowed,
                Self::PROFILE2_BSTA_DISALLOWED,
            ),
        ];
        let mut flags = 0;
        for (is_set, mask) in bits {
            if is_set {
                flags |= mask;
            }
        }
        flags
    }

    /// Writes a Vendor Extension attribute holding this sub-element (6 octets of value).
    pub fn write(&self, writer: &mut Writer) {
        writer.be_u16(WscAttrId::VENDOR_EXTENSION.0);
        writer.be_u16(6);
        writer.bytes(&WFA_VENDOR_ID);
        writer.u8(MULTI_AP_SUBELEMENT_ID);
        writer.u8(MULTI_AP_SUBELEMENT_LEN);
        writer.u8(self.flags());
    }
}

/// The Comcast haul-type sub-element of a Vendor Extension attribute
/// (`vendor_ext_attr_id_haul_type`, `em_base.h:2334`).
///
/// Note 0009 A9: the encoder writes upstream's 11 octets (vendor id, sub-id 0x01, haul type,
/// 6 zero octets, `em_configuration.cpp:5917-5924`). The decoder reads sub-id 0x01 from 5 or
/// more octets and ignores the tail; any other sub-id is `Ok(None)` at any length. Upstream's
/// reader (`em_configuration.cpp:4911-4919`) checks the OUI and that the value is longer than 3
/// octets, reads octet 3 as the sub-id, and reads octet 4 only for sub-id 0x01. So with sub-id
/// 0x01 and 4 octets it reads past the value. The port refuses a sub-id 0x01 value shorter than
/// 5 octets, and a Comcast value with no sub-id (intentional differences; upstream ignores a
/// value of 3 octets).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HaulTypeExtension {
    /// Haul type octet (`em_haul_type_t`).
    pub haul_type: u8,
}

impl HaulTypeExtension {
    /// Reads the haul type from a Vendor Extension.
    ///
    /// Returns `Ok(None)` if the vendor id is not Comcast's or the sub-id is not 0x01.
    ///
    /// # Errors
    ///
    /// [`DecodeError::InvalidLength`] (with the length of the whole attribute value) if a
    /// Comcast Vendor Extension has no sub-id, or has sub-id 0x01 and no haul type octet.
    pub fn from_vendor_extension(
        extension: &WscVendorExtension<'_>,
    ) -> Result<Option<Self>, DecodeError> {
        if extension.vendor_id != COMCAST_VENDOR_ID {
            return Ok(None);
        }
        let too_short = DecodeError::InvalidLength {
            len: extension.vendor_id.len() + extension.data.len(),
        };
        let Some((&sub_id, rest)) = extension.data.split_first() else {
            return Err(too_short);
        };
        // Upstream reads the haul type only for this sub-id (`em_configuration.cpp:4916-4917`).
        if sub_id != HAUL_TYPE_SUB_ID {
            return Ok(None);
        }
        let Some(&haul_type) = rest.first() else {
            return Err(too_short);
        };
        Ok(Some(Self { haul_type }))
    }

    /// Writes a Vendor Extension attribute holding the haul type, 11 octets of value.
    pub fn write(&self, writer: &mut Writer) {
        writer.be_u16(WscAttrId::VENDOR_EXTENSION.0);
        // Upstream's length: 3 (vendor id) + 4 + 4 (`sizeof` of the sub-id and haul type enums).
        writer.be_u16(11);
        writer.bytes(&COMCAST_VENDOR_ID);
        writer.u8(HAUL_TYPE_SUB_ID);
        writer.u8(self.haul_type);
        // Only 5 octets are meaningful; upstream's zeroed buffer fills the other 6.
        writer.zeros(6);
    }
}

/// Looks for the Message Type attribute in the value of one WSC TLV.
fn attribute_msg_type(value: &[u8]) -> Option<u8> {
    let mut rest = value;
    while rest.len() >= ATTR_HEADER_LEN {
        let id = u16::from_be_bytes([rest[0], rest[1]]);
        let len = usize::from(u16::from_be_bytes([rest[2], rest[3]]));
        let attribute = rest.get(ATTR_HEADER_LEN..ATTR_HEADER_LEN + len)?;
        if id == ATTR_ID_MSG_TYPE {
            if let Some(first) = attribute.first() {
                return Some(*first);
            }
        }
        rest = &rest[ATTR_HEADER_LEN + len..];
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One attribute: id, length, value.
    fn attr(id: u16, value: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&id.to_be_bytes());
        out.extend_from_slice(&u16::try_from(value.len()).unwrap().to_be_bytes());
        out.extend_from_slice(value);
        out
    }

    /// One TLV: type, length, value.
    fn tlv(tlv_type: TlvType, value: &[u8]) -> Vec<u8> {
        let mut out = vec![tlv_type.0];
        out.extend_from_slice(&u16::try_from(value.len()).unwrap().to_be_bytes());
        out.extend_from_slice(value);
        out
    }

    fn end() -> Vec<u8> {
        vec![0, 0, 0]
    }

    fn area(parts: &[Vec<u8>]) -> Vec<u8> {
        parts.concat()
    }

    #[test]
    fn constants() {
        assert_eq!(WSC_M1, 4);
        assert_eq!(WSC_M2, 5);
    }

    #[test]
    fn m1_and_m2_are_found() {
        for wanted in [WSC_M1, WSC_M2] {
            let attrs = [attr(0x104A, &[0x10]), attr(0x1022, &[wanted])].concat();
            let bytes = area(&[tlv(TlvType::WSC, &attrs), end()]);
            assert_eq!(wsc_msg_type(&bytes), Some(wanted));
        }
    }

    #[test]
    fn missing_attribute_gives_none() {
        let bytes = area(&[tlv(TlvType::WSC, &attr(0x104A, &[0x10])), end()]);
        assert_eq!(wsc_msg_type(&bytes), None);
    }

    #[test]
    fn no_wsc_tlv_gives_none() {
        let bytes = area(&[tlv(TlvType(0x01), &attr(0x1022, &[4])), end()]);
        assert_eq!(wsc_msg_type(&bytes), None);
    }

    #[test]
    fn zero_length_attribute_is_skipped() {
        let attrs = [attr(0x1022, &[]), attr(0x1022, &[WSC_M2])].concat();
        let bytes = area(&[tlv(TlvType::WSC, &attrs), end()]);
        assert_eq!(wsc_msg_type(&bytes), Some(WSC_M2));
    }

    #[test]
    fn only_a_zero_length_attribute_gives_none() {
        let bytes = area(&[tlv(TlvType::WSC, &attr(0x1022, &[])), end()]);
        assert_eq!(wsc_msg_type(&bytes), None);
    }

    #[test]
    fn overrun_inside_one_wsc_tlv_does_not_stop_the_next_wsc_tlv() {
        // The attribute claims 9 value bytes but the TLV holds only 1.
        let broken = [0x10, 0x22, 0x00, 0x09, 0x04];
        let good = attr(0x1022, &[WSC_M1]);
        let bytes = area(&[tlv(TlvType::WSC, &broken), tlv(TlvType::WSC, &good), end()]);
        assert_eq!(wsc_msg_type(&bytes), Some(WSC_M1));
    }

    #[test]
    fn overrun_attribute_value_is_not_returned_but_the_next_tlv_is() {
        // The malformed attribute holds M2 but claims 9 bytes; the valid one holds M1.
        let broken = [0x10, 0x22, 0x00, 0x09, WSC_M2];
        let good = attr(0x1022, &[WSC_M1]);
        let bytes = area(&[tlv(TlvType::WSC, &broken), tlv(TlvType::WSC, &good), end()]);
        assert_eq!(wsc_msg_type(&bytes), Some(WSC_M1));
    }

    #[test]
    fn first_message_type_attribute_in_one_wsc_tlv_wins() {
        let attrs = [attr(0x1022, &[0x09]), attr(0x1022, &[WSC_M1])].concat();
        let bytes = area(&[tlv(TlvType::WSC, &attrs), end()]);
        assert_eq!(wsc_msg_type(&bytes), Some(0x09));
    }

    #[test]
    fn first_wsc_tlv_with_a_message_type_wins() {
        // An unrecognised value in the first TLV still beats M1 in a later one.
        let first = tlv(TlvType::WSC, &attr(0x1022, &[0x09]));
        let second = tlv(TlvType::WSC, &attr(0x1022, &[WSC_M1]));
        let bytes = area(&[first, second, end()]);
        assert_eq!(wsc_msg_type(&bytes), Some(0x09));
    }

    #[test]
    fn tlv_overrun_gives_none() {
        // The second TLV claims 9 bytes but only 1 remains; its type is WSC and would match.
        let mut bytes = tlv(TlvType(0x01), &[]);
        bytes.extend_from_slice(&[TlvType::WSC.0, 0x00, 0x09, 0x10]);
        assert_eq!(wsc_msg_type(&bytes), None);
    }

    #[test]
    fn match_before_a_later_tlv_overrun_is_still_returned() {
        let mut bytes = tlv(TlvType::WSC, &attr(0x1022, &[WSC_M1]));
        bytes.extend_from_slice(&[0x01, 0x00, 0x09]);
        assert_eq!(wsc_msg_type(&bytes), Some(WSC_M1));
    }

    #[test]
    fn non_wsc_tlvs_are_ignored() {
        let other = tlv(TlvType(0x01), &attr(0x1022, &[WSC_M1]));
        let wsc = tlv(TlvType::WSC, &attr(0x1022, &[WSC_M2]));
        let bytes = area(&[other, wsc, end()]);
        assert_eq!(wsc_msg_type(&bytes), Some(WSC_M2));
    }

    #[test]
    fn walk_stops_at_end_of_message() {
        let wsc = tlv(TlvType::WSC, &attr(0x1022, &[WSC_M2]));
        let bytes = area(&[end(), wsc]);
        assert_eq!(wsc_msg_type(&bytes), None);
    }

    #[test]
    fn other_values_are_returned_as_is() {
        for value in [0u8, 1, 6, 0xFF] {
            let bytes = area(&[tlv(TlvType::WSC, &attr(0x1022, &[value])), end()]);
            assert_eq!(wsc_msg_type(&bytes), Some(value));
        }
    }

    #[test]
    fn only_the_first_value_byte_is_used() {
        let bytes = area(&[tlv(TlvType::WSC, &attr(0x1022, &[WSC_M2, WSC_M1])), end()]);
        assert_eq!(wsc_msg_type(&bytes), Some(WSC_M2));
    }

    // The attribute walker and writer.

    fn walk(value: &[u8]) -> Vec<Result<WscAttr<'_>, DecodeError>> {
        WscAttrIter::new(value).take(MAX_ITEMS).collect()
    }

    /// Bound for collecting an iterator in tests, so a runaway iterator fails the test instead of exhausting memory.
    const MAX_ITEMS: usize = 64;

    #[test]
    fn empty_input_has_no_attributes() {
        assert_eq!(walk(&[]).len(), 0);
    }

    #[test]
    fn attributes_round_trip_through_the_writer() {
        let mut writer = Writer::new();
        write_wsc_attr(&mut writer, 0x1022, &[0x04]).unwrap();
        write_wsc_attr(&mut writer, 0x104A, &[0x10]).unwrap();
        write_wsc_attr(&mut writer, 0x1021, b"vendor").unwrap();
        let bytes = writer.into_bytes();
        assert_eq!(bytes, attr_bytes_for_round_trip());
        assert_eq!(
            walk(&bytes),
            [
                Ok(WscAttr {
                    id: 0x1022,
                    value: &[0x04]
                }),
                Ok(WscAttr {
                    id: 0x104A,
                    value: &[0x10]
                }),
                Ok(WscAttr {
                    id: 0x1021,
                    value: b"vendor"
                }),
            ]
        );
    }

    fn attr_bytes_for_round_trip() -> Vec<u8> {
        let mut bytes = attr(0x1022, &[0x04]);
        bytes.extend(attr(0x104A, &[0x10]));
        bytes.extend(attr(0x1021, b"vendor"));
        bytes
    }

    #[test]
    fn zero_length_attribute_is_yielded() {
        let mut bytes = attr(0x1022, &[]);
        bytes.extend(attr(0x104A, &[0x10]));
        assert_eq!(
            walk(&bytes),
            [
                Ok(WscAttr {
                    id: 0x1022,
                    value: &[]
                }),
                Ok(WscAttr {
                    id: 0x104A,
                    value: &[0x10]
                }),
            ]
        );
    }

    #[test]
    fn partial_header_is_truncated() {
        let whole = attr(0x1022, &[0x04]);
        let expected = [
            DecodeError::Truncated {
                needed: 2,
                remaining: 1,
            },
            DecodeError::Truncated {
                needed: 2,
                remaining: 0,
            },
            DecodeError::Truncated {
                needed: 2,
                remaining: 1,
            },
        ];
        for (index, error) in expected.iter().enumerate() {
            let header_len = index + 1;
            assert_eq!(
                walk(&whole[..header_len]),
                [Err(*error)],
                "header of {header_len} bytes"
            );
        }
    }

    #[test]
    fn partial_header_after_a_good_attribute_is_truncated() {
        let mut bytes = attr(0x1022, &[0x04]);
        bytes.push(0x10);
        assert_eq!(
            walk(&bytes),
            [
                Ok(WscAttr {
                    id: 0x1022,
                    value: &[0x04]
                }),
                Err(DecodeError::Truncated {
                    needed: 2,
                    remaining: 1
                }),
            ]
        );
    }

    #[test]
    fn length_past_the_end_is_truncated() {
        let mut bytes = attr(0x1022, &[0x04, 0x05, 0x06]);
        bytes.pop();
        assert_eq!(
            walk(&bytes),
            [Err(DecodeError::Truncated {
                needed: 3,
                remaining: 2
            })]
        );
        // A header that promises 65535 bytes of nothing.
        assert_eq!(
            walk(&[0x10, 0x22, 0xFF, 0xFF]),
            [Err(DecodeError::Truncated {
                needed: 65535,
                remaining: 0
            })]
        );
    }

    #[test]
    fn nothing_is_yielded_after_an_error() {
        let bytes = [0x10, 0x22, 0x00, 0x05, 0x01];
        let mut iter = WscAttrIter::new(&bytes);
        assert!(matches!(iter.next(), Some(Err(_))));
        for _ in 0..3 {
            assert_eq!(iter.next(), None);
        }
    }

    #[test]
    fn writer_accepts_65535_bytes_and_refuses_65536() {
        let value = vec![0x5A; 65535];
        let mut writer = Writer::new();
        write_wsc_attr(&mut writer, 0x1001, &value).unwrap();
        let bytes = writer.into_bytes();
        assert_eq!(bytes.len(), 4 + 65535);
        assert_eq!(bytes[..4], [0x10, 0x01, 0xFF, 0xFF]);
        assert_eq!(
            walk(&bytes),
            [Ok(WscAttr {
                id: 0x1001,
                value: &value
            })]
        );

        let too_long = vec![0u8; 65536];
        let mut writer = Writer::new();
        writer.u8(0xEE);
        assert_eq!(
            write_wsc_attr(&mut writer, 0x1001, &too_long),
            Err(EncodeError::CountTooLarge {
                len: 65536,
                max: 65535
            })
        );
        assert_eq!(writer.into_bytes(), [0xEE]);
    }

    // Typed attributes (B12).

    const SIXTEEN: [u8; 16] = [
        0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F,
        0x10,
    ];
    const EIGHT: [u8; 8] = [0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18];
    const MAC_BYTES: [u8; 6] = [0x00, 0x11, 0x22, 0x33, 0x44, 0x55];

    /// One attribute with its literal id, a valid value, the typed form and the width it must have
    /// (`None` for variable-length values).
    struct Case {
        id: u16,
        value: &'static [u8],
        attr: WscAttribute<'static>,
        width: Option<usize>,
    }

    fn case(
        id: u16,
        value: &'static [u8],
        attr: WscAttribute<'static>,
        width: Option<usize>,
    ) -> Case {
        Case {
            id,
            value,
            attr,
            width,
        }
    }

    /// The 32 attributes of the audit, ids written out as literals.
    fn all_cases() -> Vec<Case> {
        let mut cases = first_cases();
        cases.extend(last_cases());
        cases
    }

    fn first_cases() -> Vec<Case> {
        vec![
            case(0x104A, &[0x11], WscAttribute::Version(0x11), Some(1)),
            case(0x1022, &[0x04], WscAttribute::MessageType(4), Some(1)),
            case(0x1047, &SIXTEEN, WscAttribute::UuidE(SIXTEEN), Some(16)),
            case(0x1048, &SIXTEEN, WscAttribute::UuidR(SIXTEEN), Some(16)),
            case(
                0x1020,
                &MAC_BYTES,
                WscAttribute::MacAddress(MacAddr::new(MAC_BYTES)),
                Some(6),
            ),
            case(
                0x101A,
                &SIXTEEN,
                WscAttribute::EnrolleeNonce(SIXTEEN),
                Some(16),
            ),
            case(
                0x1039,
                &SIXTEEN,
                WscAttribute::RegistrarNonce(SIXTEEN),
                Some(16),
            ),
            case(
                0x1032,
                &[0xC1, 0xC2],
                WscAttribute::PublicKey(&[0xC1, 0xC2]),
                None,
            ),
            case(
                0x1004,
                &[0x01, 0x02],
                WscAttribute::AuthenticationTypeFlags(0x0102),
                Some(2),
            ),
            case(
                0x1010,
                &[0x01, 0x02],
                WscAttribute::EncryptionTypeFlags(0x0102),
                Some(2),
            ),
            case(
                0x100D,
                &[0x01],
                WscAttribute::ConnectionTypeFlags(1),
                Some(1),
            ),
            case(
                0x1008,
                &[0x06, 0x80],
                WscAttribute::ConfigurationMethods(0x0680),
                Some(2),
            ),
            case(0x1044, &[0x00], WscAttribute::WscState(0), Some(1)),
            case(0x1021, b"ABC", WscAttribute::Manufacturer(b"ABC"), None),
            case(0x1023, b"M1", WscAttribute::ModelName(b"M1"), None),
            case(0x1024, b"M2", WscAttribute::ModelNumber(b"M2"), None),
            case(0x1042, b"1234", WscAttribute::SerialNumber(b"1234"), None),
            case(
                0x1054,
                &EIGHT,
                WscAttribute::PrimaryDeviceType(EIGHT),
                Some(8),
            ),
            case(0x1011, b"dev", WscAttribute::DeviceName(b"dev"), None),
            case(0x103C, &[0x08], WscAttribute::RfBands(0x08), Some(1)),
            case(
                0x1002,
                &[0x12, 0x34],
                WscAttribute::AssociationState(0x1234),
                Some(2),
            ),
            case(
                0x1012,
                &[0x00, 0x04],
                WscAttribute::DevicePasswordId(4),
                Some(2),
            ),
            case(
                0x1009,
                &[0x00, 0x15],
                WscAttribute::ConfigurationError(21),
                Some(2),
            ),
            case(
                0x102D,
                &[0x01, 0x02, 0x03, 0x04],
                WscAttribute::OsVersion([1, 2, 3, 4]),
                Some(4),
            ),
        ]
    }

    fn last_cases() -> Vec<Case> {
        vec![
            case(
                0x1018,
                &[0xE0, 0xE1, 0xE2],
                WscAttribute::EncryptedSettings(&[0xE0, 0xE1, 0xE2]),
                None,
            ),
            case(0x1005, &EIGHT, WscAttribute::Authenticator(EIGHT), Some(8)),
            case(
                0x1049,
                &[0x00, 0x37, 0x2A, 0x06, 0x01, 0x40],
                WscAttribute::VendorExtension(WscVendorExtension {
                    vendor_id: WFA_VENDOR_ID,
                    data: &[0x06, 0x01, 0x40],
                }),
                None,
            ),
            case(0x1045, b"mesh\0", WscAttribute::Ssid(b"mesh\0"), None),
            case(
                0x1003,
                &[0x00, 0x20],
                WscAttribute::AuthenticationType(0x0020),
                Some(2),
            ),
            case(
                0x100F,
                &[0x00, 0x08],
                WscAttribute::EncryptionType(0x0008),
                Some(2),
            ),
            case(0x1027, b"pw\0", WscAttribute::NetworkKey(b"pw\0"), None),
            case(
                0x101E,
                &EIGHT,
                WscAttribute::KeyWrapAuthenticator(EIGHT),
                Some(8),
            ),
        ]
    }

    fn encoded(attr: &WscAttribute<'_>) -> Vec<u8> {
        let mut writer = Writer::new();
        attr.encode(&mut writer).unwrap();
        writer.into_bytes()
    }

    #[test]
    fn id_constants_are_the_32_audited_ids() {
        let cases = all_cases();
        assert_eq!(cases.len(), 32);
        assert_eq!(WscAttrId::ALL.len(), 32);
        for one in &cases {
            let id = WscAttrId(one.id);
            assert!(id.name().is_some(), "{:#06X} has no constant", one.id);
            assert_eq!(one.attr.id(), id);
        }
        let mut ids: Vec<u16> = WscAttrId::ALL.iter().map(|id| id.0).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 32);
        assert_eq!(format!("{:?}", WscAttrId::SSID), "SSID(0x1045)");
        assert_eq!(format!("{:?}", WscAttrId(0x1FFF)), "WscAttrId(0x1FFF)");
        assert_eq!(WscAttrId::from(0x1022), WscAttrId::MESSAGE_TYPE);
        assert_eq!(WscAttrId::MESSAGE_TYPE.0, ATTR_ID_MSG_TYPE);
    }

    #[test]
    fn every_attribute_decodes() {
        for one in all_cases() {
            let attr = WscAttr {
                id: one.id,
                value: one.value,
            };
            assert_eq!(WscAttribute::decode(attr), Ok(one.attr), "{:#06X}", one.id);
        }
    }

    #[test]
    fn every_attribute_encodes() {
        for one in all_cases() {
            let expected = attr(one.id, one.value);
            assert_eq!(encoded(&one.attr), expected, "{:#06X}", one.id);
        }
    }

    #[test]
    fn fixed_width_attribute_with_the_wrong_length_is_invalid() {
        for one in all_cases() {
            let Some(width) = one.width else {
                continue;
            };
            let longer = [one.value, &[0x00]].concat();
            let shorter = &one.value[..width - 1];
            for wrong in [&[][..], shorter, &longer] {
                let attr = WscAttr {
                    id: one.id,
                    value: wrong,
                };
                assert_eq!(
                    WscAttribute::decode(attr),
                    Err(DecodeError::InvalidLength { len: wrong.len() }),
                    "{:#06X} with {} octets",
                    one.id,
                    wrong.len()
                );
            }
        }
    }

    #[test]
    fn variable_length_attribute_takes_any_length() {
        for one in all_cases() {
            if one.width.is_some() || one.id == 0x1049 {
                continue;
            }
            for value in [&[][..], &[0x00], &[0xAA; 300]] {
                let decoded = WscAttribute::decode(WscAttr { id: one.id, value }).unwrap();
                assert_eq!(encoded(&decoded), attr(one.id, value), "{:#06X}", one.id);
            }
        }
    }

    #[test]
    fn variable_length_attribute_of_65536_octets_is_refused() {
        let too_long = vec![0u8; 65536];
        for one in all_cases() {
            if one.width.is_some() {
                continue;
            }
            let long = WscAttr {
                id: one.id,
                value: &too_long,
            };
            let mut writer = Writer::new();
            let attribute = if one.id == 0x1049 {
                // Vendor id plus 65533 octets of data.
                WscAttribute::VendorExtension(WscVendorExtension {
                    vendor_id: WFA_VENDOR_ID,
                    data: &too_long[..65534],
                })
            } else {
                WscAttribute::decode(long).unwrap()
            };
            assert_eq!(
                attribute.encode(&mut writer),
                Err(EncodeError::CountTooLarge {
                    len: if one.id == 0x1049 { 65537 } else { 65536 },
                    max: 65535
                }),
                "{:#06X}",
                one.id
            );
            assert!(writer.is_empty());
        }
    }

    #[test]
    fn vendor_extension_needs_a_vendor_id() {
        for len in 0..3 {
            let attr = WscAttr {
                id: 0x1049,
                value: &[0x00, 0x37, 0x2A][..len],
            };
            assert_eq!(
                WscAttribute::decode(attr),
                Err(DecodeError::InvalidLength { len })
            );
        }
        let attr = WscAttr {
            id: 0x1049,
            value: &[0x00, 0x37, 0x2A],
        };
        assert_eq!(
            WscAttribute::decode(attr),
            Ok(WscAttribute::VendorExtension(WscVendorExtension {
                vendor_id: WFA_VENDOR_ID,
                data: &[],
            }))
        );
    }

    #[test]
    fn unknown_id_is_kept_and_written_back() {
        let unknown = WscAttr {
            id: 0x1FFF,
            value: &[1, 2, 3],
        };
        let decoded = WscAttribute::decode(unknown).unwrap();
        assert_eq!(decoded, WscAttribute::Other(unknown));
        assert_eq!(decoded.id(), WscAttrId(0x1FFF));
        assert_eq!(encoded(&decoded), attr(0x1FFF, &[1, 2, 3]));
    }

    #[test]
    fn typed_attribute_from_a_truncated_walk_is_an_error_not_a_panic() {
        // The walker reports the cut-short attribute; nothing typed is built from it.
        let bytes = [0x10, 0x4A, 0x00, 0x01];
        assert_eq!(
            walk(&bytes),
            [Err(DecodeError::Truncated {
                needed: 1,
                remaining: 0
            })]
        );
    }

    // intentional-difference tests: upstream behaviour the port does not copy.

    #[test]
    fn ssid_keeps_the_attribute_length() {
        // intentional-difference: em_configuration.cpp:4921-4926 reads SSID by copying a fixed
        // `sizeof(ssid)` and ignores the attribute length; the port yields exactly `len` octets.
        let short = WscAttr {
            id: 0x1045,
            value: b"ab",
        };
        assert_eq!(WscAttribute::decode(short), Ok(WscAttribute::Ssid(b"ab")));
    }

    #[test]
    fn authentication_type_must_be_two_octets() {
        // intentional-difference: em_configuration.cpp:4927-4931 memcpy's `len` octets into an
        // unsigned short: it overflows when len > 2 and leaves the value partly unset when len < 2;
        // the port refuses any length but 2.
        for len in [0usize, 1, 3, 4, 40] {
            let value = vec![0u8; len];
            let attr = WscAttr {
                id: 0x1003,
                value: &value,
            };
            assert_eq!(
                WscAttribute::decode(attr),
                Err(DecodeError::InvalidLength { len })
            );
        }
    }

    #[test]
    fn key_wrap_authenticator_is_read_whole() {
        // intentional-difference: em_configuration.cpp:4941-4943 reads only val[0] of the Key
        // Wrap Authenticator; the port reads all 8 octets and refuses any other length.
        let attr = WscAttr {
            id: 0x101E,
            value: &[0x5A],
        };
        assert_eq!(
            WscAttribute::decode(attr),
            Err(DecodeError::InvalidLength { len: 1 })
        );
        let whole = WscAttr {
            id: 0x101E,
            value: &EIGHT,
        };
        assert_eq!(
            WscAttribute::decode(whole),
            Ok(WscAttribute::KeyWrapAuthenticator(EIGHT))
        );
    }

    #[test]
    fn long_strings_and_keys_are_borrowed_not_copied_into_a_fixed_buffer() {
        // intentional-difference: em_configuration.cpp:4721, :4726, :4732 (manufacturer, model
        // name, serial number) and :4934-4937 (network key) memcpy `len` octets into fixed
        // buffers without a bound check; the port borrows the value at its exact length.
        let long = [0x41u8; 1000];
        for id in [0x1021, 0x1023, 0x1042, 0x1027] {
            let decoded = WscAttribute::decode(WscAttr { id, value: &long }).unwrap();
            let (WscAttribute::Manufacturer(value)
            | WscAttribute::ModelName(value)
            | WscAttribute::SerialNumber(value)
            | WscAttribute::NetworkKey(value)) = decoded
            else {
                panic!("unexpected variant {decoded:?}");
            };
            assert_eq!(value.len(), 1000);
        }
    }

    #[test]
    fn association_state_password_id_and_configuration_error_write_their_value() {
        // intentional-difference: em_configuration.cpp:3538-3562 (M1) and :3109-3133 (M2) set the
        // length to 2 but copy `sizeof(attr->val)` = 0 value octets, so the 2 octets are
        // uninitialised; the port writes the given u16, big-endian (note 0009 A9).
        assert_eq!(
            encoded(&WscAttribute::AssociationState(0x1234)),
            [0x10, 0x02, 0x00, 0x02, 0x12, 0x34]
        );
        assert_eq!(
            encoded(&WscAttribute::DevicePasswordId(0x0004)),
            [0x10, 0x12, 0x00, 0x02, 0x00, 0x04]
        );
        assert_eq!(
            encoded(&WscAttribute::ConfigurationError(0x0015)),
            [0x10, 0x09, 0x00, 0x02, 0x00, 0x15]
        );
        assert_eq!(
            encoded(&WscAttribute::AssociationState(0)),
            [0x10, 0x02, 0x00, 0x02, 0x00, 0x00]
        );
    }

    #[test]
    fn encryption_type_flags_and_configuration_methods_are_big_endian() {
        // intentional-difference: em_configuration.cpp:3425 and :3446 memcpy host-order fields
        // (on a little-endian host 0x0102 goes out as 02 01); the port writes big-endian
        // (note 0009 A9).
        assert_eq!(
            encoded(&WscAttribute::EncryptionTypeFlags(0x0102)),
            [0x10, 0x10, 0x00, 0x02, 0x01, 0x02]
        );
        assert_eq!(
            encoded(&WscAttribute::ConfigurationMethods(0x0102)),
            [0x10, 0x08, 0x00, 0x02, 0x01, 0x02]
        );
    }

    #[test]
    fn rf_bands_is_one_octet() {
        // intentional-difference: em_configuration.cpp:3532 memcpy's 1 octet of a 4-octet enum
        // (the low octet only on a little-endian host); the port takes the octet itself.
        assert_eq!(
            encoded(&WscAttribute::RfBands(0x08)),
            [0x10, 0x3C, 0x00, 0x01, 0x08]
        );
        assert_eq!(
            WscAttribute::decode(WscAttr {
                id: 0x103C,
                value: &[0x01, 0x00, 0x00, 0x00]
            }),
            Err(DecodeError::InvalidLength { len: 4 })
        );
    }

    // Multi-AP Extension and haul-type Vendor Extension.

    fn vendor(vendor_id: [u8; 3], data: &[u8]) -> WscVendorExtension<'_> {
        WscVendorExtension { vendor_id, data }
    }

    #[test]
    fn multi_ap_flags_decode_bit_by_bit() {
        let flags_for = |bit: u8| {
            MultiApExtension::from_vendor_extension(&vendor(WFA_VENDOR_ID, &[0x06, 0x01, bit]))
        };
        let none = MultiApExtension::default();
        assert_eq!(flags_for(0x00), Ok(Some(none)));
        assert_eq!(
            flags_for(0x80),
            Ok(Some(MultiApExtension {
                backhaul_sta: true,
                ..none
            }))
        );
        assert_eq!(
            flags_for(0x40),
            Ok(Some(MultiApExtension {
                backhaul_bss: true,
                ..none
            }))
        );
        assert_eq!(
            flags_for(0x20),
            Ok(Some(MultiApExtension {
                fronthaul_bss: true,
                ..none
            }))
        );
        assert_eq!(
            flags_for(0x10),
            Ok(Some(MultiApExtension {
                tear_down: true,
                ..none
            }))
        );
        assert_eq!(
            flags_for(0x08),
            Ok(Some(MultiApExtension {
                profile1_bsta_disallowed: true,
                ..none
            }))
        );
        assert_eq!(
            flags_for(0x04),
            Ok(Some(MultiApExtension {
                profile2_bsta_disallowed: true,
                ..none
            }))
        );
    }

    #[test]
    fn multi_ap_reserved_bits_are_ignored_on_decode_and_zero_on_encode() {
        let reserved_only = vendor(WFA_VENDOR_ID, &[0x06, 0x01, 0x03]);
        assert_eq!(
            MultiApExtension::from_vendor_extension(&reserved_only),
            Ok(Some(MultiApExtension::default()))
        );
        let all = MultiApExtension {
            backhaul_sta: true,
            backhaul_bss: true,
            fronthaul_bss: true,
            tear_down: true,
            profile1_bsta_disallowed: true,
            profile2_bsta_disallowed: true,
        };
        let everything = vendor(WFA_VENDOR_ID, &[0x06, 0x01, 0xFF]);
        assert_eq!(
            MultiApExtension::from_vendor_extension(&everything),
            Ok(Some(all))
        );
        let mut writer = Writer::new();
        all.write(&mut writer);
        assert_eq!(
            writer.into_bytes(),
            [0x10, 0x49, 0x00, 0x06, 0x00, 0x37, 0x2A, 0x06, 0x01, 0xFC]
        );
    }

    #[test]
    fn multi_ap_encodes_upstream_backhaul_and_fronthaul_values() {
        // em_configuration.cpp:170-173, :5903-5915: 0x40 for backhaul, 0x20 for fronthaul.
        let backhaul = MultiApExtension {
            backhaul_bss: true,
            ..MultiApExtension::default()
        };
        let fronthaul = MultiApExtension {
            fronthaul_bss: true,
            ..MultiApExtension::default()
        };
        let mut writer = Writer::new();
        backhaul.write(&mut writer);
        fronthaul.write(&mut writer);
        assert_eq!(
            writer.into_bytes(),
            [
                0x10, 0x49, 0x00, 0x06, 0x00, 0x37, 0x2A, 0x06, 0x01, 0x40, //
                0x10, 0x49, 0x00, 0x06, 0x00, 0x37, 0x2A, 0x06, 0x01, 0x20,
            ]
        );
    }

    #[test]
    fn multi_ap_other_vendor_or_other_subelement_is_not_it() {
        let other_vendor = vendor(COMCAST_VENDOR_ID, &[0x06, 0x01, 0x40]);
        assert_eq!(
            MultiApExtension::from_vendor_extension(&other_vendor),
            Ok(None)
        );
        let other_sub = vendor(WFA_VENDOR_ID, &[0x07, 0x01, 0x40]);
        assert_eq!(
            MultiApExtension::from_vendor_extension(&other_sub),
            Ok(None)
        );
        assert_eq!(
            MultiApExtension::from_vendor_extension(&vendor(WFA_VENDOR_ID, &[])),
            Ok(None)
        );
    }

    #[test]
    fn multi_ap_is_found_after_another_subelement() {
        let data = [0x01, 0x02, 0xAA, 0xBB, 0x06, 0x01, 0x80];
        assert_eq!(
            MultiApExtension::from_vendor_extension(&vendor(WFA_VENDOR_ID, &data)),
            Ok(Some(MultiApExtension {
                backhaul_sta: true,
                ..MultiApExtension::default()
            }))
        );
    }

    #[test]
    fn multi_ap_with_a_bad_length_or_truncated_is_an_error() {
        assert_eq!(
            MultiApExtension::from_vendor_extension(&vendor(
                WFA_VENDOR_ID,
                &[0x06, 0x02, 0x40, 0x00]
            )),
            Err(DecodeError::InvalidLength { len: 2 })
        );
        assert_eq!(
            MultiApExtension::from_vendor_extension(&vendor(WFA_VENDOR_ID, &[0x06, 0x00])),
            Err(DecodeError::InvalidLength { len: 0 })
        );
        assert_eq!(
            MultiApExtension::from_vendor_extension(&vendor(WFA_VENDOR_ID, &[0x06, 0x01])),
            Err(DecodeError::Truncated {
                needed: 1,
                remaining: 0
            })
        );
        assert_eq!(
            MultiApExtension::from_vendor_extension(&vendor(WFA_VENDOR_ID, &[0x06])),
            Err(DecodeError::Truncated {
                needed: 1,
                remaining: 0
            })
        );
    }

    #[test]
    fn haul_type_decodes_five_or_more_octets() {
        // em_configuration.cpp:4911-4919 reads val[3] and val[4] once the value is longer than 3
        // octets, and ignores the rest.
        let five = vendor(COMCAST_VENDOR_ID, &[0x01, 0x02]);
        assert_eq!(
            HaulTypeExtension::from_vendor_extension(&five),
            Ok(Some(HaulTypeExtension { haul_type: 2 }))
        );
        let eleven = vendor(COMCAST_VENDOR_ID, &[0x01, 0x03, 0, 0, 0, 0, 0, 0]);
        assert_eq!(
            HaulTypeExtension::from_vendor_extension(&eleven),
            Ok(Some(HaulTypeExtension { haul_type: 3 }))
        );
        let junk_tail = vendor(COMCAST_VENDOR_ID, &[0x01, 0x04, 9, 8, 7]);
        assert_eq!(
            HaulTypeExtension::from_vendor_extension(&junk_tail),
            Ok(Some(HaulTypeExtension { haul_type: 4 }))
        );
    }

    #[test]
    fn haul_type_with_fewer_than_five_octets_is_an_error() {
        // intentional-difference: em_configuration.cpp:4914-4919 ignores a 3-octet value, and
        // with sub-id 0x01 and 4 octets reads val[4] past the value; the port refuses both (the
        // first has no sub-id, the second has sub-id 0x01 and no haul type octet).
        for data in [&[][..], &[0x01]] {
            assert_eq!(
                HaulTypeExtension::from_vendor_extension(&vendor(COMCAST_VENDOR_ID, data)),
                Err(DecodeError::InvalidLength {
                    len: 3 + data.len()
                })
            );
        }
    }

    #[test]
    fn haul_type_other_sub_id_with_four_octets_is_ignored_as_upstream() {
        // em_configuration.cpp:4914-4919 reads val[4] only for sub-id 0x01, so this value is
        // read entirely inside its length and skipped.
        assert_eq!(
            HaulTypeExtension::from_vendor_extension(&vendor(COMCAST_VENDOR_ID, &[0x02])),
            Ok(None)
        );
    }

    #[test]
    fn haul_type_other_vendor_or_sub_id_is_not_it() {
        let other_vendor = vendor(WFA_VENDOR_ID, &[0x01, 0x02]);
        assert_eq!(
            HaulTypeExtension::from_vendor_extension(&other_vendor),
            Ok(None)
        );
        let other_sub = vendor(COMCAST_VENDOR_ID, &[0x02, 0x02]);
        assert_eq!(
            HaulTypeExtension::from_vendor_extension(&other_sub),
            Ok(None)
        );
    }

    #[test]
    fn haul_type_is_written_with_upstreams_eleven_octets() {
        // note 0009 A9; em_configuration.cpp:5917-5924: length 11, 5 octets set, 6 zeros.
        let mut writer = Writer::new();
        HaulTypeExtension { haul_type: 2 }.write(&mut writer);
        assert_eq!(
            writer.into_bytes(),
            [
                0x10, 0x49, 0x00, 0x0B, 0xD8, 0x9C, 0x8E, 0x01, 0x02, 0, 0, 0, 0, 0, 0
            ]
        );
    }

    // Golden attribute lists, bytes derived by hand from the audit.

    /// Joins hex text (whitespace allowed) into bytes.
    fn hex(parts: &[&str]) -> Vec<u8> {
        let digits: Vec<u8> = parts
            .iter()
            .flat_map(|part| part.bytes())
            .filter(|byte| !byte.is_ascii_whitespace())
            .collect();
        assert_eq!(digits.len() % 2, 0);
        digits
            .chunks(2)
            .map(|pair| {
                let text = core::str::from_utf8(pair).unwrap();
                u8::from_str_radix(text, 16).unwrap()
            })
            .collect()
    }

    /// Walks `bytes`, decodes every attribute, and checks the typed list; then writes the typed
    /// list back and checks the bytes.
    fn assert_golden(bytes: &[u8], expected: &[WscAttribute<'_>]) {
        let mut decoded = Vec::new();
        for item in WscAttrIter::new(bytes) {
            decoded.push(WscAttribute::decode(item.unwrap()).unwrap());
        }
        assert_eq!(decoded, expected);
        let mut writer = Writer::new();
        for attribute in expected {
            attribute.encode(&mut writer).unwrap();
        }
        assert_eq!(writer.into_bytes(), bytes);
    }

    const NONCE_E: [u8; 16] = [
        0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5, 0xA6, 0xA7, 0xA8, 0xA9, 0xAA, 0xAB, 0xAC, 0xAD, 0xAE,
        0xAF,
    ];
    const NONCE_R: [u8; 16] = [
        0xB0, 0xB1, 0xB2, 0xB3, 0xB4, 0xB5, 0xB6, 0xB7, 0xB8, 0xB9, 0xBA, 0xBB, 0xBC, 0xBD, 0xBE,
        0xBF,
    ];
    const UUID_R_BYTES: [u8; 16] = [
        0xF0, 0xF1, 0xF2, 0xF3, 0xF4, 0xF5, 0xF6, 0xF7, 0xF8, 0xF9, 0xFA, 0xFB, 0xFC, 0xFD, 0xFE,
        0xFF,
    ];
    const DEVICE_TYPE: [u8; 8] = [0x00, 0x06, 0x00, 0x50, 0xF2, 0x04, 0x00, 0x01];

    #[test]
    fn m1_attribute_list_in_upstreams_order() {
        // create_m1_msg, em_configuration.cpp:3305-3580.
        let bytes = hex(&[
            "104a 0001 11",
            "1022 0001 04",
            "1047 0010 0102030405060708090a0b0c0d0e0f10",
            "1020 0006 001122334455",
            "101a 0010 a0a1a2a3a4a5a6a7a8a9aaabacadaeaf",
            "1032 0008 c1c2c3c4c5c6c7c8",
            "1004 0002 0022",
            "1010 0002 0008",
            "100d 0001 01",
            "1008 0002 0680",
            "1044 0001 00",
            "1021 0003 414243",
            "1023 0002 4d31",
            "1024 0002 4d31",
            "1042 0004 31323334",
            "1054 0008 00060050f2040001",
            "1011 0002 4d31",
            "103c 0001 02",
            "1002 0002 0000",
            "1012 0002 0004",
            "1009 0002 0000",
            "102d 0004 00000000",
        ]);
        let expected = [
            WscAttribute::Version(0x11),
            WscAttribute::MessageType(WSC_M1),
            WscAttribute::UuidE(SIXTEEN),
            WscAttribute::MacAddress(MacAddr::new(MAC_BYTES)),
            WscAttribute::EnrolleeNonce(NONCE_E),
            WscAttribute::PublicKey(&[0xC1, 0xC2, 0xC3, 0xC4, 0xC5, 0xC6, 0xC7, 0xC8]),
            WscAttribute::AuthenticationTypeFlags(0x0022),
            WscAttribute::EncryptionTypeFlags(0x0008),
            WscAttribute::ConnectionTypeFlags(1),
            WscAttribute::ConfigurationMethods(0x0680),
            WscAttribute::WscState(0),
            WscAttribute::Manufacturer(b"ABC"),
            WscAttribute::ModelName(b"M1"),
            WscAttribute::ModelNumber(b"M1"),
            WscAttribute::SerialNumber(b"1234"),
            WscAttribute::PrimaryDeviceType(DEVICE_TYPE),
            WscAttribute::DeviceName(b"M1"),
            WscAttribute::RfBands(0x02),
            WscAttribute::AssociationState(0),
            WscAttribute::DevicePasswordId(4),
            WscAttribute::ConfigurationError(0),
            WscAttribute::OsVersion([0; 4]),
        ];
        assert_golden(&bytes, &expected);
    }

    #[test]
    fn m2_attribute_list_in_upstreams_order() {
        // create_m2_msg, em_configuration.cpp:2896-3166: Configuration Error comes before Device
        // Password ID (the reverse of M1); no MAC Address and no WSC State.
        let bytes = hex(&[
            "104a 0001 10",
            "1022 0001 05",
            "101a 0010 a0a1a2a3a4a5a6a7a8a9aaabacadaeaf",
            "1039 0010 b0b1b2b3b4b5b6b7b8b9babbbcbdbebf",
            "1048 0010 f0f1f2f3f4f5f6f7f8f9fafbfcfdfeff",
            "1032 0008 d1d2d3d4d5d6d7d8",
            "1004 0002 0022",
            "1010 0002 0008",
            "100d 0001 01",
            "1008 0002 0680",
            "1021 0003 414243",
            "1023 0002 4d31",
            "1024 0002 4d31",
            "1042 0004 31323334",
            "1054 0008 00060050f2040001",
            "1011 0002 4d31",
            "103c 0001 08",
            "1002 0002 0000",
            "1009 0002 0015",
            "1012 0002 0004",
            "102d 0004 00000000",
            "1018 0014 e0e1e2e3e4e5e6e7e8e9eaebecedeeef01020304",
            "1005 0008 0102030405060708",
        ]);
        let expected = [
            WscAttribute::Version(0x10),
            WscAttribute::MessageType(WSC_M2),
            WscAttribute::EnrolleeNonce(NONCE_E),
            WscAttribute::RegistrarNonce(NONCE_R),
            WscAttribute::UuidR(UUID_R_BYTES),
            WscAttribute::PublicKey(&[0xD1, 0xD2, 0xD3, 0xD4, 0xD5, 0xD6, 0xD7, 0xD8]),
            WscAttribute::AuthenticationTypeFlags(0x0022),
            WscAttribute::EncryptionTypeFlags(0x0008),
            WscAttribute::ConnectionTypeFlags(1),
            WscAttribute::ConfigurationMethods(0x0680),
            WscAttribute::Manufacturer(b"ABC"),
            WscAttribute::ModelName(b"M1"),
            WscAttribute::ModelNumber(b"M1"),
            WscAttribute::SerialNumber(b"1234"),
            WscAttribute::PrimaryDeviceType(DEVICE_TYPE),
            WscAttribute::DeviceName(b"M1"),
            WscAttribute::RfBands(0x08),
            WscAttribute::AssociationState(0),
            WscAttribute::ConfigurationError(21),
            WscAttribute::DevicePasswordId(4),
            WscAttribute::OsVersion([0; 4]),
            WscAttribute::EncryptedSettings(&[
                0xE0, 0xE1, 0xE2, 0xE3, 0xE4, 0xE5, 0xE6, 0xE7, 0xE8, 0xE9, 0xEA, 0xEB, 0xEC, 0xED,
                0xEE, 0xEF, 0x01, 0x02, 0x03, 0x04,
            ]),
            WscAttribute::Authenticator([1, 2, 3, 4, 5, 6, 7, 8]),
        ];
        assert_golden(&bytes, &expected);
    }

    #[test]
    fn encrypted_settings_plaintext_parses_through_the_same_accessors() {
        // create_encrypted_settings, em_configuration.cpp:5842-6016: two Vendor Extensions, SSID
        // and Network Key with their NUL, then the Key Wrap Authenticator.
        let bytes = hex(&[
            "1049 0006 00372a 06 01 40",
            "1049 000b d89c8e 01 02 000000000000",
            "1045 0005 6d65736800",
            "1003 0002 0020",
            "100f 0002 0008",
            "1027 0009 70617373776f726400",
            "1020 0006 aabbccddeeff",
            "101e 0008 1112131415161718",
        ]);
        let wfa = vendor(WFA_VENDOR_ID, &[0x06, 0x01, 0x40]);
        let comcast = vendor(COMCAST_VENDOR_ID, &[0x01, 0x02, 0, 0, 0, 0, 0, 0]);
        let expected = [
            WscAttribute::VendorExtension(wfa),
            WscAttribute::VendorExtension(comcast),
            WscAttribute::Ssid(b"mesh\0"),
            WscAttribute::AuthenticationType(0x0020),
            WscAttribute::EncryptionType(0x0008),
            WscAttribute::NetworkKey(b"password\0"),
            WscAttribute::MacAddress(MacAddr::new([0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF])),
            WscAttribute::KeyWrapAuthenticator([0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18]),
        ];
        assert_golden(&bytes, &expected);

        // The typed vendor helpers read the two extensions and write the same bytes.
        assert_eq!(
            MultiApExtension::from_vendor_extension(&wfa),
            Ok(Some(MultiApExtension {
                backhaul_bss: true,
                ..MultiApExtension::default()
            }))
        );
        assert_eq!(
            HaulTypeExtension::from_vendor_extension(&comcast),
            Ok(Some(HaulTypeExtension { haul_type: 2 }))
        );
        let mut writer = Writer::new();
        MultiApExtension {
            backhaul_bss: true,
            ..MultiApExtension::default()
        }
        .write(&mut writer);
        HaulTypeExtension { haul_type: 2 }.write(&mut writer);
        assert_eq!(writer.into_bytes(), bytes[..4 + 6 + 4 + 11]);
    }

    #[test]
    fn typed_attribute_sizes_stay_small() {
        use core::mem::size_of;
        let word = size_of::<usize>();
        assert!(size_of::<WscAttribute<'_>>() <= 4 * word);
        assert!(size_of::<WscAttr<'_>>() <= 3 * word);
        assert!(size_of::<WscVendorExtension<'_>>() <= 3 * word);
        assert!(size_of::<WscAttrId>() <= 2);
        assert!(size_of::<MultiApExtension>() <= 6);
        assert!(size_of::<HaulTypeExtension>() <= 1);
    }
}
