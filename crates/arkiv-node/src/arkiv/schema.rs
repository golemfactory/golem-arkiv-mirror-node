//! Public adapter: SDK IDs are deliberately independent of engine tags.

use super::{ArkivFault, FLAG_PERMISSIONLESS_EXTENSION, FLAG_READONLY};
use crate::value_json::{parse_scaled, scaled_text};
use arkiv_db::{Cell, CellKind, CellName, Row, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ArkivTypeId {
    Bool = 1,
    I32 = 2,
    U64 = 3,
    U256 = 4,
    Dec = 5,
    Bytes32 = 6,
    Bytes = 7,
    Str = 8,
    Addr = 9,
    Key = 10,
}

impl ArkivTypeId {
    /// The type an ABI `typeId` names, without its indexed flag.
    pub fn from_id(id: u8) -> Option<Self> {
        [
            Self::Bool,
            Self::I32,
            Self::U64,
            Self::U256,
            Self::Dec,
            Self::Bytes32,
            Self::Bytes,
            Self::Str,
            Self::Addr,
            Self::Key,
        ]
        .into_iter()
        .find(|known| *known as u8 == id)
    }

    pub fn of(value: &Value) -> Result<Self, ArkivFault> {
        Ok(match value {
            Value::Bool(_) => Self::Bool,
            Value::I32(_) => Self::I32,
            Value::U64(_) => Self::U64,
            Value::U256(_) => Self::U256,
            Value::Dec(_) => Self::Dec,
            Value::Bytes32(_) => Self::Bytes32,
            Value::Bytes(_) => Self::Bytes,
            Value::Str(_) => Self::Str,
            Value::Addr(_) => Self::Addr,
            Value::Key(_) => Self::Key,
            // i64 and the golem-grid scalars are not SDK types.
            _ => return Err(ArkivFault::Value),
        })
    }
}

pub fn protocol_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 32
        && name.as_bytes()[0].is_ascii_alphabetic()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}

pub fn sdk_safe_name(name: &str) -> bool {
    protocol_name(name)
        && !name.contains("--")
        && ![
            "and",
            "or",
            "not",
            "true",
            "false",
            "startswith",
            "exists",
            "typeof",
            "bool",
            "i32",
            "u64",
            "u256",
            "dec",
            "bytes32",
            "bytes",
            "str",
            "addr",
            "key",
        ]
        .contains(&name.to_ascii_lowercase().as_str())
}

/// Bit 7 of an ABI `typeId`: the attribute is indexed. The low seven bits
/// are the [`ArkivTypeId`].
pub const INDEXED_FLAG: u8 = 0x80;
/// Every creation flag an entity may hold.
pub(super) const ALL_FLAGS: u64 = (FLAG_READONLY | FLAG_PERMISSIONLESS_EXTENSION) as u64;
/// The most attribute triples one create or patch may carry.
pub const MAX_ATTRIBUTES: usize = 32;
/// The host cells every entity carries: `$key`, `$creator`, `$owner`,
/// `$createdAt`, `$updatedAt`, `$expiresAt` and `$creationFlags`.
pub const SYSTEM_CELLS: usize = 7;
/// The fewest engine cells per record a chain serving the SDK needs, so that
/// [`MAX_ATTRIBUTES`] user attributes always fit beside the system cells.
pub const MIN_ENTITY_CELLS: usize = SYSTEM_CELLS + MAX_ATTRIBUTES;
/// The largest `str` value, in UTF-8 bytes.
pub const MAX_STR_BYTES: usize = 128;
/// The largest single `bytes` value.
pub const MAX_BYTES_LENGTH: usize = 128 * 1024;
/// The most data the `bytes` cells of one entity may hold together, after a
/// create or patch (the SDK's `MAX_ENTITY_BYTES`).
pub const MAX_ENTITY_BYTES: usize = 128 * 1024;

