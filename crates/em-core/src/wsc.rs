// SPDX-License-Identifier: Apache-2.0

//! Synchronous WSC M1/M2 engine for one radio (note 0013 D4 and D6).
//!
//! The caller owns transport, radio states and timeouts. Keep one [`Session`] per
//! outstanding M1; a failed M2 leaves it available for a subsequent response.

use std::fmt;

use em_cmd::{NodeKey, NodeKind};
use em_crypto::dh::{DH_VALUE_LEN, DhPrivate, DhPublic};
use em_crypto::hash::{self, HashAlgorithm};
use em_crypto::rand::{RandomSource, random_secret};
use em_crypto::{SecretArray, SecretBytes, cbc, ct, kdf};
use em_msg::bodies::{
    ApRadioAdvancedCapabilities, ApRadioBasicCapabilities, ApRadioIdentifier, MacValue,
    Profile2ApCapability,
};
use em_msg::wsc::{HaulTypeExtension, MultiApExtension, WSC_M1, WSC_M2, WscAttrIter, WscAttribute};
use em_msg::{AL_SAP_BUDGET, CmduHeader, MsgBuilder, RAW_BUDGET, TlvBody, TlvIter, Writer};
use em_types::{FreqBand, MacAddr, MsgType, ProfileType, TlvType};

use crate::{AgentState, CtrlState, MsgIdCounter};

/// WSC WPA2-Personal authentication flag (`inc/em_base.h:133`).
pub const AUTH_WPA2_PERSONAL: u16 = 0x0020;
/// WSC SAE AKM 8 authentication flag (`inc/em_base.h:134`).
pub const AUTH_SAE: u16 = 0x0040;
const AUTH_FLAGS: u16 = AUTH_WPA2_PERSONAL | AUTH_SAE;
const ENCRYPTION_AES: u16 = 0x0008;
const CONFIG_METHODS: u16 = 0x0080;
const MAX_HAULS: usize = 5;
const MAX_WSC_LEN: usize = 4096;
const MAX_SSID_LEN: usize = 32;
const MAX_KEY_LEN: usize = 64;
const KDF_LABEL: &str = "Wi-Fi Easy and Secure Key Derivation";

/// Failure to build or parse a WSC exchange.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// Malformed, incomplete, ambiguous or oversized wire data.
    InvalidMessage,
    /// The message addresses a different radio or controller.
    WrongPeer,
    /// There is no outstanding M1 session.
    NoSession,
    /// The radio is not awaiting an M2.
    WrongState,
    /// An M2 authenticator did not verify.
    Authenticator,
    /// Encrypted settings or their key-wrap authenticator failed.
    RejectedSettings,
    /// The radio cannot carry any BSS.
    NoBssCapacity,
    /// A random source, cryptographic operation, or encoder failed.
    Construction,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}

fn header(msg_type: MsgType, msg_id: u16) -> CmduHeader {
    CmduHeader {
        msg_type,
        msg_id,
        frag_id: 0,
        last_fragment: true,
        relay: false,
    }
}

fn area(cmdu: &[u8], kind: MsgType) -> Result<&[u8], Error> {
    if cmdu.len() > AL_SAP_BUDGET {
        return Err(Error::InvalidMessage);
    }
    let (head, rest) = CmduHeader::parse(cmdu).map_err(|_| Error::InvalidMessage)?;
    if head.msg_type != kind || head.frag_id != 0 || !head.last_fragment {
        return Err(Error::InvalidMessage);
    }
    Ok(rest)
}

/// Device identification used for M1 and M2 attributes.
#[derive(Debug, Clone, Copy)]
pub struct Identity<'a> {
    /// WSC UUID.
    pub uuid: [u8; 16],
    /// Manufacturer name.
    pub manufacturer: &'a [u8],
    /// Model name and number.
    pub model: &'a [u8],
    /// Serial number.
    pub serial: &'a [u8],
    /// WSC primary device type.
    pub primary_device_type: [u8; 8],
}

fn identity_ok(id: &Identity<'_>) -> bool {
    id.manufacturer.len() <= 64 && id.model.len() <= 64 && id.serial.len() <= 64
}

fn push_attributes(writer: &mut Writer, attrs: &[WscAttribute<'_>]) -> Result<(), Error> {
    for attr in attrs {
        attr.encode(writer).map_err(|_| Error::Construction)?;
    }
    Ok(())
}

fn common_identity(writer: &mut Writer, id: &Identity<'_>, band: FreqBand) -> Result<(), Error> {
    if !identity_ok(id) {
        return Err(Error::Construction);
    }
    let band = u8::from(band);
    let rf = 1_u8
        .checked_shl(u32::from(band))
        .ok_or(Error::Construction)?;
    push_attributes(
        writer,
        &[
            WscAttribute::Manufacturer(id.manufacturer),
            WscAttribute::ModelName(id.model),
            WscAttribute::ModelNumber(id.model),
            WscAttribute::SerialNumber(id.serial),
            WscAttribute::PrimaryDeviceType(id.primary_device_type),
            WscAttribute::DeviceName(id.model),
            WscAttribute::RfBands(rf),
            WscAttribute::AssociationState(0),
        ],
    )
}

#[derive(Debug)]
struct Keys {
    auth: SecretArray<32>,
    wrap: SecretArray<16>,
    #[allow(dead_code, reason = "retained as part of the WSC key material")]
    emsk: SecretArray<32>,
}

fn derive_keys(
    private: &DhPrivate,
    peer: &[u8],
    n1: &[u8; 16],
    mac: MacAddr,
    n2: &[u8; 16],
) -> Result<Keys, Error> {
    let secret = private
        .shared_secret(peer)
        .map_err(|_| Error::InvalidMessage)?;
    let dhkey = hash::digest(HashAlgorithm::Sha256, &[secret.expose()]);
    let kdk = hash::hmac(
        HashAlgorithm::Sha256,
        dhkey.expose(),
        &[n1, &mac.octets(), n2],
    )
    .map_err(|_| Error::Construction)?;
    let out = kdf::wps_kdf(kdk.expose(), &[], KDF_LABEL, 80).map_err(|_| Error::Construction)?;
    Ok(Keys {
        auth: SecretArray::from_slice(&out.expose()[..32]).map_err(|_| Error::Construction)?,
        wrap: SecretArray::from_slice(&out.expose()[32..48]).map_err(|_| Error::Construction)?,
        emsk: SecretArray::from_slice(&out.expose()[48..]).map_err(|_| Error::Construction)?,
    })
}

#[derive(Debug)]
struct Exchange {
    private: DhPrivate,
    n1: SecretArray<16>,
    enrollee_mac: MacAddr,
    radio: MacAddr,
    controller: MacAddr,
    max_bss: u8,
    m1: SecretBytes,
    registrar_nonce: Option<SecretArray<16>>,
    keys: Option<Keys>,
}

/// The outstanding M1 exchange for one radio. Secret fields are wiped on drop.
///
/// Call [`clear`](Self::clear) for timeout, renew, removal or process shutdown.
/// Successful M2 processing clears it; failed processing leaves it intact.
#[derive(Debug, Default)]
pub struct Session {
    exchange: Option<Exchange>,
}

impl Session {
    /// Creates an empty session.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
    /// Drops and wipes the current exchange, if any.
    pub fn clear(&mut self) {
        self.exchange = None;
    }
    /// Whether a live M1 exchange is available.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.exchange.is_some()
    }
}

/// Agent inputs for an M1. The basic and advanced RUIDs must agree.
pub struct M1Params<'a> {
    /// Agent AL MAC, used as the WSC enrollee MAC.
    pub al_mac: MacAddr,
    /// Expected controller AL MAC.
    pub controller: MacAddr,
    /// Radio band.
    pub band: FreqBand,
    /// Agent identification.
    pub identity: Identity<'a>,
    /// Radio basic capabilities and maximum BSS count.
    pub basic: ApRadioBasicCapabilities<'a>,
    /// Profile-2 capability values.
    pub profile2: Profile2ApCapability,
    /// Radio advanced capability values.
    pub advanced: ApRadioAdvancedCapabilities,
}

