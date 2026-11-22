// SPDX-License-Identifier: Apache-2.0

//! S1 AP-Autoconfiguration Search and Response builders, routing, and handlers, and the
//! S2 Renew builder and reader.
//! All CMDUs start at the 1905 header; the transport supplies the remote AL MAC.

use std::fmt;

use em_cmd::{Command, CommandType, NodeKey, NodeKind, OrchState};
use em_msg::bodies::{
    AlMacAddress, AutoconfigFreqBand, ControllerCapability, FrequencyBand,
    Ieee1905LayerSecurityCapability, MultiApProfile, Role, RoleValue, SearchedRole,
    SearchedService, ServiceList, SupportedFreqBand, SupportedRole, SupportedService,
};
use em_msg::{
    BuildError, CmduHeader, FramingError, MsgBuilder, RAW_BUDGET, ReaderError, TlvBody,
    ValidateError, readers, validate,
};
use em_types::{FreqBand, MacAddr, MsgType, ProfileType, ServiceType, TlvType};

use crate::{AgentState, ControllerPeer, CtrlState, EngineCtx, NodeTable, Output};

/// Failure to process a discovery message or construct an output.
#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    /// The node key is absent or is not the expected kind.
    InvalidNode,
    /// A radio has no band.
    MissingBand,
    /// A required routing TLV is absent.
    MissingTlv(TlvType),
    /// The CMDU type is not the one expected by this entry point.
    UnexpectedMessageType(MsgType),
    /// CMDU framing failed.
    Framing(FramingError),
    /// A decoded TLV is malformed.
    Reader(ReaderError),
    /// A built message could not be encoded.
    Build(BuildError),
    /// An incoming message or outgoing Search failed the validation table.
    Validation(ValidateError),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
impl From<FramingError> for Error {
    fn from(error: FramingError) -> Self {
        Self::Framing(error)
    }
}
impl From<ReaderError> for Error {
    fn from(error: ReaderError) -> Self {
        Self::Reader(error)
    }
}
impl From<BuildError> for Error {
    fn from(error: BuildError) -> Self {
        Self::Build(error)
    }
}
impl From<ValidateError> for Error {
    fn from(error: ValidateError) -> Self {
        Self::Validation(error)
    }
}

fn header(msg_type: MsgType, msg_id: u16, relay: bool) -> CmduHeader {
    CmduHeader {
        msg_type,
        msg_id,
        frag_id: 0,
        last_fragment: true,
        relay,
    }
}

/// Builds the Search in upstream TLV order (`em_configuration.cpp:4542-4591`).
///
/// The caller supplies the process-wide MID. DPP Chirp and vendor extensions
/// are outside S1. The message is addressed separately to IEEE 1905 multicast.
///
/// # Errors
///
/// Returns an encoding or message-budget error.
pub fn build_search(
    al_mac: MacAddr,
    band: FreqBand,
    profile: u8,
    msg_id: u16,
) -> Result<Vec<u8>, BuildError> {
    let mut builder = MsgBuilder::new(
        &header(MsgType::AP_AUTOCONFIGURATION_SEARCH, msg_id, true),
        RAW_BUDGET,
    );
    builder
        .body(&AlMacAddress { mac: al_mac })
        .body(&SearchedRole(RoleValue {
            role: Role::REGISTRAR,
        }))
        .body(&AutoconfigFreqBand(FrequencyBand { band }))
        .body(&SupportedService(ServiceList {
            services: vec![ServiceType::MULTI_AP_AGENT],
        }))
        .body(&SearchedService(ServiceList {
            services: vec![ServiceType::MULTI_AP_CONTROLLER],
        }))
        .body(&MultiApProfile {
            profile: ProfileType::from(profile),
        });
    builder.finish()
}

/// Builds the Response in upstream TLV order (`em_configuration.cpp:4361-4498`).
///
/// The profile is the current Search's effective profile. The Controller
/// Capability sets Early AP Capability, as upstream does (`em_configuration.cpp:4458`),
/// and the `KiBMiB` Counter bit, which upstream leaves unset (note 0012 table).
/// Security capability values are zero in S1. This builder uses the
/// controller-only service list for a non-EM+ peer.
///
/// # Errors
///
/// Returns an encoding or message-budget error.
pub fn build_response(band: FreqBand, profile: u8, msg_id: u16) -> Result<Vec<u8>, BuildError> {
    build_response_for_peer(band, profile, msg_id, false)
}

