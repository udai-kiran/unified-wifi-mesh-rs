// SPDX-License-Identifier: Apache-2.0

//! Load the controller's reset-file SSID configuration (0013 D2).
//!
//! The returned records are boxed for the `em-dm` ownership boundary. They are in file order;
//! no table or persistence is involved. See the crate README for the pinned upstream behavior
//! and the deliberate differences at the configuration boundary.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use em_dm::model::network::MAX_NET_SSIDS;
use em_dm::{
    BoundedVec, CStrBytes, HaulType, MacAddr, NetId, NetworkSsidInfo, Secret, StringError,
};
use serde::Deserialize;
use serde::de::{DeserializeOwned, MapAccess, Visitor};
use serde_json::value::RawValue;

/// Why a reset-file field cannot be represented or has an invalid shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldProblem {
    /// A required field is absent.
    Missing,
    /// A field has a different JSON type from the one shown here.
    WrongType {
        /// The expected JSON type.
        expected: &'static str,
    },
    /// The required SSID list is empty.
    Empty,
    /// An array has more entries than the model can store.
    TooMany {
        /// The number of entries in the input.
        actual: usize,
        /// The number of entries supported by the model.
        capacity: usize,
    },
    /// A string exceeds its model bound or contains an embedded NUL.
    String(StringError),
    /// A mobility domain is not a supported MAC address.
    InvalidMobilityDomain,
    /// A VLAN ID is not an integer in the range 0 through 65535.
    InvalidVlanId,
}

impl fmt::Display for FieldProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing => f.write_str("missing required field"),
            Self::WrongType { expected } => write!(f, "expected {expected}"),
            Self::Empty => f.write_str("must not be empty"),
            Self::TooMany { actual, capacity } => {
                write!(f, "{actual} entries exceed the capacity of {capacity}")
            }
            Self::String(error) => write!(f, "{error}"),
            Self::InvalidMobilityDomain => f.write_str("invalid mobility-domain MAC address"),
            Self::InvalidVlanId => f.write_str("expected an integer in the range 0..=65535"),
        }
    }
}

impl std::error::Error for FieldProblem {}

/// Failure to read or decode a reset file.
#[derive(Debug)]
pub enum ResetError {
    /// The reset file does not exist.
    MissingFile {
        /// Path that was requested.
        path: PathBuf,
        /// The underlying operating-system error.
        source: io::Error,
    },
    /// The reset file could not be read as UTF-8.
    ReadFile {
        /// Path that was requested.
        path: PathBuf,
        /// The underlying operating-system or UTF-8 error.
        source: io::Error,
    },
    /// The input is not valid JSON.
    Json(serde_json::Error),
    /// A field has an invalid value or does not fit the model.
    Field {
        /// Zero-based SSID row index, or `None` for the reset envelope.
        row: Option<usize>,
        /// The reset-file field name.
        field: &'static str,
        /// Why the field was rejected.
        problem: FieldProblem,
    },
}

impl fmt::Display for ResetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingFile { path, source } => {
                write!(f, "reset file {} is missing: {source}", path.display())
            }
            Self::ReadFile { path, source } => {
                write!(f, "cannot read reset file {}: {source}", path.display())
            }
            Self::Json(source) => write!(f, "invalid reset JSON: {source}"),
            Self::Field {
                row,
                field,
                problem,
            } => {
                if let Some(row) = row {
                    write!(f, "NetworkSSIDList[{row}].{field}: {problem}")
                } else {
                    write!(f, "{field}: {problem}")
                }
            }
        }
    }
}

impl std::error::Error for ResetError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::MissingFile { source, .. } | Self::ReadFile { source, .. } => Some(source),
            Self::Json(source) => Some(source),
            Self::Field { problem, .. } => Some(problem),
        }
    }
}

/// Read a reset file and return its SSID rows in file order.
///
/// # Errors
///
/// Returns [`ResetError`] for a missing or unreadable file, invalid JSON, an invalid field or
/// a value that exceeds an `em-dm` bound.
pub fn load_reset_file(path: impl AsRef<Path>) -> Result<Vec<Box<NetworkSsidInfo>>, ResetError> {
    let path = path.as_ref();
    let input = fs::read_to_string(path).map_err(|source| {
        if source.kind() == io::ErrorKind::NotFound {
            ResetError::MissingFile {
                path: path.to_path_buf(),
                source,
            }
        } else {
            ResetError::ReadFile {
                path: path.to_path_buf(),
                source,
            }
        }
    })?;
    load_reset_str(&input)
}