/// Builds an M1 and replaces the per-radio session after construction succeeds.
///
/// # Errors
/// Returns an error if inputs, randomness, encoding or message budget fail.
pub fn build_m1(
    params: &M1Params<'_>,
    ids: &mut MsgIdCounter,
    random: &mut impl RandomSource,
    session: &mut Session,
) -> Result<Vec<u8>, Error> {
    if params.basic.ruid != params.advanced.ruid
        || params.basic.max_bss == 0
        || !identity_ok(&params.identity)
    {
        return Err(Error::Construction);
    }
    let private = DhPrivate::generate(random).map_err(|_| Error::Construction)?;
    let public = private.public_key();
    let n1 = random_secret::<16>(random).map_err(|_| Error::Construction)?;
    let mut writer = Writer::new();
    push_attributes(
        &mut writer,
        &[
            WscAttribute::Version(0x11),
            WscAttribute::MessageType(WSC_M1),
            WscAttribute::UuidE(params.identity.uuid),
            WscAttribute::MacAddress(params.al_mac),
            WscAttribute::EnrolleeNonce(*n1.expose()),
            WscAttribute::PublicKey(public.as_bytes()),
            WscAttribute::AuthenticationTypeFlags(AUTH_FLAGS),
            WscAttribute::EncryptionTypeFlags(ENCRYPTION_AES),
            WscAttribute::ConnectionTypeFlags(0),
            WscAttribute::ConfigurationMethods(CONFIG_METHODS),
            WscAttribute::WscState(0),
        ],
    )?;
    common_identity(&mut writer, &params.identity, params.band)?;
    push_attributes(
        &mut writer,
        &[
            WscAttribute::DevicePasswordId(0),
            WscAttribute::ConfigurationError(0),
            WscAttribute::OsVersion([0; 4]),
        ],
    )?;
    let m1 = SecretBytes::new(writer.into_bytes());
    let mut builder = MsgBuilder::new(
        &header(MsgType::AP_AUTOCONFIGURATION_WSC, ids.next_id()),
        RAW_BUDGET,
    );
    builder
        .body(&params.basic)
        .tlv(TlvType::WSC, m1.expose())
        .body(&params.profile2)
        .body(&params.advanced);
    let frame = builder.finish().map_err(|_| Error::Construction)?;
    session.exchange = Some(Exchange {
        private,
        n1,
        enrollee_mac: params.al_mac,
        radio: params.basic.ruid,
        controller: params.controller,
        max_bss: params.basic.max_bss,
        m1,
        registrar_nonce: None,
        keys: None,
    });
    Ok(frame)
}

/// Controller-side parsed M1 and routing data, borrowing the received CMDU.
#[derive(Debug)]
pub struct M1<'a> {
    /// RUID from AP Radio Basic Capabilities.
    radio: MacAddr,
    /// Maximum BSS count advertised by the radio.
    max_bss: u8,
    /// Band from the WSC RF Bands attribute.
    band: FreqBand,
    /// Enrollee MAC from the WSC MAC Address attribute.
    enrollee_mac: MacAddr,
    /// Advertised authentication flags, including zero from upstream M1s.
    authentication_flags: u16,
    /// Mapped AKM names, bounded to ten.
    akms: Vec<&'static str>,
    n1: [u8; 16],
    public: DhPublic,
    raw: &'a [u8],
}

impl M1<'_> {
    /// Radio unique identifier used for routing.
    #[must_use]
    pub const fn radio(&self) -> MacAddr {
        self.radio
    }
    /// Advertised maximum BSS count.
    #[must_use]
    pub const fn max_bss(&self) -> u8 {
        self.max_bss
    }
    /// Frequency band advertised by the radio.
    #[must_use]
    pub const fn band(&self) -> FreqBand {
        self.band
    }
    /// Enrollee MAC used in key derivation.
    #[must_use]
    pub const fn enrollee_mac(&self) -> MacAddr {
        self.enrollee_mac
    }
    /// Authentication flags from the received M1.
    #[must_use]
    pub const fn authentication_flags(&self) -> u16 {
        self.authentication_flags
    }
    /// AKM names derived from those flags, with at most ten entries.
    #[must_use]
    pub fn akms(&self) -> &[&'static str] {
        &self.akms
    }
}

fn unique<T>(slot: &mut Option<T>, value: T) -> Result<(), Error> {
    if slot.is_some() {
        return Err(Error::InvalidMessage);
    }
    *slot = Some(value);
    Ok(())
}

fn rf_band(bits: u8) -> Result<FreqBand, Error> {
    match bits {
        1 => Ok(FreqBand::BAND_2_4_GHZ),
        2 => Ok(FreqBand::BAND_5_GHZ),
        4 => Ok(FreqBand::BAND_60_GHZ),
        8 => Ok(FreqBand::BAND_6_GHZ),
        _ => Err(Error::InvalidMessage),
    }
}

fn clamp_akms<'a>(input: impl IntoIterator<Item = &'a str>) -> Vec<&'a str> {
    let mut output = Vec::new();
    let mut overflow = false;
    for akm in input {
        if !output.contains(&akm) {
            if output.len() < 10 {
                output.push(akm);
            } else {
                overflow = true;
            }
        }
    }
    if overflow {
        tracing::warn!("akm-clamp-10: M1 AKM list exceeds ten");
    }
    output
}

// Mirrors the upstream set of AKM names (em_configuration.cpp:3186-3198,3273-3302).
fn akms_for_flags(flags: u16) -> Vec<&'static str> {
    let mut names = Vec::new();
    if flags & 0x0002 != 0 {
        names.push("wpa-psk");
    }
    if flags & 0x0008 != 0 {
        names.push("wpa-eap");
    }
    if flags & 0x0010 != 0 {
        names.push("wpa2-eap");
    }
    if flags & 0x0020 != 0 {
        names.push("wpa2-psk");
    }
    if flags & (0x0040 | 0x0100) != 0 {
        names.push("sae");
    }
    if flags & 0x0080 != 0 {
        names.push("dpp");
    }
    names.sort_unstable();
    clamp_akms(names)
}

/// Parses and routes an M1 without assuming its E-MAC equals the RUID.
///
/// # Errors
/// Returns an error for malformed framing, attributes or required radio data.
pub fn parse_m1(cmdu: &[u8]) -> Result<M1<'_>, Error> {
    let rest = area(cmdu, MsgType::AP_AUTOCONFIGURATION_WSC)?;
    em_msg::validate(cmdu, ProfileType::from(3)).map_err(|_| Error::InvalidMessage)?;
    let (mut basic, mut raw) = (None, None);
    for item in TlvIter::new(rest) {
        let tlv = item.map_err(|_| Error::InvalidMessage)?;
        if tlv.tlv_type == TlvType::AP_RADIO_BASIC_CAPABILITIES {
            unique(
                &mut basic,
                ApRadioBasicCapabilities::decode_value(tlv.value)
                    .map_err(|_| Error::InvalidMessage)?,
            )?;
        } else if tlv.tlv_type == TlvType::WSC {
            if tlv.value.len() > MAX_WSC_LEN {
                return Err(Error::InvalidMessage);
            }
            unique(&mut raw, tlv.value)?;
        }
    }
    let basic = basic.ok_or(Error::InvalidMessage)?;
    let raw = raw.ok_or(Error::InvalidMessage)?;
    let (mut kind, mut mac, mut n1, mut public, mut flags, mut band) =
        (None, None, None, None, None, None);
    for item in WscAttrIter::new(raw) {
        match WscAttribute::decode(item.map_err(|_| Error::InvalidMessage)?)
            .map_err(|_| Error::InvalidMessage)?
        {
            WscAttribute::MessageType(v) => unique(&mut kind, v)?,
            WscAttribute::MacAddress(v) => unique(&mut mac, v)?,
            WscAttribute::EnrolleeNonce(v) => unique(&mut n1, v)?,
            WscAttribute::PublicKey(v) => {
                if v.len() != DH_VALUE_LEN {
                    return Err(Error::InvalidMessage);
                }
                unique(
                    &mut public,
                    DhPublic::from_be_bytes(v).map_err(|_| Error::InvalidMessage)?,
                )?;
            }
            WscAttribute::AuthenticationTypeFlags(v) => unique(&mut flags, v)?,
            WscAttribute::RfBands(v) => unique(&mut band, rf_band(v)?)?,
            WscAttribute::Manufacturer(v)
            | WscAttribute::ModelName(v)
            | WscAttribute::ModelNumber(v)
            | WscAttribute::SerialNumber(v)
            | WscAttribute::DeviceName(v)
                if v.len() > 64 =>
            {
                return Err(Error::InvalidMessage);
            }
            _ => {}
        }
    }
    if kind != Some(WSC_M1) {
        return Err(Error::InvalidMessage);
    }
    let authentication_flags = flags.ok_or(Error::InvalidMessage)?;
    Ok(M1 {
        radio: basic.ruid,
        max_bss: basic.max_bss,
        band: band.ok_or(Error::InvalidMessage)?,
        enrollee_mac: mac.ok_or(Error::InvalidMessage)?,
        authentication_flags,
        akms: akms_for_flags(authentication_flags),
        n1: n1.ok_or(Error::InvalidMessage)?,
        public: public.ok_or(Error::InvalidMessage)?,
        raw,
    })
}

/// A borrowed view of one controller `NetworkSSIDList` row.
///
/// The role core can obtain these fields from `em_dm::NetworkSsidInfo`.
pub struct NetworkSsid<'a> {
    /// Whether the row is enabled.
    pub enabled: bool,
    /// SSID bytes, without a terminator.
    pub ssid: &'a [u8],
    /// Secret key bytes, without a terminator.
    pub network_key: &'a [u8],
    /// Supported radio bands.
    pub bands: &'a [FreqBand],
    /// Haul indices (0 fronthaul, 1 backhaul, 2–4 other fronthaul).
    pub haul_types: &'a [u8],
    /// WSC authentication bits.
    pub authentication_type: u16,
}

