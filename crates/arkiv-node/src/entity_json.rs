//! Entities as the JSON APIs render them: the `arkiv_query` value encoding
//! and the proof-verified entity of the light nodes.

use crate::{
    arkiv,
    error::{ErrorCode, Result},
    text::hex,
};
use alloy_primitives::U256;
use arkiv_db::{Row, Value};
use serde_json::{Value as Json, json};

/// The SDK type name and JSON value of an attribute.
pub(crate) fn scalar(value: &Value) -> Result<(&'static str, Json)> {
    Ok(match value {
        Value::Bool(v) => ("bool", json!(v)),
        Value::I32(v) => ("i32", json!(v)),
        Value::U64(v) => ("u64", json!(format!("0x{v:x}"))),
        Value::U256(v) => ("u256", json!(format!("0x{:x}", U256::from_be_bytes(*v)))),
        Value::Dec(v) => ("dec", json!(arkiv::format_decimal(v))),
        Value::Bytes32(v) => ("bytes32", json!(hex(v))),
        Value::Bytes(v) => ("bytes", json!(hex(v))),
        Value::Str(v) => ("str", json!(v.as_ref())),
        Value::Addr(v) => ("addr", json!(hex(v))),
        Value::Key(v) => ("key", json!(hex(v))),
        // i64 and the golem-grid scalars are not SDK types.
        _ => return Err(ErrorCode::InvalidEntity.into()),
    })
}

/// Every user attribute of an entity, indexed or not, as
/// `{name, type, indexed, value}`.
fn attributes(e: &arkiv::ArkivEntityProjection) -> Result<Json> {
    e.attributes
        .iter()
        .map(|(n, c)| {
            scalar(&c.value).map(
                |(t, v)| json!({"name":n.as_str(),"type":t,"indexed":c.is_attribute(),"value":v}),
            )
        })
        .collect::<Result<Vec<_>>>()
        .map(Json::Array)
}

/// The entity in a proven `row`, which must be live at `height`: a row from
/// the future or past its expiry does not belong to that state.
pub(crate) fn verified(row: &Row, height: u64) -> Result<Json> {
    let p = arkiv::project(row).map_err(|_| ErrorCode::InvalidProof)?;
    if p.created_at > height
        || p.updated_at > height
        || (p.expires_at != u64::MAX && p.expires_at <= height)
    {
        return Err(ErrorCode::InvalidProof.into());
    }
    Ok(json!({
        "key": hex(&p.key),
        "owner": hex(&p.owner),
        "creator": hex(&p.creator),
        "creationFlags": {
            "raw": p.creation_flags,
            "readonly": p.creation_flags & 1 != 0,
            "permissionlessExtension": p.creation_flags & 2 != 0,
        },
        "createdAt": p.created_at.to_string(),
        "updatedAt": p.updated_at.to_string(),
        "expiresAt": p.expires_at.to_string(),
        "attributes": attributes(&p)?,
    }))
}