/// Decode reset JSON and return its SSID rows in file order.
///
/// # Errors
///
/// Returns [`ResetError`] for invalid JSON, an invalid field or a value that exceeds an
/// `em-dm` bound.
pub fn load_reset_str(input: &str) -> Result<Vec<Box<NetworkSsidInfo>>, ResetError> {
    // Validate the entire document before checking its shape. Otherwise a wrong-shaped prefix
    // such as `[` can mask malformed JSON as an object type error.
    // cJSON_Parse accepts an initial UTF-8 BOM (dm_easy_mesh.cpp:903-904).
    let input = input.strip_prefix('\u{feff}').unwrap_or(input);
    let json: Box<RawValue> = serde_json::from_str(input).map_err(ResetError::Json)?;
    let root: JsonObject = parse(&json, "$", None, "an object")?;

    // decode_config_reset selects this object and its NetworkSSIDList (dm_easy_mesh.cpp:862-864,
    // 894-912, 951-973). The network record is not applied (dm_easy_mesh_ctrl.cpp:10861-10867).
    let reset = required(&root, "wfa-dataelements:Reset", None)?;
    let network: JsonObject = parse(reset, "wfa-dataelements:Reset", None, "an object")?;
    let list = required(&network, "NetworkSSIDList", None)?;
    let rows: Vec<Box<RawValue>> = parse(list, "NetworkSSIDList", None, "an array")?;
    if rows.is_empty() {
        return Err(invalid(None, "NetworkSSIDList", FieldProblem::Empty));
    }
    check_capacity(rows.len(), MAX_NET_SSIDS, "NetworkSSIDList", None)?;
    rows.iter()
        .enumerate()
        .map(|(index, row)| decode_row(row, index))
        .collect()
}

// Preserve object member order and duplicates: cJSON_GetObjectItem finds the first key, without
// regard to ASCII case (dm_network_ssid.cpp:45-106). serde_json::Value's map would discard
// duplicate keys before the lookup. RawValue also lets us ignore network fields without parsing
// them as model fields.
#[derive(Deserialize)]
#[serde(transparent)]
struct JsonObject(#[serde(deserialize_with = "object_members")] Vec<(String, Box<RawValue>)>);

fn object_members<'de, D>(deserializer: D) -> Result<Vec<(String, Box<RawValue>)>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct ObjectVisitor;

    impl<'de> Visitor<'de> for ObjectVisitor {
        type Value = Vec<(String, Box<RawValue>)>;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("a JSON object")
        }

        fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
        where
            A: MapAccess<'de>,
        {
            let mut members = Vec::new();
            while let Some(member) = map.next_entry()? {
                members.push(member);
            }
            Ok(members)
        }
    }

    deserializer.deserialize_map(ObjectVisitor)
}

impl JsonObject {
    fn get(&self, name: &str) -> Option<&RawValue> {
        self.0.iter().find_map(|(key, value)| {
            // cJSON stores object names as C strings, which end at the first NUL.
            let key = key.split('\0').next().unwrap_or_default();
            key.eq_ignore_ascii_case(name).then_some(value.as_ref())
        })
    }
}

fn invalid(row: Option<usize>, field: &'static str, problem: FieldProblem) -> ResetError {
    ResetError::Field {
        row,
        field,
        problem,
    }
}

fn required<'a>(
    object: &'a JsonObject,
    field: &'static str,
    row: Option<usize>,
) -> Result<&'a RawValue, ResetError> {
    object
        .get(field)
        .ok_or_else(|| invalid(row, field, FieldProblem::Missing))
}

fn parse<T: DeserializeOwned>(
    value: &RawValue,
    field: &'static str,
    row: Option<usize>,
    expected: &'static str,
) -> Result<T, ResetError> {
    // The outer parse has already validated the complete JSON document. Do not include the
    // rejected value in an error: it may contain a passphrase.
    serde_json::from_str(value.get())
        .map_err(|_| invalid(row, field, FieldProblem::WrongType { expected }))
}

fn check_capacity(
    actual: usize,
    capacity: usize,
    field: &'static str,
    row: Option<usize>,
) -> Result<(), ResetError> {
    if actual > capacity {
        return Err(invalid(
            row,
            field,
            FieldProblem::TooMany { actual, capacity },
        ));
    }
    Ok(())
}

fn optional_string<const CAP: usize>(
    object: &JsonObject,
    field: &'static str,
    row: usize,
) -> Result<CStrBytes<CAP>, ResetError> {
    let Some(value) = object.get(field) else {
        return Ok(CStrBytes::default());
    };
    let text: String = parse(value, field, Some(row), "a string")?;
    CStrBytes::new(text.as_bytes())
        .map_err(|error| invalid(Some(row), field, FieldProblem::String(error)))
}

fn string_list<const COUNT: usize, const CAP: usize>(
    object: &JsonObject,
    field: &'static str,
    row: usize,
) -> Result<BoundedVec<CStrBytes<CAP>, COUNT>, ResetError> {
    let Some(value) = object.get(field) else {
        return Ok(BoundedVec::new());
    };
    let strings: Vec<String> = parse(value, field, Some(row), "an array of strings")?;
    check_capacity(strings.len(), COUNT, field, Some(row))?;
    let items = strings
        .iter()
        .map(|text| {
            CStrBytes::new(text.as_bytes())
                .map_err(|error| invalid(Some(row), field, FieldProblem::String(error)))
        })
        .collect::<Result<Vec<_>, _>>()?;
    BoundedVec::try_from_vec(items).map_err(|error| {
        invalid(
            Some(row),
            field,
            FieldProblem::TooMany {
                actual: error.requested,
                capacity: error.capacity,
            },
        )
    })
}