impl fmt::Debug for NetworkSsid<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NetworkSsid")
            .field("enabled", &self.enabled)
            .field("ssid", &self.ssid)
            .field("network_key", &"<redacted>")
            .field("bands", &self.bands)
            .field("haul_types", &self.haul_types)
            .field("authentication_type", &self.authentication_type)
            .finish()
    }
}

/// Maps upstream reset-file `AuthType` strings to WSC bits.
/// An unrecognized value defaults to SAE (`em_configuration.cpp:6018-6025`).
#[must_use]
pub fn authentication_type(mode: &[u8]) -> u16 {
    match mode {
        b"Open" => 0x0001,
        b"WPA2 Personal" => AUTH_WPA2_PERSONAL,
        b"WPA3 Transition" => AUTH_FLAGS,
        b"Enhanced Open" => 0x1000,
        _ => AUTH_SAE,
    }
}

/// Maps a reset-file band string to the corresponding band.
#[must_use]
pub fn network_band(value: &[u8]) -> Option<FreqBand> {
    if value.starts_with(b"2.4") {
        Some(FreqBand::BAND_2_4_GHZ)
    } else if value.starts_with(b"5") {
        Some(FreqBand::BAND_5_GHZ)
    } else if value.starts_with(b"6") {
        Some(FreqBand::BAND_6_GHZ)
    } else {
        None
    }
}

/// Controller M2 build result, including the diagnostic required by D6.
#[derive(Debug)]
pub struct M2Output {
    /// Radio for the post-handoff effects.
    pub radio: MacAddr,
    /// Complete CMDU without an Ethernet header.
    pub cmdu: Vec<u8>,
    /// No emitted fronthaul BSS carries WPA2 PSK.
    pub no_fronthaul_psk: bool,
}

/// An ordered controller effect to apply only after M2 handoff succeeds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControllerEffect {
    /// Remove the previous configuration chain for this radio.
    RemoveChainForNode(NodeKey),
    /// Advance this radio after handoff.
    SetState(NodeKey, CtrlState),
    /// Submit its configuration chain with its owning agent.
    SubmitEmConfig {
        /// Radio being configured.
        radio: NodeKey,
        /// The agent's AL address.
        agent_al: MacAddr,
    },
}

impl M2Output {
    /// Returns the three D3 effects, in upstream order; apply only on successful handoff.
    #[must_use]
    pub fn post_handoff_effects(&self, agent_al: MacAddr) -> [ControllerEffect; 3] {
        let radio = NodeKey {
            mac: self.radio,
            kind: NodeKind::Radio,
        };
        [
            ControllerEffect::RemoveChainForNode(radio),
            ControllerEffect::SetState(radio, CtrlState::WscM2Sent),
            ControllerEffect::SubmitEmConfig { radio, agent_al },
        ]
    }
}

fn put_secret_attr(out: &mut [u8], offset: &mut usize, id: u16, value: &[u8]) {
    let len = u16::try_from(value.len()).expect("bounded before encoding");
    out[*offset..*offset + 2].copy_from_slice(&id.to_be_bytes());
    out[*offset + 2..*offset + 4].copy_from_slice(&len.to_be_bytes());
    out[*offset + 4..*offset + 4 + value.len()].copy_from_slice(value);
    *offset += 4 + value.len();
}