pub fn validate_value(value: &Value) -> Result<(), ArkivFault> {
    ArkivTypeId::of(value)?;
    match value {
        // Any UTF-8 within the length cap, control characters included, as
        // on the Arkiv network.
        Value::Str(text) if text.len() > MAX_STR_BYTES => Err(ArkivFault::Value),
        Value::Bytes(bytes) if bytes.len() > MAX_BYTES_LENGTH => Err(ArkivFault::Value),
        _ => Ok(()),
    }
}

/// A user attribute as written: an SDK-safe name and an SDK value, indexed
/// ([`CellKind::Attribute`]) or not ([`CellKind::Field`]). `bytes` is stored
/// but never indexed. `$` names belong to the host.
pub fn validate_user_cell(name: &CellName, cell: &Cell) -> Result<(), ArkivFault> {
    validate_value(&cell.value)?;
    if !sdk_safe_name(name.as_str()) {
        return Err(ArkivFault::Name);
    }
    if cell.is_attribute() && matches!(cell.value, Value::Bytes(_)) {
        return Err(ArkivFault::Value);
    }
    Ok(())
}

pub fn validate_remove(name: &CellName) -> Result<(), ArkivFault> {
    if sdk_safe_name(name.as_str()) {
        Ok(())
    } else {
        Err(ArkivFault::Name)
    }
}

/// The bytes held by the `bytes` values among `cells`.
pub fn entity_bytes<'a>(cells: impl IntoIterator<Item = &'a (CellName, Cell)>) -> usize {
    cells
        .into_iter()
        .map(|(_, cell)| match &cell.value {
            Value::Bytes(b) => b.len(),
            _ => 0,
        })
        .sum()
}

/// Fractional digits of a `dec` value.
const DEC_SCALE: u8 = 18;

/// Parse decimal scaled units without floating point or rounding. Unlike the
/// JSON form ([`parse_scaled`]) a leading `+` is accepted.
pub fn parse_decimal(input: &str) -> Result<[u8; 32], ArkivFault> {
    let unsigned = match input.strip_prefix('+') {
        Some(rest) if rest.starts_with('-') => return Err(ArkivFault::Value),
        Some(rest) => rest,
        None => input,
    };
    parse_scaled(unsigned, DEC_SCALE, 256).ok_or(ArkivFault::Value)
}