fn hauls(object: &JsonObject, row: usize) -> Result<BoundedVec<HaulType, 8>, ResetError> {
    let Some(value) = object.get("HaulType") else {
        return Ok(BoundedVec::new());
    };
    let strings: Vec<String> = parse(value, "HaulType", Some(row), "an array of strings")?;
    check_capacity(strings.len(), 8, "HaulType", Some(row))?;
    let mut items = Vec::with_capacity(strings.len());
    for value in &strings {
        if let Some(position) = value.as_bytes().iter().position(|byte| *byte == 0) {
            return Err(invalid(
                Some(row),
                "HaulType",
                FieldProblem::String(StringError::EmbeddedNul { position }),
            ));
        }
        // strncmp compares strlen(input) bytes, in this order: prefixes and the empty string
        // match, and unknown values become em_haul_type_max (dm_network_ssid.cpp:253-269).
        let haul = if "Fronthaul".starts_with(value) {
            HaulType::FRONTHAUL
        } else if "Backhaul".starts_with(value) {
            HaulType::BACKHAUL
        } else if "IoT".starts_with(value) {
            HaulType::IOT
        } else if "Configurator".starts_with(value) {
            HaulType::CONFIGURATOR
        } else if "Hotspot".starts_with(value) {
            HaulType::HOTSPOT
        } else {
            HaulType(5)
        };
        items.push(haul);
    }
    BoundedVec::try_from_vec(items).map_err(|error| {
        invalid(
            Some(row),
            "HaulType",
            FieldProblem::TooMany {
                actual: error.requested,
                capacity: error.capacity,
            },
        )
    })
}

fn haul_name(haul: HaulType) -> &'static str {
    // dm_network_ssid.cpp:222-247; the default zeroed slot is Fronthaul.
    match haul {
        HaulType::FRONTHAUL => "Fronthaul",
        HaulType::BACKHAUL => "Backhaul",
        HaulType::IOT => "IoT",
        HaulType::CONFIGURATOR => "Configurator",
        HaulType::HOTSPOT => "Hotspot",
        _ => "Unknown",
    }
}

fn decode_row(value: &RawValue, row: usize) -> Result<Box<NetworkSsidInfo>, ResetError> {
    let object: JsonObject = parse(value, "NetworkSSIDList", Some(row), "an object")?;
    let haul_type = hauls(&object, row)?;
    let first_haul = haul_type
        .as_slice()
        .first()
        .copied()
        .unwrap_or(HaulType::FRONTHAUL);

    // intentional-difference:reset-global-id: upstream uses the file's ID for the SSID key
    // (dm_easy_mesh.cpp:951-973; dm_network_ssid.cpp:108); D2 ignores that network entry and
    // attaches the rows to the bootstrapped GLOBAL_NET_ID (dm_easy_mesh_ctrl.cpp:10823-10824,
    // 10861-10867).
    let id = format!("{}@{}", haul_name(first_haul), NetId::GLOBAL);
    let id = CStrBytes::new(id.as_bytes())
        .map_err(|error| invalid(Some(row), "ID", FieldProblem::String(error)))?;

    let mobility_domain = if let Some(value) = object.get("MobilityDomain") {
        let text: String = parse(value, "MobilityDomain", Some(row), "a string")?;
        text.parse::<MacAddr>().map_err(|_| {
            invalid(
                Some(row),
                "MobilityDomain",
                FieldProblem::InvalidMobilityDomain,
            )
        })?
    } else {
        MacAddr::ZERO
    };
    let vlan_id = if let Some(value) = object.get("VLANID") {
        serde_json::from_str::<u16>(value.get())
            .map_err(|_| invalid(Some(row), "VLANID", FieldProblem::InvalidVlanId))?
    } else {
        0
    };
    let enable = object.get("Enable").map_or(Ok(false), |value| {
        parse(value, "Enable", Some(row), "a boolean")
    })?;
    let advertisement = object
        .get("AdvertisementEnabled")
        .map_or(Ok(false), |value| {
            parse(value, "AdvertisementEnabled", Some(row), "a boolean")
        })?;

    // A fresh upstream row is zeroed before decoding; Band and AKMsAllowed retain their string
    // elements and order, while HaulType is converted above (dm_network_ssid.cpp:45-106).
    Ok(Box::new(NetworkSsidInfo {
        id,
        ssid: optional_string(&object, "SSID", row)?,
        pass_phrase: Secret::new(optional_string(&object, "PassPhrase", row)?),
        band: string_list(&object, "Band", row)?,
        enable,
        akm: string_list(&object, "AKMsAllowed", row)?,
        suite_select: optional_string(&object, "SuiteSelector", row)?,
        advertisement,
        mfp: optional_string(&object, "MFPConfig", row)?,
        mobility_domain,
        haul_type,
        auth_type: optional_string(&object, "AuthType", row)?,
        vlan_id,
    }))
}