fn build_response_for_peer(
    band: FreqBand,
    profile: u8,
    msg_id: u16,
    is_emplus_agent: bool,
) -> Result<Vec<u8>, BuildError> {
    let mut services = vec![ServiceType::MULTI_AP_CONTROLLER];
    if is_emplus_agent {
        services.push(ServiceType::EMPLUS_CONTROLLER);
    }
    let mut builder = MsgBuilder::new(
        &header(MsgType::AP_AUTOCONFIGURATION_RESPONSE, msg_id, false),
        RAW_BUDGET,
    );
    builder
        .body(&SupportedRole(RoleValue {
            role: Role::REGISTRAR,
        }))
        .body(&SupportedFreqBand(FrequencyBand { band }))
        .body(&SupportedService(ServiceList { services }))
        .body(&Ieee1905LayerSecurityCapability {
            onboarding_protocol: 0,
            mic_algorithm: 0,
            encryption_algorithm: 0,
        })
        .body(&MultiApProfile {
            profile: ProfileType::from(profile),
        })
        .body(&ControllerCapability {
            kib_mib_counter: true,
            early_ap_capability: true,
        });
    builder.finish()
}

/// Builds the controller's AP-Autoconfiguration Renew (`em_configuration.cpp:850-926`).
///
/// It carries the controller's AL MAC, Supported Role Registrar and the radio's band, and is
/// unicast, so the relay indicator is clear.
///
/// # Errors
///
/// Returns an encoding or message-budget error.
pub fn build_renew(
    controller_al_mac: MacAddr,
    band: FreqBand,
    msg_id: u16,
) -> Result<Vec<u8>, BuildError> {
    let mut builder = MsgBuilder::new(
        &header(MsgType::AP_AUTOCONFIGURATION_RENEW, msg_id, false),
        RAW_BUDGET,
    );
    builder
        .body(&AlMacAddress {
            mac: controller_al_mac,
        })
        .body(&SupportedRole(RoleValue {
            role: Role::REGISTRAR,
        }))
        .body(&SupportedFreqBand(FrequencyBand { band }));
    builder.finish()
}

/// What an agent needs from a Renew to pick its radio (`src/agent/em_agent.cpp:2400-2420`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenewRoute {
    /// The AL MAC TLV, which names the controller.
    pub al_mac: MacAddr,
    /// The band of the radio to renew.
    pub band: FreqBand,
}

/// Reads a Renew's band and AL MAC and validates it at profile 2
/// (`em_configuration.cpp:6486-6497`, `em_agent.cpp:2400-2420`).
///
/// # Errors
///
/// Returns an error for a malformed header, a missing or unreadable band or AL MAC, or a
/// failed validation.
pub fn route_renew(cmdu: &[u8]) -> Result<RenewRoute, Error> {
    let (_, area) = message_area(cmdu, MsgType::AP_AUTOCONFIGURATION_RENEW)?;
    validate(cmdu, ProfileType::MULTI_AP_PROFILE_2)?;
    let band = readers::freq_band(area)?.ok_or(Error::MissingTlv(TlvType::SUPPORTED_FREQ_BAND))?;
    let al_mac =
        readers::al_mac_address(area)?.ok_or(Error::MissingTlv(TlvType::AL_MAC_ADDRESS))?;
    Ok(RenewRoute { al_mac, band })
}

fn message_area(cmdu: &[u8], expected: MsgType) -> Result<(CmduHeader, &[u8]), Error> {
    let (header, area) = CmduHeader::parse(cmdu)?;
    if header.msg_type != expected {
        return Err(Error::UnexpectedMessageType(header.msg_type));
    }
    Ok((header, area))
}

// Preserve reserved octets for the handler's S1 fallback while enforcing the
// one- and four-octet layouts accepted by upstream (em_msg.cpp:131-152).
fn raw_profile(area: &[u8]) -> Result<Option<u8>, ReaderError> {
    let Some(value) = readers::find_tlv(area, TlvType::MULTI_AP_PROFILE)? else {
        return Ok(None);
    };
    match value {
        &[raw] | &[raw, _, _, _] => Ok(Some(raw)),
        _ => Err(ReaderError::InvalidLength {
            tlv_type: TlvType::MULTI_AP_PROFILE,
            len: value.len(),
        }),
    }
}

fn effective_profile(raw: Option<u8>, own: u8) -> u8 {
    match raw {
        None => 1,
        Some(value @ 1..=3) => value,
        Some(_) => own,
    }
}

/// Details extracted from a Search before the configuration handler validates it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchRoute {
    /// Search AL MAC; the response is unicast to this address.
    pub al_mac: MacAddr,
    /// Frequency band from the Search.
    pub band: FreqBand,
    /// Profile 0 through 3; one if absent, malformed, or out of range.
    pub profile: u8,
    /// Whether `SupportedService` names the vendor EM+ agent.
    pub is_emplus_agent: bool,
}