pub fn format_decimal(bytes: &[u8; 32]) -> String {
    scaled_text(*bytes, DEC_SCALE)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArkivEntityProjection {
    pub key: [u8; 32],
    pub creator: [u8; 20],
    pub owner: [u8; 20],
    pub created_at: u64,
    pub updated_at: u64,
    pub expires_at: u64,
    pub creation_flags: u8,
    /// User attributes in name order; the cell kind says whether each is
    /// indexed ([`CellKind::Attribute`]) or only stored ([`CellKind::Field`]).
    pub attributes: Vec<(CellName, Cell)>,
}

impl ArkivEntityProjection {
    /// The user attribute `name`, if the entity holds one.
    pub fn attribute(&self, name: &str) -> Option<&Cell> {
        self.attributes
            .iter()
            .find(|(n, _)| n.as_str() == name)
            .map(|(_, c)| c)
    }
}

pub fn project(row: &Row) -> Result<ArkivEntityProjection, ArkivFault> {
    let structural: [u8; 32] = row
        .key
        .as_ref()
        .ok_or(ArkivFault::Corrupt)?
        .as_bytes()
        .try_into()
        .map_err(|_| ArkivFault::Corrupt)?;
    if row.cells.windows(2).any(|pair| pair[0].0 >= pair[1].0) {
        return Err(ArkivFault::Corrupt);
    }
    for (name, cell) in &row.cells {
        validate_value(&cell.value).map_err(|_| ArkivFault::Corrupt)?;
        let system = |valid: bool, kind: CellKind| valid && cell.kind == kind;
        let expected = match name.as_str() {
            "$key" => system(
                matches!(cell.value, Value::Key(k) if k == structural),
                CellKind::Attribute,
            ),
            "$creator" | "$owner" => {
                system(matches!(cell.value, Value::Addr(_)), CellKind::Attribute)
            }
            "$createdAt" | "$updatedAt" | "$expiresAt" => {
                system(matches!(cell.value, Value::U64(_)), CellKind::Attribute)
            }
            "$creationFlags" => system(
                matches!(cell.value, Value::U64(flags) if flags <= ALL_FLAGS),
                CellKind::Field,
            ),
            // Users write indexed or unindexed attributes; `bytes` only unindexed.
            user => {
                protocol_name(user)
                    && !(cell.is_attribute() && matches!(cell.value, Value::Bytes(_)))
            }
        };
        if !expected {
            return Err(ArkivFault::Corrupt);
        }
    }
    if entity_bytes(&row.cells) > MAX_ENTITY_BYTES {
        return Err(ArkivFault::Corrupt);
    }
    let get = |wanted: &str| {
        row.cells
            .iter()
            .find(|(name, _)| name.as_str() == wanted)
            .map(|(_, cell)| &cell.value)
            .ok_or(ArkivFault::Corrupt)
    };
    let number = |name| match get(name)? {
        Value::U64(number) => Ok(*number),
        _ => Err(ArkivFault::Corrupt),
    };
    let address = |name| match get(name)? {
        Value::Addr(address) => Ok(*address),
        _ => Err(ArkivFault::Corrupt),
    };
    get("$key")?;
    let result = ArkivEntityProjection {
        key: structural,
        creator: address("$creator")?,
        owner: address("$owner")?,
        created_at: number("$createdAt")?,
        updated_at: number("$updatedAt")?,
        expires_at: number("$expiresAt")?,
        creation_flags: number("$creationFlags")? as u8,
        attributes: row
            .cells
            .iter()
            .filter(|(n, _)| !n.as_str().starts_with('$'))
            .cloned()
            .collect(),
    };
    if result.updated_at < result.created_at || result.expires_at <= result.created_at {
        return Err(ArkivFault::Corrupt);
    }
    Ok(result)
}

pub fn validate_proven_row(row: &Row) -> Result<(), ArkivFault> {
    project(row).map(|_| ())
}

/// SDK ABI values: fixed scalars occupy a canonical 32-byte Solidity word.
pub fn decode_abi_value(type_id: u8, bytes: &[u8]) -> Result<Value, ArkivFault> {
    let value = match ArkivTypeId::from_id(type_id).ok_or(ArkivFault::Value)? {
        ArkivTypeId::Bytes if bytes.len() <= MAX_BYTES_LENGTH => Value::bytes(bytes),
        ArkivTypeId::Str if bytes.len() <= MAX_STR_BYTES => {
            Value::str(std::str::from_utf8(bytes).map_err(|_| ArkivFault::Value)?)
        }
        ArkivTypeId::Bytes | ArkivTypeId::Str => return Err(ArkivFault::Value),
        scalar => {
            let word = bytes.try_into().map_err(|_| ArkivFault::Value)?;
            decode_abi_word(scalar, word).ok_or(ArkivFault::Value)?
        }
    };
    validate_value(&value)?;
    Ok(value)
}

/// The fixed scalar `word` encodes; `None` unless its padding is canonical.
fn decode_abi_word(type_id: ArkivTypeId, word: [u8; 32]) -> Option<Value> {
    let (padding, value) = match type_id {
        ArkivTypeId::Bool => word.split_at(31),
        ArkivTypeId::I32 => word.split_at(28),
        ArkivTypeId::U64 => word.split_at(24),
        ArkivTypeId::Addr => word.split_at(12),
        _ => word.split_at(0),
    };
    let zero_padded = padding.iter().all(|byte| *byte == 0);
    Some(match type_id {
        ArkivTypeId::Bool if zero_padded && value[0] <= 1 => Value::Bool(value[0] == 1),
        ArkivTypeId::I32 => {
            let number = i32::from_be_bytes(value.try_into().expect("width"));
            let sign = if number < 0 { 0xff } else { 0 };
            if padding.iter().any(|byte| *byte != sign) {
                return None;
            }
            Value::I32(number)
        }
        ArkivTypeId::U64 if zero_padded => {
            Value::U64(u64::from_be_bytes(value.try_into().expect("width")))
        }
        ArkivTypeId::U256 => Value::U256(word),
        ArkivTypeId::Dec => Value::Dec(word),
        ArkivTypeId::Bytes32 => Value::Bytes32(word),
        ArkivTypeId::Addr if zero_padded => Value::Addr(value.try_into().expect("width")),
        ArkivTypeId::Key => Value::Key(word),
        _ => return None,
    })
}

pub fn encode_abi_value(value: &Value) -> Result<Vec<u8>, ArkivFault> {
    validate_value(value)?;
    let mut word = [0; 32];
    match value {
        Value::Bool(flag) => word[31] = u8::from(*flag),
        Value::I32(number) => {
            word.fill(if *number < 0 { 0xff } else { 0 });
            word[28..].copy_from_slice(&number.to_be_bytes())
        }
        Value::U64(number) => word[24..].copy_from_slice(&number.to_be_bytes()),
        Value::Addr(address) => word[12..].copy_from_slice(address),
        Value::U256(fixed) | Value::Dec(fixed) | Value::Bytes32(fixed) | Value::Key(fixed) => {
            word = *fixed
        }
        Value::Str(text) => return Ok(text.as_bytes().to_vec()),
        Value::Bytes(bytes) => return Ok(bytes.to_vec()),
        // i64 and the golem-grid scalars are not SDK types.
        _ => return Err(ArkivFault::Value),
    }
    Ok(word.to_vec())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AbiAttribute {
    pub name: [u8; 32],
    pub type_id: u8,
    pub value: Vec<u8>,
}

/// Strict protocol triple validation; SDK-safe authoring is a separate policy.
///
/// The low seven bits of `typeId` are the [`ArkivTypeId`] and bit 7
/// ([`INDEXED_FLAG`]) asks for an index: a flagged triple becomes a
/// [`CellKind::Attribute`] cell, an unflagged one a [`CellKind::Field`].
/// `bytes` is never indexed, so `0x87` is refused. Type zero is accepted only
/// as an empty patch tombstone (`None`), never flagged and never stored.
pub fn decode_protocol_triples(
    triples: &[AbiAttribute],
    patch: bool,
) -> Result<Vec<(CellName, Option<Cell>)>, ArkivFault> {
    if triples.len() > MAX_ATTRIBUTES {
        return Err(ArkivFault::Value);
    }
    let mut previous = None;
    let mut result = Vec::new();
    for triple in triples {
        if previous.is_some_and(|name| name >= triple.name) {
            return Err(ArkivFault::Duplicate);
        }
        previous = Some(triple.name);
        let end = triple.name.iter().position(|b| *b == 0).unwrap_or(32);
        if triple.name[end..].iter().any(|b| *b != 0) {
            return Err(ArkivFault::Name);
        }
        let text = std::str::from_utf8(&triple.name[..end]).map_err(|_| ArkivFault::Name)?;
        if !protocol_name(text) {
            return Err(ArkivFault::Name);
        }
        let name = CellName::new(text).map_err(|_| ArkivFault::Name)?;
        let cell = if triple.type_id == 0 {
            if !patch || !triple.value.is_empty() {
                return Err(ArkivFault::Value);
            }
            None
        } else {
            let indexed = triple.type_id & INDEXED_FLAG != 0;
            let base = triple.type_id & !INDEXED_FLAG;
            // A flagged tombstone (0x80) is no type at all.
            if base == 0 {
                return Err(ArkivFault::Value);
            }
            let value = decode_abi_value(base, &triple.value)?;
            if indexed && matches!(value, Value::Bytes(_)) {
                return Err(ArkivFault::Value);
            }
            Some(if indexed {
                Cell::attribute(value)
            } else {
                Cell::field(value)
            })
        };
        result.push((name, cell));
    }
    Ok(result)
}