fn encrypt_settings(
    row: Option<(&NetworkSsid<'_>, u16)>,
    haul: u8,
    radio: MacAddr,
    keys: &Keys,
    random: &mut impl RandomSource,
) -> Result<Vec<u8>, Error> {
    let mut vendor = Writer::new();
    MultiApExtension {
        backhaul_bss: row.is_some() && haul == 1,
        fronthaul_bss: row.is_some() && haul != 1,
        tear_down: row.is_none(),
        ..MultiApExtension::default()
    }
    .write(&mut vendor);
    HaulTypeExtension { haul_type: haul }.write(&mut vendor);
    let vendor = vendor.into_bytes();
    let (ssid, key, auth) = row.map_or((&[][..], &[][..], 0), |(r, auth)| {
        (r.ssid, r.network_key, auth)
    });
    if ssid.len() > MAX_SSID_LEN || key.len() > MAX_KEY_LEN || ssid.contains(&0) || key.contains(&0)
    {
        return Err(Error::Construction);
    }
    // The upstream SSID includes a NUL. The key intentionally excludes it.
    let length = vendor.len() + 4 + ssid.len() + 1 + 6 + 6 + 4 + key.len() + 10 + 12;
    let mut plain = SecretBytes::zeroed(length);
    let buf = plain.expose_mut();
    buf[..vendor.len()].copy_from_slice(&vendor);
    let mut offset = vendor.len();
    let mut ssid_nul = [0_u8; MAX_SSID_LEN + 1];
    ssid_nul[..ssid.len()].copy_from_slice(ssid);
    put_secret_attr(buf, &mut offset, 0x1045, &ssid_nul[..=ssid.len()]);
    put_secret_attr(buf, &mut offset, 0x1003, &auth.to_be_bytes());
    put_secret_attr(buf, &mut offset, 0x100f, &ENCRYPTION_AES.to_be_bytes());
    put_secret_attr(buf, &mut offset, 0x1027, key);
    put_secret_attr(buf, &mut offset, 0x1020, &radio.octets());
    let tag = hash::hmac(HashAlgorithm::Sha256, keys.auth.expose(), &[&buf[..offset]])
        .map_err(|_| Error::Construction)?;
    put_secret_attr(buf, &mut offset, 0x101e, &tag.expose()[..8]);
    let mut iv = [0; 16];
    random.fill(&mut iv).map_err(|_| Error::Construction)?;
    let cipher = cbc::encrypt(keys.wrap.expose(), &iv, plain.expose());
    let mut result = Vec::with_capacity(16 + cipher.len());
    result.extend_from_slice(&iv);
    result.extend_from_slice(&cipher);
    Ok(result)
}

fn make_m2(
    m1: &M1<'_>,
    id: &Identity<'_>,
    private: &DhPrivate,
    n2: &[u8; 16],
    row: Option<(&NetworkSsid<'_>, u16)>,
    haul: u8,
    random: &mut impl RandomSource,
) -> Result<Vec<u8>, Error> {
    let keys = derive_keys(private, m1.public.as_bytes(), &m1.n1, m1.enrollee_mac, n2)?;
    let settings = encrypt_settings(row, haul, m1.radio, &keys, random)?;
    let public = private.public_key();
    let mut writer = Writer::new();
    push_attributes(
        &mut writer,
        &[
            WscAttribute::Version(0x10),
            WscAttribute::MessageType(WSC_M2),
            WscAttribute::EnrolleeNonce(m1.n1),
            WscAttribute::RegistrarNonce(*n2),
            WscAttribute::UuidR(id.uuid),
            WscAttribute::PublicKey(public.as_bytes()),
            WscAttribute::AuthenticationTypeFlags(AUTH_FLAGS),
            WscAttribute::EncryptionTypeFlags(ENCRYPTION_AES),
            WscAttribute::ConnectionTypeFlags(0),
            WscAttribute::ConfigurationMethods(CONFIG_METHODS),
        ],
    )?;
    common_identity(&mut writer, id, m1.band)?;
    push_attributes(
        &mut writer,
        &[
            WscAttribute::ConfigurationError(0),
            WscAttribute::DevicePasswordId(0),
            WscAttribute::OsVersion([0; 4]),
            WscAttribute::EncryptedSettings(&settings),
        ],
    )?;
    let mut attributes = writer.into_bytes();
    let tag = hash::hmac(
        HashAlgorithm::Sha256,
        keys.auth.expose(),
        &[m1.raw, &attributes],
    )
    .map_err(|_| Error::Construction)?;
    let mut auth_writer = Writer::new();
    let mut auth = [0; 8];
    auth.copy_from_slice(&tag.expose()[..8]);
    WscAttribute::Authenticator(auth)
        .encode(&mut auth_writer)
        .map_err(|_| Error::Construction)?;
    attributes.extend_from_slice(&auth_writer.into_bytes());
    Ok(attributes)
}

/// Builds an M2 CMDU with one independently keyed WSC TLV per selected haul.
///
/// # Errors
/// Returns an error for no BSS capacity, randomness, invalid rows or encoding failure.
pub fn build_m2(
    m1: &M1<'_>,
    id: &Identity<'_>,
    rows: &[NetworkSsid<'_>],
    ids: &mut MsgIdCounter,
    random: &mut impl RandomSource,
) -> Result<M2Output, Error> {
    let limit = usize::from(m1.max_bss).min(MAX_HAULS);
    if limit == 0 {
        return Err(Error::NoBssCapacity);
    }
    if !identity_ok(id) {
        return Err(Error::Construction);
    }
    let private = DhPrivate::generate(random).map_err(|_| Error::Construction)?;
    let mut selected = Vec::new();
    for haul in 0..limit {
        let haul = u8::try_from(haul).map_err(|_| Error::Construction)?;
        let effective_auth = |row: &NetworkSsid<'_>| {
            if m1.band == FreqBand::BAND_6_GHZ
                && [AUTH_WPA2_PERSONAL, 0x0010, 0x0001, AUTH_FLAGS]
                    .contains(&row.authentication_type)
            {
                AUTH_SAE
            } else {
                row.authentication_type
            }
        };
        if let Some(row) = rows.iter().find(|row| {
            let auth = effective_auth(row);
            row.enabled
                && row.haul_types.contains(&haul)
                && row.bands.contains(&m1.band)
                && auth != 0
                && (auth & m1.authentication_flags) == auth
        }) {
            selected.push((haul, Some((row, effective_auth(row)))));
        }
    }
    if selected.is_empty() {
        selected.push((0, None));
    }
    let no_fronthaul_psk = !selected.iter().any(|(haul, row)| {
        *haul != 1 && row.is_some_and(|(_, auth)| auth & AUTH_WPA2_PERSONAL != 0)
    });
    if no_fronthaul_psk {
        tracing::warn!("no fronthaul M2 carries PSK");
    }
    let mut builder = MsgBuilder::new(
        &header(MsgType::AP_AUTOCONFIGURATION_WSC, ids.next_id()),
        AL_SAP_BUDGET,
    );
    builder.body(&ApRadioIdentifier(MacValue { mac: m1.radio }));
    let mut used = Vec::<SecretArray<16>>::new();
    for (haul, row) in selected {
        let mut next = None;
        for _ in 0..16 {
            let candidate = random_secret::<16>(random).map_err(|_| Error::Construction)?;
            if used.iter().all(|n| !ct::eq(n.expose(), candidate.expose())) {
                next = Some(candidate);
                break;
            }
        }
        let n2 = next.ok_or(Error::Construction)?;
        let raw = make_m2(m1, id, &private, n2.expose(), row, haul, random)?;
        builder.tlv(TlvType::WSC, &raw);
        used.push(n2);
    }
    Ok(M2Output {
        radio: m1.radio,
        cmdu: builder.finish().map_err(|_| Error::Construction)?,
        no_fronthaul_psk,
    })
}

/// One authenticated configuration returned to the platform.
#[derive(Debug)]
pub struct BssConfig {
    /// SSID bytes without a trailing NUL.
    pub ssid: Vec<u8>,
    /// WSC authentication type.
    pub authentication_type: u16,
    /// Network key, wiped when dropped.
    pub network_key: SecretBytes,
    /// Multi-AP flags including BSS type and Tear Down.
    pub multi_ap: MultiApExtension,
    /// Radio address carried in the settings.
    pub mac: MacAddr,
    /// Comcast haul type, or the WSC index when absent.
    pub haul_type: u8,
}

fn strip_nul(bytes: &[u8]) -> &[u8] {
    bytes.strip_suffix(&[0]).unwrap_or(bytes)
}

fn parse_settings(plaintext: &[u8], keys: &Keys, index: usize) -> Result<BssConfig, Error> {
    // Locate and verify the final Key Wrap Authenticator before interpreting settings.
    let mut tag = None;
    let mut offset = 0;
    for item in WscAttrIter::new(plaintext) {
        let attr = item.map_err(|_| Error::RejectedSettings)?;
        let start = offset;
        offset += 4 + attr.value.len();
        if let WscAttribute::KeyWrapAuthenticator(value) =
            WscAttribute::decode(attr).map_err(|_| Error::RejectedSettings)?
        {
            if offset != plaintext.len() {
                return Err(Error::RejectedSettings);
            }
            unique(&mut tag, (start, value)).map_err(|_| Error::RejectedSettings)?;
        }
    }
    let (tag_offset, tag) = tag.ok_or(Error::RejectedSettings)?;
    let expected = hash::hmac(
        HashAlgorithm::Sha256,
        keys.auth.expose(),
        &[&plaintext[..tag_offset]],
    )
    .map_err(|_| Error::RejectedSettings)?;
    if !ct::eq(&tag, &expected.expose()[..8]) {
        return Err(Error::RejectedSettings);
    }

    let (mut multi, mut haul, mut ssid, mut auth, mut encryption, mut key, mut mac) =
        (None, None, None, None, None, None, None);
    for item in WscAttrIter::new(&plaintext[..tag_offset]) {
        let attr = item.map_err(|_| Error::RejectedSettings)?;
        let value = WscAttribute::decode(attr).map_err(|_| Error::RejectedSettings)?;
        match value {
            WscAttribute::VendorExtension(v) => {
                if let Some(value) = MultiApExtension::from_vendor_extension(&v)
                    .map_err(|_| Error::RejectedSettings)?
                {
                    unique(&mut multi, value).map_err(|_| Error::RejectedSettings)?;
                }
                if let Some(value) = HaulTypeExtension::from_vendor_extension(&v)
                    .map_err(|_| Error::RejectedSettings)?
                {
                    unique(&mut haul, value.haul_type).map_err(|_| Error::RejectedSettings)?;
                }
            }
            WscAttribute::Ssid(v) => unique(&mut ssid, v).map_err(|_| Error::RejectedSettings)?,
            WscAttribute::AuthenticationType(v) => {
                unique(&mut auth, v).map_err(|_| Error::RejectedSettings)?;
            }
            WscAttribute::EncryptionType(v) => {
                unique(&mut encryption, v).map_err(|_| Error::RejectedSettings)?;
            }
            WscAttribute::NetworkKey(v) => {
                unique(&mut key, v).map_err(|_| Error::RejectedSettings)?;
            }
            WscAttribute::MacAddress(v) => {
                unique(&mut mac, v).map_err(|_| Error::RejectedSettings)?;
            }
            _ => {}
        }
    }
    let multi = multi.ok_or(Error::RejectedSettings)?;
    let ssid = strip_nul(ssid.ok_or(Error::RejectedSettings)?);
    let key = strip_nul(key.ok_or(Error::RejectedSettings)?);
    if ssid.len() > MAX_SSID_LEN
        || key.len() > MAX_KEY_LEN
        || ssid.contains(&0)
        || key.contains(&0)
        || encryption != Some(ENCRYPTION_AES)
    {
        return Err(Error::RejectedSettings);
    }
    Ok(BssConfig {
        ssid: ssid.to_vec(),
        authentication_type: auth.ok_or(Error::RejectedSettings)?,
        network_key: SecretBytes::from_slice(key),
        multi_ap: multi,
        mac: mac.ok_or(Error::RejectedSettings)?,
        haul_type: haul.unwrap_or(u8::try_from(index).map_err(|_| Error::RejectedSettings)?),
    })
}

fn check_m2(
    raw: &[u8],
    exchange: &Exchange,
    index: usize,
) -> Result<(BssConfig, SecretArray<16>, Keys), Error> {
    if raw.len() > MAX_WSC_LEN {
        return Err(Error::InvalidMessage);
    }
    let (mut kind, mut n1, mut n2, mut public, mut encrypted, mut tag) =
        (None, None, None, None, None, None);
    let mut offset = 0;
    for item in WscAttrIter::new(raw) {
        let attr = item.map_err(|_| Error::InvalidMessage)?;
        let start = offset;
        offset += 4 + attr.value.len();
        match WscAttribute::decode(attr).map_err(|_| Error::InvalidMessage)? {
            WscAttribute::MessageType(v) => unique(&mut kind, v)?,
            WscAttribute::EnrolleeNonce(v) => unique(&mut n1, v)?,
            WscAttribute::RegistrarNonce(v) => unique(&mut n2, v)?,
            WscAttribute::PublicKey(v) => {
                if v.len() != DH_VALUE_LEN {
                    return Err(Error::InvalidMessage);
                }
                unique(&mut public, v)?;
            }
            WscAttribute::EncryptedSettings(v) => unique(&mut encrypted, v)?,
            WscAttribute::Authenticator(v) => {
                if offset != raw.len() {
                    return Err(Error::InvalidMessage);
                }
                unique(&mut tag, (start, v))?;
            }
            _ => {}
        }
    }
    if kind != Some(WSC_M2) || !ct::eq(&n1.ok_or(Error::InvalidMessage)?, exchange.n1.expose()) {
        return Err(Error::InvalidMessage);
    }
    let n2 = n2.ok_or(Error::InvalidMessage)?;
    let keys = derive_keys(
        &exchange.private,
        public.ok_or(Error::InvalidMessage)?,
        exchange.n1.expose(),
        exchange.enrollee_mac,
        &n2,
    )?;
    let (tag_offset, tag) = tag.ok_or(Error::InvalidMessage)?;
    let calculated = hash::hmac(
        HashAlgorithm::Sha256,
        keys.auth.expose(),
        &[exchange.m1.expose(), &raw[..tag_offset]],
    )
    .map_err(|_| Error::Construction)?;
    if !ct::eq(&tag, &calculated.expose()[..8]) {
        return Err(Error::Authenticator);
    }
    let encrypted = encrypted.ok_or(Error::InvalidMessage)?;
    if encrypted.len() < 32 || encrypted.len() > 1024 || encrypted.len() % 16 != 0 {
        return Err(Error::RejectedSettings);
    }
    let iv: &[u8; 16] = encrypted[..16]
        .try_into()
        .map_err(|_| Error::RejectedSettings)?;
    let plain = cbc::decrypt(keys.wrap.expose(), iv, &encrypted[16..])
        .map_err(|_| Error::RejectedSettings)?;
    let record = parse_settings(plain.expose(), &keys, index)?;
    let nonce = SecretArray::from_slice(&n2).map_err(|_| Error::Construction)?;
    Ok((record, nonce, keys))
}

/// Checks every M2 TLV before accepting any configuration. A failure retains the session.
///
/// # Errors
/// Rejects incorrect peer/radio, framing, authenticators, padding or settings.
pub fn accept_m2(
    session: &mut Session,
    state: AgentState,
    sender: MacAddr,
    cmdu: &[u8],
) -> Result<Vec<BssConfig>, Error> {
    if state != AgentState::WscM2Pending {
        return Err(Error::WrongState);
    }
    let exchange = session.exchange.as_ref().ok_or(Error::NoSession)?;
    if sender != exchange.controller {
        return Err(Error::WrongPeer);
    }
    let expected_radio = exchange.radio;
    let max_bss = exchange.max_bss;
    let rest = area(cmdu, MsgType::AP_AUTOCONFIGURATION_WSC)?;
    em_msg::validate(cmdu, ProfileType::from(3)).map_err(|_| Error::InvalidMessage)?;
    let (mut radio, mut payloads) = (None, Vec::new());
    for item in TlvIter::new(rest) {
        let tlv = item.map_err(|_| Error::InvalidMessage)?;
        if tlv.tlv_type == TlvType::AP_RADIO_IDENTIFIER {
            unique(
                &mut radio,
                ApRadioIdentifier::decode_value(tlv.value)
                    .map_err(|_| Error::InvalidMessage)?
                    .0
                    .mac,
            )?;
        } else if tlv.tlv_type == TlvType::WSC {
            if payloads.len() >= usize::from(max_bss) {
                return Err(Error::InvalidMessage);
            }
            payloads.push(tlv.value);
        }
    }
    if radio != Some(expected_radio) {
        return Err(Error::WrongPeer);
    }
    if payloads.is_empty() {
        return Err(Error::InvalidMessage);
    }
    let mut records = Vec::with_capacity(payloads.len());
    for payload in payloads {
        let (record, nonce, keys) = check_m2(
            payload,
            session.exchange.as_ref().ok_or(Error::NoSession)?,
            records.len(),
        )?;
        if record.mac != expected_radio
            || (!record.multi_ap.tear_down
                && (record.authentication_type == 0
                    || record.authentication_type & AUTH_FLAGS != record.authentication_type))
        {
            return Err(Error::RejectedSettings);
        }
        let exchange = session.exchange.as_mut().ok_or(Error::NoSession)?;
        exchange.registrar_nonce = Some(nonce);
        exchange.keys = Some(keys);
        records.push(record);
    }
    if records.len() > 1 && records.iter().any(|record| record.multi_ap.tear_down) {
        return Err(Error::RejectedSettings);
    }
    session.clear();
    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::*;
    use em_crypto::rand::RandomError;
    use em_msg::bodies::BasicOperatingClass;

    const AL: MacAddr = MacAddr::new([2, 1, 2, 3, 4, 5]);
    const RADIO: MacAddr = MacAddr::new([2, 1, 2, 3, 4, 6]);
    const CTRL: MacAddr = MacAddr::new([2, 1, 2, 3, 4, 7]);

    struct TestRandom(u8);
    impl RandomSource for TestRandom {
        fn fill(&mut self, dest: &mut [u8]) -> Result<(), RandomError> {
            self.0 = self.0.wrapping_add(1).max(2);
            dest.fill(self.0);
            if dest.len() == DH_VALUE_LEN {
                dest.fill(0);
                dest[DH_VALUE_LEN - 1] = self.0;
            }
            Ok(())
        }
    }
    fn identity() -> Identity<'static> {
        Identity {
            uuid: [9; 16],
            manufacturer: b"example",
            model: b"model",
            serial: b"1234",
            primary_device_type: [0, 6, 0, 0x50, 0xf2, 4, 0, 1],
        }
    }
    fn params() -> M1Params<'static> {
        M1Params {
            al_mac: AL,
            controller: CTRL,
            band: FreqBand::BAND_5_GHZ,
            identity: identity(),
            basic: ApRadioBasicCapabilities {
                ruid: RADIO,
                max_bss: 2,
                operating_classes: vec![BasicOperatingClass {
                    operating_class: 81,
                    max_tx_power: 20,
                    non_operable_channels: &[1, 2],
                }],
            },
            profile2: Profile2ApCapability {
                max_prioritization_rules: 3,
                byte_counter_units: 1,
                prioritization: true,
                dpp_onboarding: false,
                traffic_separation: true,
                max_vids: 4,
            },
            advanced: ApRadioAdvancedCapabilities {
                ruid: RADIO,
                combined_front_back: true,
                combined_profile_1_2: true,
                mscs: false,
                scs: false,
                qos_map: false,
                dscp_policy: false,
                qm_scs_traffic_description: false,
            },
        }
    }
    fn row(
        haul: &'static [u8],
        ssid: &'static [u8],
        key: &'static [u8],
        auth: u16,
        enabled: bool,
    ) -> NetworkSsid<'static> {
        NetworkSsid {
            enabled,
            ssid,
            network_key: key,
            bands: &[FreqBand::BAND_5_GHZ],
            haul_types: haul,
            authentication_type: auth,
        }
    }
    fn setup() -> (Session, Vec<u8>, MsgIdCounter, TestRandom) {
        let mut session = Session::new();
        let mut ids = MsgIdCounter::new();
        let mut random = TestRandom(0);
        let m1 = build_m1(&params(), &mut ids, &mut random, &mut session).unwrap();
        (session, m1, ids, random)
    }
    fn wsc_payloads(frame: &[u8]) -> Vec<&[u8]> {
        let (_, area) = CmduHeader::parse(frame).unwrap();
        TlvIter::new(area)
            .map(Result::unwrap)
            .filter(|tlv| tlv.tlv_type == TlvType::WSC)
            .map(|tlv| tlv.value)
            .collect()
    }
    fn attrs(payload: &[u8]) -> Vec<WscAttribute<'_>> {
        WscAttrIter::new(payload)
            .map(|v| WscAttribute::decode(v.unwrap()).unwrap())
            .collect()
    }
    fn m2_parts(payload: &[u8]) -> ([u8; 16], &[u8], &[u8]) {
        let (mut nonce, mut public, mut settings) = (None, None, None);
        for attr in attrs(payload) {
            match attr {
                WscAttribute::RegistrarNonce(v) => nonce = Some(v),
                WscAttribute::PublicKey(v) => public = Some(v),
                WscAttribute::EncryptedSettings(v) => settings = Some(v),
                _ => {}
            }
        }
        (nonce.unwrap(), public.unwrap(), settings.unwrap())
    }
    fn rebuild(payloads: &[&[u8]], mid: u16) -> Vec<u8> {
        let mut b = MsgBuilder::new(
            &header(MsgType::AP_AUTOCONFIGURATION_WSC, mid),
            AL_SAP_BUDGET,
        );
        b.body(&ApRadioIdentifier(MacValue { mac: RADIO }));
        for payload in payloads {
            b.tlv(TlvType::WSC, payload);
        }
        b.finish().unwrap()
    }
    #[allow(clippy::too_many_arguments)]
    fn raw_compatible_m2(
        m1: &M1<'_>,
        private: &DhPrivate,
        n2: &[u8; 16],
        row: &NetworkSsid<'_>,
        haul: u8,
        rng: &mut TestRandom,
        nul_key: bool,
        bad_tag: bool,
        bad_padding: bool,
    ) -> Vec<u8> {
        let original = make_m2(
            m1,
            &identity(),
            private,
            n2,
            Some((row, row.authentication_type)),
            haul,
            rng,
        )
        .unwrap();
        let keys = derive_keys(private, m1.public.as_bytes(), &m1.n1, m1.enrollee_mac, n2).unwrap();
        let (_, _, encrypted) = m2_parts(&original);
        let iv: &[u8; 16] = encrypted[..16].try_into().unwrap();
        let plain = cbc::decrypt(keys.wrap.expose(), iv, &encrypted[16..]).unwrap();
        let mut settings_writer = Writer::new();
        for attr in attrs(plain.expose()) {
            match attr {
                WscAttribute::KeyWrapAuthenticator(_) => {}
                WscAttribute::NetworkKey(value) if nul_key => {
                    let mut key = value.to_vec();
                    key.push(0);
                    WscAttribute::NetworkKey(&key)
                        .encode(&mut settings_writer)
                        .unwrap();
                }
                _ => attr.encode(&mut settings_writer).unwrap(),
            }
        }
        let mut settings = settings_writer.into_bytes();
        let hmac = hash::hmac(HashAlgorithm::Sha256, keys.auth.expose(), &[&settings]).unwrap();
        let mut tag: [u8; 8] = hmac.expose()[..8].try_into().unwrap();
        if bad_tag {
            tag[0] ^= 1;
        }
        let mut writer = Writer::new();
        WscAttribute::KeyWrapAuthenticator(tag)
            .encode(&mut writer)
            .unwrap();
        settings.extend_from_slice(&writer.into_bytes());
        let mut encrypted_new = iv.to_vec();
        encrypted_new.extend_from_slice(&cbc::encrypt(keys.wrap.expose(), iv, &settings));
        if bad_padding {
            let padding = u8::try_from(16 - settings.len() % 16).unwrap();
            let previous_block_last = encrypted_new.len() - 17;
            encrypted_new[previous_block_last] ^= padding;
        }
        let mut writer = Writer::new();
        for attr in attrs(&original) {
            match attr {
                WscAttribute::Authenticator(_) => {}
                WscAttribute::EncryptedSettings(_) => {
                    WscAttribute::EncryptedSettings(&encrypted_new)
                        .encode(&mut writer)
                        .unwrap();
                }
                _ => attr.encode(&mut writer).unwrap(),
            }
        }
        let mut changed = writer.into_bytes();
        let outer = hash::hmac(
            HashAlgorithm::Sha256,
            keys.auth.expose(),
            &[m1.raw, &changed],
        )
        .unwrap();
        let mut writer = Writer::new();
        WscAttribute::Authenticator(outer.expose()[..8].try_into().unwrap())
            .encode(&mut writer)
            .unwrap();
        changed.extend_from_slice(&writer.into_bytes());
        changed
    }

    #[test]
    fn m1_bytes_and_upstream_style_compatibility() {
        let (_session, frame, _, _) = setup();
        let digest = hash::digest(HashAlgorithm::Sha256, &[&frame]);
        assert_eq!(
            digest.expose(),
            &[
                0x7c, 0xc5, 0xd2, 0x1c, 0x3a, 0x61, 0x4f, 0x69, 0x40, 0xca, 0xff, 0x4c, 0x3e, 0xe0,
                0x32, 0x4c, 0x95, 0xc7, 0x44, 0x46, 0x30, 0x43, 0x62, 0xc5, 0xff, 0x0b, 0x3f, 0x38,
                0x41, 0x30, 0x79, 0xe4
            ]
        );
        let parsed = parse_m1(&frame).unwrap();
        assert_eq!(
            (parsed.radio, parsed.enrollee_mac, parsed.band),
            (RADIO, AL, FreqBand::BAND_5_GHZ)
        );
        assert_eq!(parsed.authentication_flags, AUTH_FLAGS);
        // intentional-difference:m1-mac-al em_configuration.cpp:3346-3353
        assert!(attrs(parsed.raw).contains(&WscAttribute::MacAddress(AL)));
        // intentional-difference:m1-auth-flags em_configuration.cpp:3376-3415
        assert!(attrs(parsed.raw).contains(&WscAttribute::AuthenticationTypeFlags(AUTH_FLAGS)));
        // intentional-difference:m1-short-attrs em_configuration.cpp:3537-3563
        for a in [
            WscAttribute::AssociationState(0),
            WscAttribute::DevicePasswordId(0),
            WscAttribute::ConfigurationError(0),
        ] {
            assert!(attrs(parsed.raw).contains(&a));
        }
        // intentional-difference:m1-capability-tlvs em_configuration.cpp:4319-4333
        let (_, area) = CmduHeader::parse(&frame).unwrap();
        let tlvs: Vec<_> = TlvIter::new(area).map(Result::unwrap).collect();
        assert_eq!(
            tlvs.iter().map(|t| t.tlv_type).collect::<Vec<_>>(),
            [
                TlvType::AP_RADIO_BASIC_CAPABILITIES,
                TlvType::WSC,
                TlvType::PROFILE_2_AP_CAPABILITY,
                TlvType::AP_RADIO_ADVANCED_CAPABILITIES
            ]
        );
        assert_eq!(
            Profile2ApCapability::decode_value(tlvs[2].value).unwrap(),
            params().profile2
        );
        assert_eq!(
            ApRadioAdvancedCapabilities::decode_value(tlvs[3].value).unwrap(),
            params().advanced
        );
        let mut w = Writer::new();
        for a in attrs(parsed.raw) {
            match a {
                WscAttribute::MacAddress(_) => {
                    WscAttribute::MacAddress(RADIO).encode(&mut w).unwrap();
                }
                WscAttribute::AuthenticationTypeFlags(_) => {
                    WscAttribute::AuthenticationTypeFlags(0)
                        .encode(&mut w)
                        .unwrap();
                }
                _ => a.encode(&mut w).unwrap(),
            }
        }
        let raw = w.into_bytes();
        let mut b = MsgBuilder::new(&header(MsgType::AP_AUTOCONFIGURATION_WSC, 77), RAW_BUDGET);
        b.body(&params().basic)
            .tlv(TlvType::WSC, &raw)
            .body(&params().profile2)
            .body(&params().advanced);
        let upstream = b.finish().unwrap();
        let parsed = parse_m1(&upstream).unwrap();
        assert_eq!(
            (parsed.enrollee_mac, parsed.authentication_flags),
            (RADIO, 0)
        );
    }

    #[test]
    fn upstream_radio_mac_and_zero_flags_use_the_m1_emac_for_m2_keys() {
        let (mut session, frame, mut ids, mut random) = setup();
        let original = parse_m1(&frame).unwrap();
        let mut writer = Writer::new();
        for attribute in attrs(original.raw) {
            match attribute {
                WscAttribute::MacAddress(_) => {
                    WscAttribute::MacAddress(RADIO).encode(&mut writer).unwrap();
                }
                WscAttribute::AuthenticationTypeFlags(_) => {
                    WscAttribute::AuthenticationTypeFlags(0)
                        .encode(&mut writer)
                        .unwrap();
                }
                _ => attribute.encode(&mut writer).unwrap(),
            }
        }
        let raw = writer.into_bytes();
        let mut builder =
            MsgBuilder::new(&header(MsgType::AP_AUTOCONFIGURATION_WSC, 77), RAW_BUDGET);
        builder
            .body(&params().basic)
            .tlv(TlvType::WSC, &raw)
            .body(&params().profile2)
            .body(&params().advanced);
        let upstream = builder.finish().unwrap();
        let parsed = parse_m1(&upstream).unwrap();
        let exchange = session.exchange.as_mut().unwrap();
        exchange.enrollee_mac = RADIO;
        exchange.m1 = SecretBytes::from_slice(parsed.raw);
        let result = build_m2(
            &parsed,
            &identity(),
            &[row(&[0], b"test", b"password", AUTH_WPA2_PERSONAL, true)],
            &mut ids,
            &mut random,
        )
        .unwrap();
        let config = accept_m2(&mut session, AgentState::WscM2Pending, CTRL, &result.cmdu).unwrap();
        assert_eq!(config.len(), 1);
        assert!(config[0].multi_ap.tear_down);
    }

    #[test]
    fn m2_round_trip_and_unique_nonce_upstream_failure() {
        let (mut session, frame, mut ids, mut random) = setup();
        let m1 = parse_m1(&frame).unwrap();
        let disabled = row(&[0], b"disabled", b"wrong", AUTH_WPA2_PERSONAL, false);
        let front = row(&[0], b"front", b"password", AUTH_WPA2_PERSONAL, true);
        let back = row(&[1], b"back", b"another", AUTH_SAE, true);
        // intentional-difference:m2-row-search dm_easy_mesh.cpp:2853-2874 and em_configuration.cpp:2908-2941
        let output = build_m2(
            &m1,
            &identity(),
            &[disabled, front, back],
            &mut ids,
            &mut random,
        )
        .unwrap();
        assert!(!output.no_fronthaul_psk);
        let digest = hash::digest(HashAlgorithm::Sha256, &[&output.cmdu]);
        assert_eq!(
            digest.expose(),
            &[
                0x87, 0x2c, 0x77, 0x98, 0x47, 0x12, 0xe5, 0x80, 0x2b, 0x88, 0xa2, 0x9d, 0x68, 0xb2,
                0x2c, 0x52, 0x01, 0x01, 0xf5, 0x06, 0x28, 0xf2, 0x11, 0x47, 0x99, 0xfe, 0xa8, 0x76,
                0x36, 0xcf, 0xfd, 0x85
            ]
        );
        // intentional-difference:m2-new-mid em_configuration.cpp:4177-4182
        assert_ne!(
            CmduHeader::parse(&frame).unwrap().0.msg_id,
            CmduHeader::parse(&output.cmdu).unwrap().0.msg_id
        );
        let payloads = wsc_payloads(&output.cmdu);
        assert_eq!(payloads.len(), 2);
        let (first_nonce, first_public, encrypted_a) = m2_parts(payloads[0]);
        let (second_nonce, second_public, _) = m2_parts(payloads[1]);
        // intentional-difference:m2-unique-n2 em_configuration.cpp:2970-2976,4889-4895
        assert_ne!(first_nonce, second_nonce);
        // A model of upstream's last-key-for-all-TLVs decryption cannot decrypt the first.
        let exchange = session.exchange.as_ref().unwrap();
        let first = derive_keys(
            &exchange.private,
            first_public,
            exchange.n1.expose(),
            AL,
            &first_nonce,
        )
        .unwrap();
        let iv_first: &[u8; 16] = encrypted_a[..16].try_into().unwrap();
        let plain_first = cbc::decrypt(first.wrap.expose(), iv_first, &encrypted_a[16..]).unwrap();
        assert!(attrs(plain_first.expose()).contains(&WscAttribute::NetworkKey(b"password")));
        let last = derive_keys(
            &exchange.private,
            second_public,
            exchange.n1.expose(),
            AL,
            &second_nonce,
        )
        .unwrap();
        let iv: &[u8; 16] = encrypted_a[..16].try_into().unwrap();
        let wrong = cbc::decrypt(last.wrap.expose(), iv, &encrypted_a[16..]);
        assert!(wrong.is_err());
        // intentional-difference:m2-flags em_configuration.cpp:2995-3030
        assert!(attrs(payloads[0]).contains(&WscAttribute::AuthenticationTypeFlags(AUTH_FLAGS)));
        assert!(attrs(payloads[0]).contains(&WscAttribute::EncryptionTypeFlags(ENCRYPTION_AES)));
        assert!(attrs(payloads[0]).contains(&WscAttribute::ConfigurationMethods(CONFIG_METHODS)));
        // intentional-difference:separate-dh-keys em_crypto.cpp:210-234
        assert_ne!(m1.public.as_bytes().as_slice(), second_public);
        let effects = output.post_handoff_effects(AL);
        assert!(matches!(
            effects,
            [
                ControllerEffect::RemoveChainForNode(_),
                ControllerEffect::SetState(_, CtrlState::WscM2Sent),
                ControllerEffect::SubmitEmConfig { .. }
            ]
        ));
        let result = accept_m2(&mut session, AgentState::WscM2Pending, CTRL, &output.cmdu).unwrap();
        assert!(!session.is_active());
        assert_eq!(result.len(), 2);
        assert_eq!(
            (&result[0].ssid[..], result[0].network_key.expose()),
            (&b"front"[..], &b"password"[..])
        );
        assert_eq!(
            (&result[1].ssid[..], result[1].network_key.expose()),
            (&b"back"[..], &b"another"[..])
        );
        // intentional-difference:network-key-no-nul em_configuration.cpp:5963-5967
        assert!(!result[0].network_key.expose().ends_with(&[0]));
        // intentional-difference:multiap-ext em_configuration.cpp:4909-4943
        assert!(result[0].multi_ap.fronthaul_bss && result[1].multi_ap.backhaul_bss);
        assert_eq!((result[0].haul_type, result[1].haul_type), (0, 1));
    }

    #[test]
    fn shared_nonce_and_terminated_key_are_accepted() {
        let (mut session, frame, _, mut random) = setup();
        let m1 = parse_m1(&frame).unwrap();
        let private = DhPrivate::from_be_bytes(&[77]).unwrap();
        let n2 = [5; 16];
        let row1 = row(&[0], b"first", b"password", AUTH_WPA2_PERSONAL, true);
        let row2 = row(&[1], b"second", b"secret", AUTH_SAE, true);
        let one = raw_compatible_m2(
            &m1,
            &private,
            &n2,
            &row1,
            0,
            &mut random,
            true,
            false,
            false,
        );
        let two = raw_compatible_m2(
            &m1,
            &private,
            &n2,
            &row2,
            1,
            &mut random,
            true,
            false,
            false,
        );
        let response = rebuild(&[&one, &two], 17);
        let result = accept_m2(&mut session, AgentState::WscM2Pending, CTRL, &response).unwrap();
        assert_eq!(result[0].network_key.expose(), b"password");
        assert_eq!(result[1].network_key.expose(), b"secret");
    }

    #[test]
    fn unsupported_rows_teardown_and_warning() {
        let (mut session, frame, mut ids, mut random) = setup();
        let m1 = parse_m1(&frame).unwrap();
        let output = build_m2(
            &m1,
            &identity(),
            &[
                row(&[0], b"x", b"pass", 0x0080, true),
                row(&[1], b"y", b"pass", AUTH_SAE, true),
            ],
            &mut ids,
            &mut random,
        )
        .unwrap();
        // intentional-difference:m2-auth-from-m1 em_configuration.cpp:5884-5893
        assert_eq!(wsc_payloads(&output.cmdu).len(), 1);
        assert!(output.no_fronthaul_psk);
        assert!(
            accept_m2(&mut session, AgentState::WscM2Pending, CTRL, &output.cmdu).unwrap()[0]
                .multi_ap
                .backhaul_bss
        );
        let (mut session, frame, mut ids, mut random) = setup();
        let m1 = parse_m1(&frame).unwrap();
        // intentional-difference:teardown-m2-no-row em_configuration.cpp:4223-4227
        let output = build_m2(&m1, &identity(), &[], &mut ids, &mut random).unwrap();
        assert!(output.no_fronthaul_psk);
        let result = accept_m2(&mut session, AgentState::WscM2Pending, CTRL, &output.cmdu).unwrap();
        assert_eq!(result.len(), 1);
        assert!(result[0].multi_ap.tear_down);
    }

    #[test]
    fn agent_accepts_up_to_its_advertised_bss_limit() {
        let mut session = Session::new();
        let mut ids = MsgIdCounter::new();
        let mut random = TestRandom(0);
        let mut p = params();
        p.basic.max_bss = 6;
        let frame = build_m1(&p, &mut ids, &mut random, &mut session).unwrap();
        let m1 = parse_m1(&frame).unwrap();
        let private = DhPrivate::from_be_bytes(&[42]).unwrap();
        let row = row(&[0], b"test", b"password", AUTH_WPA2_PERSONAL, true);
        let mut payloads = Vec::new();
        for haul in 0..6 {
            payloads.push(
                make_m2(
                    &m1,
                    &identity(),
                    &private,
                    &[9; 16],
                    Some((&row, AUTH_WPA2_PERSONAL)),
                    haul,
                    &mut random,
                )
                .unwrap(),
            );
        }
        let refs: Vec<&[u8]> = payloads.iter().map(Vec::as_slice).collect();
        let six = rebuild(&refs, 42);
        assert_eq!(
            accept_m2(&mut session, AgentState::WscM2Pending, CTRL, &six)
                .unwrap()
                .len(),
            6
        );
    }

    #[test]
    fn mixed_tear_down_and_bss_is_rejected_atomically() {
        let (mut session, frame, mut ids, mut random) = setup();
        let m1 = parse_m1(&frame).unwrap();
        let teardown = build_m2(&m1, &identity(), &[], &mut ids, &mut random).unwrap();
        let normal = build_m2(
            &m1,
            &identity(),
            &[row(&[0], b"test", b"password", AUTH_WPA2_PERSONAL, true)],
            &mut ids,
            &mut random,
        )
        .unwrap();
        let teardown_payload = wsc_payloads(&teardown.cmdu);
        let normal_payload = wsc_payloads(&normal.cmdu);
        let mixed = rebuild(&[teardown_payload[0], normal_payload[0]], 24);
        assert_eq!(
            accept_m2(&mut session, AgentState::WscM2Pending, CTRL, &mixed).unwrap_err(),
            Error::RejectedSettings
        );
        assert!(session.is_active());
        assert!(accept_m2(&mut session, AgentState::WscM2Pending, CTRL, &normal.cmdu).is_ok());
    }

    #[test]
    fn six_ghz_uses_upstreams_sae_conversion_before_capability_check() {
        let mut session = Session::new();
        let mut ids = MsgIdCounter::new();
        let mut random = TestRandom(0);
        let mut p = params();
        p.band = FreqBand::BAND_6_GHZ;
        let frame = build_m1(&p, &mut ids, &mut random, &mut session).unwrap();
        let m1 = parse_m1(&frame).unwrap();
        let row = NetworkSsid {
            enabled: true,
            ssid: b"six",
            network_key: b"password",
            bands: &[FreqBand::BAND_6_GHZ],
            haul_types: &[0],
            authentication_type: AUTH_WPA2_PERSONAL,
        };
        // Upstream em_configuration.cpp:5884-5893 uses SAE for WPA2 on 6 GHz.
        let output = build_m2(&m1, &identity(), &[row], &mut ids, &mut random).unwrap();
        assert!(output.no_fronthaul_psk);
        let bsses = accept_m2(&mut session, AgentState::WscM2Pending, CTRL, &output.cmdu).unwrap();
        assert_eq!(bsses[0].authentication_type, AUTH_SAE);
    }

    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "one exchange exercises atomic failure and terminal paths"
    )]
    fn rejection_preserves_session_and_clearing_ends_exchange() {
        let (mut session, frame, mut ids, mut random) = setup();
        let m1 = parse_m1(&frame).unwrap();
        let front = row(&[0], b"test", b"password", AUTH_WPA2_PERSONAL, true);
        let good = build_m2(&m1, &identity(), &[front], &mut ids, &mut random)
            .unwrap()
            .cmdu;
        let mut broken = Vec::with_capacity(good.len());
        broken.extend_from_slice(&good);
        let end = broken.len();
        broken[end - 4] ^= 1;
        // intentional-difference:m2-authenticator em_configuration.cpp:4820-4823
        assert_eq!(
            accept_m2(&mut session, AgentState::WscM2Pending, CTRL, &broken).unwrap_err(),
            Error::Authenticator
        );
        assert!(session.is_active());
        assert_eq!(
            accept_m2(&mut session, AgentState::OwconfigPending, CTRL, &good).unwrap_err(),
            Error::WrongState
        );
        assert_eq!(
            accept_m2(&mut session, AgentState::WscM2Pending, AL, &good).unwrap_err(),
            Error::WrongPeer
        );
        assert!(session.is_active());
        let private = DhPrivate::from_be_bytes(&[42]).unwrap();
        let row = row(&[0], b"test", b"password", AUTH_WPA2_PERSONAL, true);
        let invalid = raw_compatible_m2(
            &m1,
            &private,
            &[7; 16],
            &row,
            0,
            &mut random,
            false,
            true,
            false,
        );
        // intentional-difference:key-wrap-auth em_configuration.cpp:4941-4943
        assert_eq!(
            accept_m2(
                &mut session,
                AgentState::WscM2Pending,
                CTRL,
                &rebuild(&[&invalid], 22)
            )
            .unwrap_err(),
            Error::RejectedSettings
        );
        assert!(session.is_active());
        let padding = raw_compatible_m2(
            &m1,
            &private,
            &[8; 16],
            &row,
            0,
            &mut random,
            false,
            false,
            true,
        );
        assert_eq!(
            accept_m2(
                &mut session,
                AgentState::WscM2Pending,
                CTRL,
                &rebuild(&[&padding], 23)
            )
            .unwrap_err(),
            Error::RejectedSettings
        );
        let good_raw = wsc_payloads(&good);
        assert_eq!(
            accept_m2(
                &mut session,
                AgentState::WscM2Pending,
                CTRL,
                &rebuild(&[good_raw[0], &invalid], 24)
            )
            .unwrap_err(),
            Error::RejectedSettings
        );
        assert!(session.exchange.as_ref().unwrap().registrar_nonce.is_some());
        assert!(session.exchange.as_ref().unwrap().keys.is_some());
        assert!(accept_m2(&mut session, AgentState::WscM2Pending, CTRL, &good).is_ok());
        assert!(!session.is_active());
        for _terminal_reason in ["timeout", "renew", "removal", "stop"] {
            let (mut session, old_m1, _, _) = setup();
            assert!(session.is_active());
            session.clear();
            assert!(!session.is_active());
            let mut random = TestRandom(10);
            let mut ids = MsgIdCounter::new();
            build_m1(&params(), &mut ids, &mut random, &mut session).unwrap();
            assert_ne!(
                session.exchange.as_ref().unwrap().m1.expose(),
                wsc_payloads(&old_m1)[0]
            );
            assert_eq!(
                accept_m2(&mut session, AgentState::WscM2Pending, CTRL, &good).unwrap_err(),
                Error::InvalidMessage
            );
        }
    }

    #[test]
    fn random_failure_does_not_replace_a_live_exchange() {
        let (mut session, frame, mut ids, mut random) = setup();
        let m1 = parse_m1(&frame).unwrap();
        let response = build_m2(
            &m1,
            &identity(),
            &[row(&[0], b"test", b"password", AUTH_WPA2_PERSONAL, true)],
            &mut ids,
            &mut random,
        )
        .unwrap();
        let mut failing = em_crypto::rand::FailingRandom;
        assert_eq!(
            build_m1(&params(), &mut ids, &mut failing, &mut session).unwrap_err(),
            Error::Construction
        );
        assert!(session.is_active());
        assert_eq!(
            build_m2(&m1, &identity(), &[], &mut ids, &mut failing).unwrap_err(),
            Error::Construction
        );
        assert!(accept_m2(&mut session, AgentState::WscM2Pending, CTRL, &response.cmdu).is_ok());
    }

    #[test]
    fn replacing_a_live_exchange_drops_the_previous_session() {
        let (mut session, old_m1, mut ids, mut random) = setup();
        let old = parse_m1(&old_m1).unwrap();
        let response = build_m2(
            &old,
            &identity(),
            &[row(&[0], b"test", b"password", AUTH_WPA2_PERSONAL, true)],
            &mut ids,
            &mut random,
        )
        .unwrap();
        let new_m1 = build_m1(&params(), &mut ids, &mut random, &mut session).unwrap();
        assert_ne!(wsc_payloads(&old_m1), wsc_payloads(&new_m1));
        assert_eq!(
            accept_m2(&mut session, AgentState::WscM2Pending, CTRL, &response.cmdu).unwrap_err(),
            Error::InvalidMessage
        );
        assert!(session.is_active());
        session.clear();
        assert!(!session.is_active());
    }

    #[test]
    fn akm_limit_and_malformed_inputs() {
        // intentional-difference:akm-clamp-10 em_configuration.cpp:4701-4711
        let names = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11"];
        assert_eq!(clamp_akms(names).len(), 10);
        let (mut session, frame, mut ids, mut random) = setup();
        let m1 = parse_m1(&frame).unwrap();
        let good = build_m2(
            &m1,
            &identity(),
            &[row(&[0], b"test", b"password", AUTH_WPA2_PERSONAL, true)],
            &mut ids,
            &mut random,
        )
        .unwrap()
        .cmdu;
        // intentional-difference:bounded-wsc-parse em_configuration.cpp:4605-4629,4668-4755
        for len in 0..frame.len() {
            assert!(parse_m1(&frame[..len]).is_err());
        }
        for len in 0..good.len() {
            assert!(accept_m2(&mut session, AgentState::WscM2Pending, CTRL, &good[..len]).is_err());
        }
        for byte in 0..=255_u8 {
            let input = [byte; 47];
            assert!(parse_m1(&input).is_err());
            assert!(accept_m2(&mut session, AgentState::WscM2Pending, CTRL, &input).is_err());
        }
        assert!(session.is_active());
    }

    fn unhex(input: &str) -> Vec<u8> {
        input
            .as_bytes()
            .chunks_exact(2)
            .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
            .collect()
    }

    #[test]
    fn derived_key_matches_upstream_and_minimal_dh_secret() {
        // em_crypto.cpp:1122-1131 oracle yields one-byte 08, not 192 padded octets.
        let private = DhPrivate::from_be_bytes(&[3]).unwrap();
        assert_eq!(private.shared_secret(&[2]).unwrap().expose(), &[8]);
        let mut n1 = [0; 16];
        for (i, byte) in n1.iter_mut().enumerate() {
            *byte = 0x10 + u8::try_from(i).unwrap();
        }
        let mut n2 = [0; 16];
        for (i, byte) in n2.iter_mut().enumerate() {
            *byte = 0x30 + u8::try_from(i).unwrap();
        }
        let lines = include_str!("../../em-crypto/tests/data/upstream_crypto_vectors.tsv");
        let row = lines
            .lines()
            .find(|line| line.starts_with("wsc\tdh-secret\t"))
            .unwrap();
        let values: Vec<_> = row.split('\t').collect();
        let parts: Vec<_> = values[2].split(';').collect();
        let secret = unhex(parts[0].strip_prefix("private=").unwrap());
        let peer = unhex(parts[1].strip_prefix("peer=").unwrap());
        let private = DhPrivate::from_be_bytes(&secret).unwrap();
        let keys = derive_keys(
            &private,
            &peer,
            &n1,
            MacAddr::new([0x20, 0x21, 0x22, 0x23, 0x24, 0x25]),
            &n2,
        )
        .unwrap();
        assert_eq!(
            keys.auth.expose().as_slice(),
            unhex("f1e359bf600178aeea97fed7873d3a7990ed194323aa48b67a887e75bce091d4")
        );
        assert_eq!(
            keys.wrap.expose().as_slice(),
            unhex("388326303403775d846739bdf64c1eae")
        );
        assert_eq!(
            keys.emsk.expose().as_slice(),
            unhex("ca94dff32f087de3788b92cf2abb0aa886a960baf480d8fea4aead2e064599ef")
        );
    }
}