/// Extracts controller routing/model inputs before full validation, as in
/// `src/ctrl/em_ctrl.cpp:1197-1238`. An unreadable optional service or profile
/// uses the upstream fallback. Profiles 4 and above also fall back to 1
/// (`src/em/em_msg.cpp:131-152`, `src/ctrl/em_ctrl.cpp:1219-1222`), while 0
/// is preserved. The first Supported or Autoconfig Frequency Band TLV supplies
/// the band, as in upstream `get_freq_band`.
///
/// # Errors
///
/// Returns an error for a missing or malformed header, band, or AL MAC.
pub fn route_search(cmdu: &[u8]) -> Result<SearchRoute, Error> {
    let (_, area) = message_area(cmdu, MsgType::AP_AUTOCONFIGURATION_SEARCH)?;
    let band = readers::freq_band(area)?.ok_or(Error::MissingTlv(TlvType::AUTOCONFIG_FREQ_BAND))?;
    let al_mac =
        readers::al_mac_address(area)?.ok_or(Error::MissingTlv(TlvType::AL_MAC_ADDRESS))?;
    let services = readers::supported_services(area).ok().flatten();
    let is_emplus_agent =
        services.is_some_and(|list| list.contains(&u8::from(ServiceType::EMPLUS_AGENT)));
    Ok(SearchRoute {
        al_mac,
        band,
        profile: readers::profile(area).ok().flatten().map_or(1, u8::from),
        is_emplus_agent,
    })
}

/// Selects the first radio in insertion order matching a Response's band and
/// not in an excluded upstream state (`src/agent/em_agent.cpp:2366-2396`).
/// The first Supported or Autoconfig Frequency Band TLV supplies the band
/// (`src/em/em_msg.cpp:346-363`). Selection precedes full Response validation.
///
/// # Errors
///
/// Returns an error if the header is malformed, or if the frequency band TLV is
/// missing or unreadable.
pub fn route_response(
    nodes: &NodeTable<AgentState>,
    cmdu: &[u8],
) -> Result<Option<NodeKey>, Error> {
    let (_, area) = message_area(cmdu, MsgType::AP_AUTOCONFIGURATION_RESPONSE)?;
    // Preserve the router's framing-error variant when using the shared reader.
    let band = readers::freq_band(area)
        .map_err(|error| match error {
            ReaderError::Framing(error) => Error::Framing(error),
            other => Error::Reader(other),
        })?
        .ok_or(Error::MissingTlv(TlvType::SUPPORTED_FREQ_BAND))?;
    Ok(nodes
        .iter()
        .find(|node| {
            node.key.kind == NodeKind::Radio
                && node.band == Some(band)
                && !matches!(
                    node.state,
                    AgentState::AutoconfigRenewPending
                        | AgentState::WscM2Pending
                        | AgentState::OwconfigPending
                        | AgentState::Ieee1905Securing
                        | AgentState::WscM2Processed
                )
        })
        .map(|node| node.key))
}

/// Whether upstream's dev-init gate permits an agent node's proto tick
/// (`src/em/em.cpp:450-468`). Configuration Renew is outside S1.
#[must_use]
pub fn search_gate(ctx: &EngineCtx<'_, AgentState>) -> bool {
    let Some(node) = ctx.nodes.get(ctx.key) else {
        return false;
    };
    if node.key.kind != NodeKind::Radio
        || node.state >= AgentState::Configured
        || ctx.commands.orch_state(ctx.key) != OrchState::Progress
    {
        return false;
    }
    let Some(active) = ctx.commands.active(ctx.key) else {
        return false;
    };
    active.kind == CommandType::DevInit
        && matches!(
            ctx.commands.command(active.chain),
            Some(Command::DevInit(_))
        )
}

/// Handles an agent proto tick; a Search is built on each eligible tick in
/// Unconfigured or Response-pending state (`em_configuration.cpp:6695-6737,6770-6774`).
///
/// # Errors
///
/// Returns a message encoding or validation error, or a missing band.
pub fn agent_proto_tick(ctx: &mut EngineCtx<'_, AgentState>) -> Result<(), Error> {
    if !search_gate(ctx) {
        return Ok(());
    }
    let node = ctx.nodes.get(ctx.key).ok_or(Error::InvalidNode)?;
    if !matches!(
        node.state,
        AgentState::Unconfigured | AgentState::AutoconfigRspPending
    ) {
        return Ok(());
    }
    let Some(active) = ctx.commands.active(ctx.key) else {
        return Ok(());
    };
    let Some(Command::DevInit(command)) = ctx.commands.command(active.chain) else {
        return Ok(());
    };
    let al_mac = command.al_mac;
    let band = node.band.ok_or(Error::MissingBand)?;
    if em_easyconnect::onboarding_in_progress(al_mac) {
        return Ok(());
    }
    let profile = node.profile;
    let id = ctx.msg_ids.next_id();
    let cmdu = build_search(al_mac, band, profile, id)?;
    validate(&cmdu, ProfileType::MULTI_AP_PROFILE_3)?;
    ctx.out.events.push(Output::Frame {
        remote: MacAddr::IEEE1905_MULTICAST,
        cmdu,
    });
    let node = ctx.nodes.get_mut(ctx.key).ok_or(Error::InvalidNode)?;
    if node.state != AgentState::AutoconfigRspPending {
        node.state = AgentState::AutoconfigRspPending;
        ctx.out.events.push(Output::Progress(ctx.key));
    }
    tracing::info!(
        ?band,
        profile,
        mid = id,
        "autoconfiguration Search handed off"
    );
    Ok(())
}

/// Handles a Response for an agent radio. The MID is deliberately not matched:
/// upstream accepts any MID (`em_configuration.cpp:6284-6391`). It records the
/// controller and enters WSC M2 pending. It sends no M1: the agent core sends M1 once
/// the radio has moved (0013 D4). A Response that arrives in any other state is ignored.
///
/// # Errors
///
/// Returns an error for an invalid node, framing, malformed Profile TLV, or
/// validation failure.
pub fn handle_response(
    ctx: &mut EngineCtx<'_, AgentState>,
    remote: MacAddr,
    cmdu: &[u8],
) -> Result<(), Error> {
    let node = ctx.nodes.get(ctx.key).ok_or(Error::InvalidNode)?;
    if node.key.kind != NodeKind::Radio {
        return Err(Error::InvalidNode);
    }
    if node.state != AgentState::AutoconfigRspPending {
        return Ok(());
    }
    let (_, area) = message_area(cmdu, MsgType::AP_AUTOCONFIGURATION_RESPONSE)?;
    let raw = match raw_profile(area) {
        // A framing error is left for `validate` to report.
        Err(ReaderError::Framing(_)) => None,
        result => result?,
    };
    let profile = effective_profile(raw, node.profile);
    validate(cmdu, ProfileType::from(profile))?;
    let capability = readers::find_tlv(area, TlvType::CONTROLLER_CAPABILITY)?
        .and_then(|value| ControllerCapability::decode_value(value).ok());
    tracing::info!(controller = %remote, profile, ?capability, "controller found");
    let node = ctx.nodes.get_mut(ctx.key).ok_or(Error::InvalidNode)?;
    node.controller = Some(ControllerPeer {
        al_mac: remote,
        profile,
    });
    node.state = AgentState::WscM2Pending;
    ctx.out.events.push(Output::Progress(ctx.key));
    Ok(())
}

/// Validates a routed Search, sends a Response to its AL MAC, and advances the
/// controller AL node (`em_configuration.cpp:6393-6484`). Every valid Search
/// is answered, including repeats while awaiting M1.
///
/// # Errors
///
/// Returns an error for an invalid node, malformed Search, failed validation,
/// or inability to build the Response. The Response's own validation failure
/// is logged but does not prevent handoff, as upstream does.
pub fn handle_search(ctx: &mut EngineCtx<'_, CtrlState>, cmdu: &[u8]) -> Result<(), Error> {
    if ctx.key.kind != NodeKind::Al || ctx.nodes.get(ctx.key).is_none() {
        return Err(Error::InvalidNode);
    }
    let (header, area) = message_area(cmdu, MsgType::AP_AUTOCONFIGURATION_SEARCH)?;
    let profile = effective_profile(raw_profile(area).ok().flatten(), 3);
    validate(cmdu, ProfileType::from(profile))?;
    let route = route_search(cmdu)?;
    let response =
        build_response_for_peer(route.band, profile, header.msg_id, route.is_emplus_agent)?;
    if let Err(error) = validate(&response, ProfileType::from(profile)) {
        tracing::error!(%error, "autoconfiguration Response failed validation; handing off anyway");
    }
    ctx.out.events.push(Output::Frame {
        remote: route.al_mac,
        cmdu: response,
    });
    let node = ctx.nodes.get_mut(ctx.key).ok_or(Error::InvalidNode)?;
    if node.state != CtrlState::WscM1Pending {
        node.state = CtrlState::WscM1Pending;
        ctx.out.events.push(Output::Progress(ctx.key));
    }
    tracing::info!(agent = %route.al_mac, band = ?route.band, profile, mid = header.msg_id, "autoconfiguration Response handed off");
    Ok(())
}
