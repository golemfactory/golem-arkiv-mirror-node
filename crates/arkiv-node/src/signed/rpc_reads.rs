//! Unverified SDK JSON-RPC reads, with bounded native index execution.
//!
//! Cursors retain native snapshot cursors in a bounded process-local cache. They
//! are not proofs and expire after restart or eviction; callers can restart a
//! query at its explicitly retained block height.
//!
//! `arkiv_query`, `arkiv_getEntity` and `arkiv_getEntityCount` are [planned](plan) under the node
//! lock, which checks heights and pins the snapshot, and [run](Planned::run)
//! after it is released (see [`super::reads`]). [`dispatch`] does both.
//!
//! All three evaluate with planned budgets ([`Budget::planned`]): each
//! AND-group is driven by its cheapest index source and counts use stored
//! posting sizes, so the clause order of a query does not decide its cost,
//! and the rows, cursors and counts are exactly the written evaluation's. Their
//! group and literal bounds are the genesis limits unless the node's
//! [`ReadLimits`] (`--read-max-groups`, `--read-max-literals`) raise them;
//! a query beyond them still takes the bounded scan fallback (pages) or is
//! refused (counts). Block execution evaluates no SDK query, so neither
//! choice reaches consensus.
//!
//! Entities follow the user-defined-columns model: every user attribute is
//! returned, indexed or not, but a predicate on a user name only ever
//! matches an indexed one ([`arkiv_db::CellKind::Attribute`]). Refused
//! queries answer with the SDK's code table ([`QueryErrorKind`]).
//!
//! # Order, offset and count
//!
//! Without `ORDER BY` a page lists its matches in record-id order, which is
//! creation order: by block, then transaction, then operation. An `offset`
//! first walks past that many matches reading ids only, then reads on from
//! the last of them ([`Snapshot::cursor_after`]); it costs the walk, metered
//! against the read budget like any other read.
//!
//! `ORDER BY name:type [ASC|DESC]` sorts by one attribute, read as one type
//! whether or not it is indexed; an entity without it, or holding another
//! type there, sorts below every value, and ties come in record-id order in
//! either direction. A sorted page is a fetch-and-sort, as upstream's: it
//! reads the sort cell (and the key) of every match at the pinned snapshot,
//! keeps the first `offset + limit` in order in a bounded heap, then reads the
//! `[offset, offset + limit)` window's rows in full. The walk is charged to the
//! read budget, so a match set too large to sort within it is refused as
//! `limits`. Sorted pages have no cursor: they page by `offset`, pinned with
//! `atBlock` for a consistent order, and a `cursor` beside `ORDER BY` is
//! refused. `select.count` adds the match set's size from the planned count
//! path.

use super::{Error, Result, host::Host, reads::ReadLimits};
use crate::{
    arkiv,
    simulator::{
        query_language::{
            ast::{AnnotKey, AttributeValue, BuiltIn, Query as Ast},
            error::{ParseError, ParseErrorKind},
            lexer::{self, SpannedToken, Token},
            parse,
        },
        wire::hex,
    },
};
use alloy_primitives::{U256, keccak256};
use arkiv_db::{
    Budget, CellName, Cursor, Limits, Literal, Projection, Query, QueryLimits, QueryPage, RecordId,
    RecordKey, Row, Snapshot, Term, TypeTag, Value,
};
use serde_json::{Value as Json, json};
use std::{
    cmp::Ordering,
    collections::{BTreeMap, BinaryHeap},
    ops::Bound,
    sync::{Mutex, OnceLock},
};

const READ_BUDGET: u64 = 25_000_000;
const RESPONSE_CAP: usize = 8 * 1024 * 1024;
/// The bytes a page's entities may take: the response cap less room for the
/// page envelope, its cursor and the JSON-RPC envelope around it.
const PAGE_BYTES: usize = RESPONSE_CAP - 4096;
const CURSOR_CAP: usize = 1024;
const GROUP_CAP: usize = 64;
const LITERAL_CAP: usize = 256;
const QUERY_BYTES: usize = 8192;
const MAX_DEPTH: usize = 32;
const MAX_PREDICATES: usize = 64;
/// The most rows the scan fallback examines for one request.
const SCAN_ROWS: usize = 4096;
/// The `select` keys, in response order. Matches the SDK's `RpcSelect`.
const FIELDS: &[&str] = &[
    "key",
    "owner",
    "creator",
    "createdAt",
    "updatedAt",
    "expiresAt",
    "creationFlags",
    "attributeSchema",
    "attributes",
];

/// What a node rejected an `arkiv_query` (or `arkiv_getEntity`, `arkiv_getEntityCount`) for:
/// the SDK's frozen code table (`src/query/errors.ts`, `QueryErrorKind`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueryErrorKind {
    /// The query string does not parse (`-32001`).
    Parse,
    /// A well-formed predicate is ill-typed: `STARTSWITH` on a type other
    /// than `str`, `BETWEEN` bounds of two types, an unknown tag (`-32002`).
    Type,
    /// A literal failed validation: an `i32` out of range, a bad EIP-55
    /// checksum, more than 18 decimal places (`-32003`).
    Literal,
    /// A node budget was exceeded: query length, predicate count, nesting
    /// depth, Boolean expansion, read budget or response size (`-32004`).
    Limits,
    /// The cursor expired, is malformed, or belongs to another query, block
    /// or selection (`-32005`).
    Cursor,
    /// The requested block is outside the range the node retains: above the
    /// head or below the state floor (`-32006`).
    Block,
}

impl QueryErrorKind {
    /// The JSON-RPC error code.
    pub const fn code(self) -> i64 {
        match self {
            Self::Parse => -32001,
            Self::Type => -32002,
            Self::Literal => -32003,
            Self::Limits => -32004,
            Self::Cursor => -32005,
            Self::Block => -32006,
        }
    }

    /// The SDK's name for the kind.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Parse => "parse",
            Self::Type => "type",
            Self::Literal => "literal",
            Self::Limits => "limits",
            Self::Cursor => "cursor",
            Self::Block => "block",
        }
    }
}

/// A refused SDK query: its kind, a human-readable message and, for errors
/// at a known place in the query string, `data`: `{position, got}` plus
/// `expected` for a parse error. `position` is a UTF-8 byte offset.
#[derive(Clone, Debug, PartialEq)]
pub struct QueryError {
    pub kind: QueryErrorKind,
    pub message: String,
    pub data: Option<Json>,
}

impl QueryError {
    pub fn new(kind: QueryErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            data: None,
        }
    }

    /// An error at byte `position` of `source`.
    fn at(kind: QueryErrorKind, source: &str, position: usize, message: impl Into<String>) -> Self {
        let message = message.into();
        let position = position.min(source.len());
        let rest = source.get(position..).unwrap_or("");
        let got: String = rest
            .split_whitespace()
            .next()
            .map_or_else(|| "end of query".into(), |t| t.chars().take(32).collect());
        let mut data = json!({"position": position, "got": got});
        if kind == QueryErrorKind::Parse {
            data["expected"] = json!(message);
        }
        Self {
            kind,
            message,
            data: Some(data),
        }
    }

    /// The JSON-RPC `error` object.
    pub fn to_json(&self) -> Json {
        let mut error = json!({
            "code": self.kind.code(),
            "message": format!("query rejected ({}): {}", self.kind.name(), self.message),
        });
        if let Some(data) = &self.data {
            error["data"] = data.clone();
        }
        error
    }
}

/// Why an SDK read was refused: a query error in the SDK's table, or a node
/// error (invalid parameters, a pruned height, storage).
#[derive(Clone, Debug, PartialEq)]
pub enum ReadError {
    Query(QueryError),
    Node(Error),
}

impl From<Error> for ReadError {
    fn from(error: Error) -> Self {
        Self::Node(error)
    }
}

impl From<QueryError> for ReadError {
    fn from(error: QueryError) -> Self {
        Self::Query(error)
    }
}

type Read<T> = std::result::Result<T, ReadError>;

fn limits(message: &str) -> QueryError {
    QueryError::new(QueryErrorKind::Limits, message)
}

fn cursor_mismatch() -> ReadError {
    QueryError::new(
        QueryErrorKind::Cursor,
        "the cursor expired, is malformed, or belongs to another query, block or selection",
    )
    .into()
}

fn unknown_block(height: u64, head: u64) -> ReadError {
    QueryError::new(
        QueryErrorKind::Block,
        format!("block {height} is above the head {head}"),
    )
    .into()
}

/// A query's `ORDER BY`: one attribute, read as one type.
#[derive(Clone, Debug, PartialEq)]
struct Order {
    name: CellName,
    tag: TypeTag,
    descending: bool,
}

impl Order {
    /// The value `row` sorts by: its cell of that name, indexed or not, when
    /// it holds the sort's type.
    fn value(&self, row: &Row) -> Option<Value> {
        row.cells
            .iter()
            .find(|(name, _)| name == &self.name)
            .map(|(_, cell)| &cell.value)
            .filter(|value| value.tag() == self.tag)
            .cloned()
    }

    /// The order of two matches: by value, a missing one below every value
    /// and the whole comparison reversed by `DESC`, then by record id
    /// ascending in either direction, which makes the order total.
    fn cmp(&self, a: (Option<&Value>, RecordId), b: (Option<&Value>, RecordId)) -> Ordering {
        let by_value = match (a.0, b.0) {
            (None, None) => Ordering::Equal,
            (None, Some(_)) => Ordering::Less,
            (Some(_), None) => Ordering::Greater,
            (Some(x), Some(y)) => x.cmp_ordered(y).unwrap_or(Ordering::Equal),
        };
        let by_value = if self.descending {
            by_value.reverse()
        } else {
            by_value
        };
        by_value.then(a.1.cmp(&b.1))
    }
}

/// A parsed `arkiv_query` string: the filter and its optional order.
#[derive(Clone, Debug, PartialEq)]
struct Statement {
    query: Query,
    order: Option<Order>,
}

#[derive(Clone)]
struct CachedCursor {
    binding: [u8; 32],
    cursor: Cursor,
}

#[derive(Default)]
struct Cursors {
    serial: u64,
    entries: BTreeMap<String, CachedCursor>,
    order: std::collections::VecDeque<String>,
}

static CURSORS: OnceLock<Mutex<Cursors>> = OnceLock::new();

/// Plans and runs an SDK read under the default read limits, or `None` for
/// another method.
pub fn dispatch(host: &Host, method: &str, params: &Json) -> Option<Read<Json>> {
    plan(host, method, params, ReadLimits::default()).map(|planned| planned.and_then(Planned::run))
}

/// The methods whose evaluation runs after the node lock is released.
pub(super) fn detached(method: &str) -> bool {
    matches!(
        method,
        "arkiv_query" | "arkiv_getEntity" | "arkiv_getEntityCount"
    )
}

/// An SDK read with every check against the node done: what is left needs
/// only the pinned snapshot it holds.
pub enum Planned {
    Ready(Json),
    Count {
        snapshot: Snapshot,
        query: Query,
        bounds: QueryLimits,
    },
    Page(Box<PageRead>),
    /// `arkiv_getEntity`: a one-row page whose row, or `null`, is the result.
    Entity(Box<PageRead>),
}

pub struct PageRead {
    snapshot: Snapshot,
    query: Query,
    order: Option<Order>,
    limits: Limits,
    bounds: QueryLimits,
    limit: usize,
    /// Matches to skip before the page; zero on a cursor page.
    offset: u64,
    cursor: Option<Cursor>,
    selection: Selection,
    binding: [u8; 32],
    height: u64,
}

/// Validates an SDK read and pins its snapshot, or `None` for another method.
/// `reads` supplies the node's query bounds.
pub fn plan(host: &Host, method: &str, params: &Json, reads: ReadLimits) -> Option<Read<Planned>> {
    let bounds = reads.query_limits(&host.genesis.genesis.limits);
    Some(match method {
        "arkiv_query" => plan_query(host, params, bounds).map(|page| Planned::Page(Box::new(page))),
        "arkiv_getEntity" => {
            plan_entity(host, params, bounds).map(|page| Planned::Entity(Box::new(page)))
        }
        "arkiv_getEntityCount" => plan_count(host, params, bounds),
        "arkiv_getBlockTiming" => empty_params(params)
            .and_then(|()| {
                let current_block_time = host.head.timestamp_ms / 1000;
                let duration = if host.head.height == 0 {
                    0
                } else {
                    let previous = host.certificate(host.head.height - 1)?.header;
                    current_block_time.saturating_sub(previous.timestamp_ms / 1000)
                };
                Ok(Planned::Ready(json!({
                    "current_block": host.head.height,
                    "current_block_time": current_block_time,
                    "duration": duration,
                })))
            })
            .map_err(ReadError::from),
        _ => return None,
    })
}

impl Planned {
    /// Evaluates the read against its pinned snapshot. Its metered reads stop
    /// early when an enclosing [`arkiv_db::Interrupt`] is raised.
    pub fn run(self) -> Read<Json> {
        match self {
            Self::Ready(value) => Ok(value),
            Self::Count {
                snapshot,
                query,
                bounds,
            } => {
                let mut budget = READ_BUDGET;
                let n = count_matches(&snapshot, &query, bounds, &mut budget)?;
                if n > 9_007_199_254_740_991 {
                    return Err(limits("the count exceeds the JSON safe-integer range").into());
                }
                Ok(json!(n))
            }
            Self::Page(page) => page.run(),
            Self::Entity(page) => {
                let mut result = page.run()?;
                Ok(match result["data"].as_array_mut() {
                    Some(rows) if !rows.is_empty() => rows.swap_remove(0),
                    _ => Json::Null,
                })
            }
        }
    }
}

/// The budget of one native read: `units`, interruptible, planned, admitted
/// against `bounds`.
fn read_budget(units: u64, bounds: QueryLimits) -> Budget {
    Budget::limit(units)
        .interruptible()
        .planned()
        .with_query_limits(bounds)
}

fn exhausted() -> ReadError {
    limits("the read budget is exhausted").into()
}

/// Charges a read's spent units to the request's `budget`.
fn charge(budget: &mut u64, receipt: &arkiv_db::Receipt) -> Read<()> {
    *budget = budget
        .checked_sub(receipt.spent.units())
        .ok_or_else(exhausted)?;
    Ok(())
}

/// How many live entities match `query`, by the planned count (posting
/// sizes where they decide it), charged to `budget`.
fn count_matches(
    snapshot: &Snapshot,
    query: &Query,
    bounds: QueryLimits,
    budget: &mut u64,
) -> Read<u64> {
    match snapshot
        .try_count(
            arkiv::entity_namespace(),
            query,
            read_budget(*budget, bounds),
        )
        .map_err(Error::from)?
    {
        Ok(count) => {
            charge(budget, &count.receipt)?;
            Ok(count.value)
        }
        // Before the entity namespace exists there are no entities.
        Err(r) if r.reason == arkiv_db::Rejection::UnknownNamespace => Ok(0),
        Err(r) => Err(read_error(r)),
    }
}

/// One native page of entities, charged to `budget`, or `None` before the
/// entity namespace exists.
fn native_page(
    snapshot: &Snapshot,
    query: &Query,
    projection: &Projection,
    limit: usize,
    cursor: Option<&Cursor>,
    bounds: QueryLimits,
    budget: &mut u64,
) -> Read<Option<QueryPage>> {
    match snapshot
        .try_query(
            arkiv::entity_namespace(),
            query,
            projection,
            limit,
            cursor,
            read_budget(*budget, bounds),
        )
        .map_err(Error::from)?
    {
        Ok(metered) => {
            charge(budget, &metered.receipt)?;
            Ok(Some(metered.value))
        }
        Err(r) if r.reason == arkiv_db::Rejection::UnknownNamespace => Ok(None),
        Err(r) => Err(read_error(r)),
    }
}

fn plan_count(host: &Host, params: &Json, bounds: QueryLimits) -> Read<Planned> {
    let empty = json!({});
    let options = if params.is_null() {
        &empty
    } else {
        let args = params
            .as_array()
            .filter(|args| args.len() <= 1)
            .ok_or(Error("InvalidParams"))?;
        args.first().filter(|v| !v.is_null()).unwrap_or(&empty)
    };
    let options = options.as_object().ok_or(Error("InvalidParams"))?;
    let height = match options.get("block").filter(|v| !v.is_null()) {
        Some(value) => value.as_u64().ok_or(Error("InvalidParams"))?,
        None => host.head.height,
    };
    if height > host.head.height {
        return Err(unknown_block(height, host.head.height));
    }
    // A count has no order: an `ORDER BY` is read and set aside, so the
    // query string of a page counts as it is.
    let query = match options.get("query").filter(|v| !v.is_null()) {
        Some(value) => parse_query(value.as_str().ok_or(Error("InvalidParams"))?)?.query,
        None => Query::match_all(),
    };
    Ok(Planned::Count {
        snapshot: host.state(height)?,
        query,
        bounds,
    })
}

fn empty_params(params: &Json) -> Result<()> {
    if params.is_null() || params.as_array().is_some_and(Vec::is_empty) {
        Ok(())
    } else {
        Err(Error("InvalidParams"))
    }
}

fn quantity(value: &Json) -> Result<u64> {
    if let Some(n) = value.as_u64() {
        return Ok(n);
    }
    let s = value
        .as_str()
        .and_then(|s| s.strip_prefix("0x"))
        .ok_or(Error("InvalidParams"))?;
    if s.is_empty()
        || (s.len() > 1 && s.starts_with('0'))
        || !s.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err(Error("InvalidParams"));
    }
    u64::from_str_radix(s, 16).map_err(|_| Error("InvalidParams"))
}

fn read_error(error: arkiv_db::Rejected) -> ReadError {
    rejection(&error.reason)
}

/// An engine read rejection in the SDK's table. The callers answer a missing
/// entity namespace (a chain before its first entity) as no entities, so it
/// does not reach here; anything that is no fault of the query is `-32000`.
fn rejection(reason: &arkiv_db::Rejection) -> ReadError {
    use arkiv_db::{QueryFault as F, Rejection as R};
    match reason {
        R::CursorMismatch => cursor_mismatch(),
        R::OutOfBudget => limits("the read budget is exhausted").into(),
        R::InvalidQuery(F::TooManyGroups | F::TooManyLiterals) => {
            limits("the query exceeds the node's query bounds").into()
        }
        R::InvalidQuery(F::ValueTooLarge) => QueryError::new(
            QueryErrorKind::Literal,
            "a str value or prefix exceeds the indexed-string limit",
        )
        .into(),
        R::InvalidQuery(
            fault @ (F::NotIndexable | F::NotOrdered | F::MixedBounds | F::UnboundedRange),
        ) => QueryError::new(QueryErrorKind::Type, fault.to_string()).into(),
        _ => Error("ReadRejected").into(),
    }
}

/// A validated `select`.
#[derive(Clone, Debug, PartialEq)]
struct Selection {
    /// The object as resolved: the request's keys, plus `key: true` unless
    /// it says otherwise or selects nothing but `count`. It binds cursors.
    fields: Json,
    /// `count: true`: the size of the match set rides on every page.
    count: bool,
    /// Whether the request reads a page. `count` with no entity field
    /// selected reads none: `data` is empty and there is no cursor.
    rows: bool,
}

/// The `select` object: every key one of [`FIELDS`] or `count` with a
/// boolean, except `attributes`, which may also map attribute names to
/// booleans. Any other key is refused. `key` defaults to selected unless
/// `count` is the only field selected.
fn selection(value: Option<&Json>) -> Result<Selection> {
    let selected = value.cloned().unwrap_or_else(|| json!({}));
    let object = selected.as_object().ok_or(Error("InvalidParams"))?;
    for (name, v) in object {
        if name == "count" {
            if !v.is_boolean() {
                return Err(Error("InvalidParams"));
            }
            continue;
        }
        if !FIELDS.contains(&name.as_str()) {
            return Err(Error("InvalidParams"));
        }
        if name == "attributes" && v.is_object() {
            if v.as_object().unwrap().len() > 256
                || !v
                    .as_object()
                    .unwrap()
                    .iter()
                    .all(|(n, v)| arkiv::protocol_name(n) && v.is_boolean())
            {
                return Err(Error("InvalidParams"));
            }
        } else if !v.is_boolean() {
            return Err(Error("InvalidParams"));
        }
    }
    let count = object.get("count") == Some(&Json::Bool(true));
    let any_field = FIELDS.iter().any(|field| {
        object
            .get(*field)
            .is_some_and(|v| v == true || v.is_object())
    });
    let count_only = count && !any_field;
    let mut fields = selected;
    if !count_only {
        fields
            .as_object_mut()
            .unwrap()
            .entry("key")
            .or_insert(Json::Bool(true));
    }
    Ok(Selection {
        fields,
        count,
        rows: !count_only,
    })
}

/// `arkiv_getEntity`: `[key]` at the head, or `[key, block]` as of a retained
/// block (a hex quantity or a number). The SDK's single-entity read: every
/// field except `attributeSchema`, or `null` when no live entity has the key.
/// Expired entities are purged when their block is built, so a snapshot holds
/// only live ones. Planned as the one-row `$key` query it is equivalent to.
fn plan_entity(host: &Host, params: &Json, bounds: QueryLimits) -> Read<PageRead> {
    let args = params
        .as_array()
        .filter(|p| !p.is_empty() && p.len() <= 2)
        .ok_or(Error("InvalidParams"))?;
    let key = args[0]
        .as_str()
        .and_then(|s| s.strip_prefix("0x"))
        .filter(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or(Error("InvalidParams"))?;
    let mut options = json!({
        "select": {
            "key": true, "owner": true, "creator": true, "createdAt": true,
            "updatedAt": true, "expiresAt": true, "creationFlags": true,
            "attributes": true,
        },
        "limit": 1,
    });
    if let Some(block) = args.get(1).filter(|b| !b.is_null()) {
        options["atBlock"] = json!(quantity(block)?);
    }
    plan_query(
        host,
        &json!([
            format!("$key = key(0x{})", key.to_ascii_lowercase()),
            options
        ]),
        bounds,
    )
}

/// `arkiv_query`: `[query, options?]`. Options: `atBlock`, `select`, `limit`,
/// `offset` and `cursor`. A cursor carries its own position, so it continues
/// past the offset of the first page and an `offset` beside it is ignored; it
/// needs the explicit `atBlock` of the page that issued it.
fn plan_query(host: &Host, params: &Json, bounds: QueryLimits) -> Read<PageRead> {
    let args = params
        .as_array()
        .filter(|p| !p.is_empty() && p.len() <= 2)
        .ok_or(Error("InvalidParams"))?;
    let source = args[0].as_str().ok_or(Error("InvalidParams"))?;
    let Statement { query, order } = parse_query(source)?;
    let empty = json!({});
    let options = args
        .get(1)
        .unwrap_or(&empty)
        .as_object()
        .ok_or(Error("InvalidParams"))?;
    if options
        .keys()
        .any(|k| !["atBlock", "select", "limit", "offset", "cursor"].contains(&k.as_str()))
    {
        return Err(Error("InvalidParams").into());
    }
    let height = options
        .get("atBlock")
        .map(quantity)
        .transpose()?
        .unwrap_or(host.head.height);
    if height > host.head.height {
        return Err(unknown_block(height, host.head.height));
    }
    // Checked on every page: a cursor is not a lease on its snapshot.
    host.require_state(height)?;
    let selection = selection(options.get("select"))?;
    let limit = options
        .get("limit")
        .map(quantity)
        .transpose()?
        .unwrap_or(100);
    if !(1..=200).contains(&limit) {
        return Err(Error("InvalidParams").into());
    }
    let offset = options
        .get("offset")
        .map(quantity)
        .transpose()?
        .unwrap_or(0);
    let binding: [u8; 32] = keccak256(
        serde_json::to_vec(&json!([
            host.identity.genesis_hash,
            host.identity.run_id,
            height,
            source,
            selection.fields
        ]))
        .map_err(|_| Error("InvalidParams"))?,
    )
    .into();
    let cursor = if let Some(token) = options.get("cursor") {
        // A sorted page is an offset window and never issues a cursor.
        if order.is_some() {
            return Err(QueryError::new(
                QueryErrorKind::Cursor,
                "ORDER BY pages have no cursor: page them with offset, pinned to one block \
                 with atBlock",
            )
            .into());
        }
        if !options.contains_key("atBlock") {
            return Err(cursor_mismatch());
        }
        let token = token
            .as_str()
            .filter(|s| s.len() == 66)
            .ok_or_else(cursor_mismatch)?;
        let cache = CURSORS
            .get_or_init(Default::default)
            .lock()
            .map_err(|_| Error("ReadUnavailable"))?;
        let entry = cache
            .entries
            .get(token)
            .filter(|e| e.binding == binding)
            .ok_or_else(cursor_mismatch)?;
        Some(entry.cursor.clone())
    } else {
        None
    };
    Ok(PageRead {
        snapshot: host.state(height)?,
        query,
        order,
        limits: host.genesis.genesis.limits,
        bounds,
        limit: limit as usize,
        offset: if cursor.is_some() { 0 } else { offset },
        cursor,
        selection,
        binding,
        height,
    })
}

impl PageRead {
    fn run(self) -> Read<Json> {
        self.run_within(READ_BUDGET)
    }

    /// The page, every read of the request (count, offset or sort walk,
    /// rows) charged to one `budget`.
    fn run_within(self, mut budget: u64) -> Read<Json> {
        let Self {
            snapshot,
            query,
            order,
            limits,
            bounds,
            limit,
            offset,
            cursor,
            selection,
            binding,
            height,
        } = self;
        let count = if !selection.count {
            None
        } else if scans(&query, bounds) {
            // Beyond the bounds the count scans, as its page does.
            Some(scan_count(&snapshot, &query, &limits, bounds, &mut budget)?)
        } else {
            Some(count_matches(&snapshot, &query, bounds, &mut budget)?)
        };
        if !selection.rows {
            return page_result(Vec::new(), None, binding, height, count);
        }
        let window = Window {
            snapshot: &snapshot,
            query: &query,
            limits: &limits,
            bounds,
            limit,
            offset,
        };
        // `next`: where the page after this one starts, present only when
        // a further match exists. A sorted page has none.
        let (rows, mut next) = match &order {
            None => {
                let page = read_page(&window, cursor, &mut budget)?;
                (page.rows, page.next)
            }
            Some(order) => (sorted_page(&window, order, &mut budget)?, None),
        };
        let mut data = rows
            .iter()
            .map(|row| project(row, &selection.fields))
            .collect::<Result<Vec<_>>>()?;
        let fit = rows_within(&data, PAGE_BYTES)?;
        if fit < data.len() {
            if fit == 0 {
                return Err(self::limits("one entity alone exceeds the 8 MiB response cap").into());
            }
            // A sorted page has no cursor to carry the cut rows: its
            // window must fit whole.
            if order.is_some() {
                return Err(self::limits(
                    "the sorted page exceeds the 8 MiB response cap; lower the limit",
                )
                .into());
            }
            // A shorter page, whose cut rows are still to come: the next
            // page starts after its last row.
            data.truncate(fit);
            next = Some(snapshot.cursor_after(
                arkiv::entity_namespace(),
                &native_query(&query, bounds),
                &Projection::ALL,
                rows[fit - 1].id,
            ));
        }
        page_result(data, next, binding, height, count)
    }
}

/// How many of `rows`, in order, fit in `budget` bytes of a JSON array.
fn rows_within(rows: &[Json], budget: usize) -> Result<usize> {
    let mut used = 2;
    for (i, row) in rows.iter().enumerate() {
        let bytes = serde_json::to_vec(row).map_err(|_| Error("ResourceLimit"))?;
        used += bytes.len() + usize::from(i > 0);
        if used > budget {
            return Ok(i);
        }
    }
    Ok(rows.len())
}

fn page_result(
    data: Vec<Json>,
    next: Option<Cursor>,
    binding: [u8; 32],
    height: u64,
    count: Option<u64>,
) -> Read<Json> {
    let mut result = json!({"data":data,"blockNumber":format!("0x{height:x}")});
    if let Some(count) = count {
        result["count"] = json!(format!("0x{count:x}"));
    }
    if serde_json::to_vec(&result)
        .map_err(|_| Error("ResourceLimit"))?
        .len()
        > RESPONSE_CAP
    {
        return Err(limits("the response exceeds 8 MiB; lower the limit or select less").into());
    }
    if let Some(cursor) = next {
        let mut cache = CURSORS
            .get_or_init(Default::default)
            .lock()
            .map_err(|_| Error("ReadUnavailable"))?;
        cache.serial = cache
            .serial
            .checked_add(1)
            .ok_or_else(|| limits("cursor serials are exhausted"))?;
        let token =
            hex(keccak256([binding.as_slice(), &cache.serial.to_be_bytes()].concat()).as_slice());
        while cache.entries.len() >= CURSOR_CAP {
            if let Some(old) = cache.order.pop_front() {
                cache.entries.remove(&old);
            }
        }
        cache.order.push_back(token.clone());
        cache
            .entries
            .insert(token.clone(), CachedCursor { binding, cursor });
        result["cursor"] = json!(token);
    }
    Ok(result)
}

/// The page a request asks for, before its starting position.
struct Window<'a> {
    snapshot: &'a Snapshot,
    query: &'a Query,
    limits: &'a Limits,
    bounds: QueryLimits,
    limit: usize,
    /// Matches to skip first.
    offset: u64,
}

/// Whether `query` exceeds the read's group or literal `bounds`, so that it
/// takes the bounded scan fallback instead of the index.
fn scans(query: &Query, bounds: QueryLimits) -> bool {
    query.groups.len() > bounds.max_groups
        || query.groups.iter().any(|g| g.len() > bounds.max_literals)
}

/// The query the engine evaluates for `query`: itself, or every entity for
/// the scan fallback, which filters the rows itself.
fn native_query(query: &Query, bounds: QueryLimits) -> Query {
    if scans(query, bounds) {
        Query::match_all()
    } else {
        query.clone()
    }
}

/// A record-id page: combine native pages within the read's group and
/// literal `bounds` (the chain's committed limits unless the node raised
/// them). Larger Boolean trees use a bounded scan fallback of at most
/// [`SCAN_ROWS`] rows; other queries take the engine's indexed path. Without
/// a cursor, the first `offset` matches are skipped: by id alone on the
/// indexed path, then the page reads on after the last of them. An offset at
/// or past the end gives an empty page without a continuation.
fn read_page(window: &Window<'_>, mut cursor: Option<Cursor>, budget: &mut u64) -> Read<QueryPage> {
    let &Window {
        snapshot,
        query,
        limits,
        bounds,
        limit,
        offset,
    } = window;
    let scan = scans(query, bounds);
    let native = native_query(query, bounds);
    let end = |rows| Ok(QueryPage { rows, next: None });
    let mut skip = offset;
    if !scan {
        while skip > 0 {
            let size = usize::try_from(skip)
                .unwrap_or(usize::MAX)
                .min(limits.max_page);
            let Some(page) = native_page(
                snapshot,
                &native,
                &Projection::ID_ONLY,
                size,
                cursor.as_ref(),
                bounds,
                budget,
            )?
            else {
                return end(Vec::new());
            };
            skip -= page.rows.len() as u64;
            cursor = match page.next {
                // Read on after the last skipped match with the page's
                // projection: a cursor is a position, not a count.
                Some(next) if skip == 0 => Some(snapshot.cursor_after(
                    arkiv::entity_namespace(),
                    &native,
                    &Projection::ALL,
                    next.after(),
                )),
                Some(next) => Some(next),
                None => return end(Vec::new()),
            };
        }
    }
    let mut rows = Vec::new();
    let mut resume = None;
    for _ in 0..SCAN_ROWS {
        let page_size = if scan {
            1
        } else {
            (limit - rows.len()).min(limits.max_page)
        };
        if page_size == 0 {
            return Err(exhausted());
        }
        let Some(page) = native_page(
            snapshot,
            &native,
            &Projection::ALL,
            page_size,
            cursor.as_ref(),
            bounds,
            budget,
        )?
        else {
            // Before the entity namespace exists there are no entities.
            return end(rows);
        };
        for row in page.rows {
            if !scan || matches_query(query, &row) {
                if skip > 0 {
                    skip -= 1;
                    continue;
                }
                if rows.len() == limit {
                    return Ok(QueryPage { rows, next: resume });
                }
                rows.push(row);
                resume = page.next.clone();
            }
        }
        cursor = page.next;
        if cursor.is_none() || (!scan && rows.len() == limit) {
            return Ok(QueryPage { rows, next: cursor });
        }
    }
    Err(exhausted())
}

/// What the scan fallback reads of each row to evaluate `query`: the cells
/// its literals name, plus `extra`, and the key when `key`; every cell when
/// they are more than a projection may name.
fn scan_projection(
    query: &Query,
    limits: &Limits,
    key: bool,
    extra: Option<&CellName>,
) -> Projection {
    let names: std::collections::BTreeSet<CellName> = query
        .groups
        .iter()
        .flatten()
        .map(|literal| literal.term.name().clone())
        .chain(extra.cloned())
        .collect();
    if names.len() > limits.max_cells {
        Projection::ALL
    } else {
        Projection::named(key, names)
    }
}

/// `select.count` for a query beyond the read bounds: the matches among at
/// most [`SCAN_ROWS`] rows, reading only the cells the query names.
fn scan_count(
    snapshot: &Snapshot,
    query: &Query,
    limits: &Limits,
    bounds: QueryLimits,
    budget: &mut u64,
) -> Read<u64> {
    let all = Query::match_all();
    let projection = scan_projection(query, limits, false, None);
    let (mut count, mut scanned, mut cursor) = (0u64, 0usize, None);
    loop {
        let Some(page) = native_page(
            snapshot,
            &all,
            &projection,
            limits.max_page,
            cursor.as_ref(),
            bounds,
            budget,
        )?
        else {
            return Ok(0);
        };
        scanned += page.rows.len();
        if scanned > SCAN_ROWS {
            return Err(exhausted());
        }
        count += page
            .rows
            .iter()
            .filter(|row| matches_query(query, row))
            .count() as u64;
        cursor = page.next;
        if cursor.is_none() {
            return Ok(count);
        }
    }
}

/// One match of a sorted walk: its sort value, id and key.
struct Ranked<'o> {
    order: &'o Order,
    value: Option<Value>,
    id: RecordId,
    key: RecordKey,
}

impl PartialEq for Ranked<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Ranked<'_> {}
impl PartialOrd for Ranked<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Ranked<'_> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.order.cmp(
            (self.value.as_ref(), self.id),
            (other.value.as_ref(), other.id),
        )
    }
}

/// An `ORDER BY` page: the `[offset, offset + limit)` window of the matches
/// in `order`, upstream's fetch-and-sort.
///
/// Every match is visited, reading only its key and sort cell (whatever its
/// kind: the sort reads unindexed attributes too), and the first
/// `offset + limit` in order are kept in a bounded heap; then the window's
/// rows are read in full by key. The walk is charged to `budget`, so a match
/// set too large to sort within the read budget is refused as `limits`. The
/// scan fallback reads only the cells it needs and stops at [`SCAN_ROWS`]
/// like a page.
fn sorted_page(window: &Window<'_>, order: &Order, budget: &mut u64) -> Read<Vec<Row>> {
    let &Window {
        snapshot,
        query,
        limits,
        bounds,
        limit,
        offset,
    } = window;
    let scan = scans(query, bounds);
    let native = native_query(query, bounds);
    let projection = if scan {
        scan_projection(query, limits, true, Some(&order.name))
    } else {
        Projection::named(true, [order.name.clone()])
    };
    let start = usize::try_from(offset).unwrap_or(usize::MAX);
    let keep = start.saturating_add(limit);
    let mut kept: BinaryHeap<Ranked<'_>> = BinaryHeap::new();
    let mut cursor: Option<Cursor> = None;
    let mut scanned = 0usize;
    loop {
        let Some(page) = native_page(
            snapshot,
            &native,
            &projection,
            limits.max_page,
            cursor.as_ref(),
            bounds,
            budget,
        )?
        else {
            return Ok(Vec::new());
        };
        for row in page.rows {
            if scan {
                scanned += 1;
                if scanned > SCAN_ROWS {
                    return Err(exhausted());
                }
                if !matches_query(query, &row) {
                    continue;
                }
            }
            let ranked = Ranked {
                order,
                value: order.value(&row),
                id: row.id,
                key: row.key.clone().ok_or(Error("StorageCorrupt"))?,
            };
            kept.push(ranked);
            if kept.len() > keep {
                kept.pop();
            }
        }
        cursor = page.next;
        if cursor.is_none() {
            break;
        }
    }
    let sorted = kept.into_sorted_vec();
    let mut rows = Vec::new();
    for ranked in sorted.iter().skip(start).take(limit) {
        let read = snapshot
            .try_get_record(
                arkiv::entity_namespace(),
                &ranked.key,
                &Projection::ALL,
                read_budget(*budget, bounds),
            )
            .map_err(Error::from)?
            .map_err(read_error)?;
        charge(budget, &read.receipt)?;
        rows.push(read.value.ok_or(Error("StorageCorrupt"))?);
    }
    Ok(rows)
}

/// The scan fallback's evaluation of `query` on one row. Like the index, it
/// only sees indexed cells: an unindexed field never matches a predicate.
fn matches_query(query: &Query, row: &Row) -> bool {
    query.groups.iter().any(|group| {
        group.iter().all(|literal| {
            let value = row
                .cells
                .iter()
                .find(|(name, cell)| name == literal.term.name() && cell.is_attribute())
                .map(|(_, c)| &c.value);
            let matches = value.is_some_and(|value| match &literal.term {
                Term::Eq {
                    value: expected, ..
                } => value == expected,
                Term::NotEq {
                    value: expected, ..
                } => value.tag() == expected.tag() && value != expected,
                Term::Exists { tag, .. } => value.tag() == *tag,
                Term::StartsWith { prefix, .. } => {
                    matches!(value, Value::Str(s) if s.starts_with(prefix.as_ref()))
                }
                Term::Range { lower, upper, .. } => {
                    let lower_ok = match lower {
                        Bound::Unbounded => true,
                        Bound::Included(v) => value.cmp_ordered(v).is_some_and(|c| c.is_ge()),
                        Bound::Excluded(v) => value.cmp_ordered(v).is_some_and(|c| c.is_gt()),
                    };
                    let upper_ok = match upper {
                        Bound::Unbounded => true,
                        Bound::Included(v) => value.cmp_ordered(v).is_some_and(|c| c.is_le()),
                        Bound::Excluded(v) => value.cmp_ordered(v).is_some_and(|c| c.is_lt()),
                    };
                    lower_ok && upper_ok
                }
            });
            matches != literal.negated
        })
    })
}

fn scalar(value: &Value) -> Result<(&'static str, Json)> {
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
        _ => return Err(Error("InvalidEntity")),
    })
}

/// Every user attribute of a verified entity, indexed or not, as
/// `{name, type, indexed, value}` with the `arkiv_query` value encoding.
pub(crate) fn verified_attributes(e: &arkiv::ArkivEntityProjection) -> Result<Json> {
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

/// One entity in the SDK's `RpcEntity` shape, with only the selected keys.
/// `attributeSchema` entries are `{name, type, indexed}`; `attributes`
/// carries every user attribute, indexed or not, `bytes` values included.
fn project(row: &Row, selected: &Json) -> Result<Json> {
    let e = arkiv::project(row).map_err(|_| Error("InvalidEntity"))?;
    let mut result = serde_json::Map::new();
    for field in FIELDS {
        let choice = &selected[*field];
        if choice != true && !(*field == "attributes" && choice.is_object()) {
            continue;
        }
        let value = match *field {
            "key" => json!(hex(&e.key)),
            "owner" => json!(hex(&e.owner)),
            "creator" => json!(hex(&e.creator)),
            "createdAt" => json!(format!("0x{:x}", e.created_at)),
            "updatedAt" => json!(format!("0x{:x}", e.updated_at)),
            "expiresAt" => json!(format!("0x{:x}", e.expires_at)),
            "creationFlags" => {
                json!({
                    "raw": e.creation_flags,
                    "readonly": e.creation_flags&1!=0,
                    "permissionlessExtension": e.creation_flags&2!=0,
                })
            }
            "attributeSchema" => Json::Array(
                e.attributes
                    .iter()
                    .map(|(n, c)| {
                        scalar(&c.value).map(
                            |(t, _)| json!({"name":n.as_str(),"type":t,"indexed":c.is_attribute()}),
                        )
                    })
                    .collect::<Result<_>>()?,
            ),
            "attributes" => Json::Array(
                e.attributes
                    .iter()
                    .filter(|(n, _)| choice == true || choice[n.as_str()] == true)
                    .map(|(n, c)| {
                        scalar(&c.value).map(|(t, v)| json!({"name":n.as_str(),"type":t,"value":v}))
                    })
                    .collect::<Result<_>>()?,
            ),
            _ => unreachable!(),
        };
        result.insert((*field).into(), value);
    }
    Ok(Json::Object(result))
}

// Boolean conversion is bounded before expansion, not after allocating a DNF.
fn bounded(groups: Vec<Vec<Literal>>) -> std::result::Result<Query, QueryError> {
    if groups.len() > GROUP_CAP || groups.iter().map(Vec::len).sum::<usize>() > LITERAL_CAP {
        return Err(limits(
            "the query expands to more than 64 groups or 256 literals",
        ));
    }
    Ok(Query { groups })
}

fn union(mut a: Query, b: Query) -> std::result::Result<Query, QueryError> {
    if a.groups.len() + b.groups.len() > GROUP_CAP {
        return Err(limits("the query expands to more than 64 groups"));
    }
    a.groups.extend(b.groups);
    bounded(a.groups)
}

fn intersect(a: Query, b: Query) -> std::result::Result<Query, QueryError> {
    if a.groups.len().saturating_mul(b.groups.len()) > GROUP_CAP {
        return Err(limits("the query expands to more than 64 groups"));
    }
    let total = a
        .groups
        .iter()
        .map(Vec::len)
        .sum::<usize>()
        .saturating_mul(b.groups.len())
        + b.groups
            .iter()
            .map(Vec::len)
            .sum::<usize>()
            .saturating_mul(a.groups.len());
    if total > LITERAL_CAP {
        return Err(limits("the query expands to more than 256 literals"));
    }
    bounded(
        a.groups
            .iter()
            .flat_map(|a| {
                b.groups
                    .iter()
                    .map(move |b| a.iter().chain(b).cloned().collect())
            })
            .collect(),
    )
}

fn complement(query: Query) -> std::result::Result<Query, QueryError> {
    let mut out = Query::match_all();
    for group in query.groups {
        out = intersect(
            out,
            Query {
                groups: group
                    .into_iter()
                    .map(|mut l| {
                        l.negated = !l.negated;
                        vec![l]
                    })
                    .collect(),
            },
        )?;
    }
    Ok(out)
}

fn name(key: &AnnotKey) -> Option<CellName> {
    let text = match key {
        AnnotKey::User(n) => n.as_str(),
        AnnotKey::BuiltIn(b) => match b {
            BuiltIn::Owner => "$owner",
            BuiltIn::Creator => "$creator",
            BuiltIn::Key => "$key",
            BuiltIn::ExpiresAt => "$expiresAt",
            BuiltIn::CreatedAt => "$createdAt",
        },
    };
    CellName::new(text).ok()
}

fn native_value(key: &AnnotKey, value: &AttributeValue) -> Option<Value> {
    Some(match value {
        AttributeValue::Bool(v) => Value::Bool(*v),
        AttributeValue::Int(v) => Value::I32(*v),
        AttributeValue::U64(v) => Value::U64(*v),
        AttributeValue::U256(v)
            if matches!(
                key,
                AnnotKey::BuiltIn(BuiltIn::CreatedAt | BuiltIn::ExpiresAt)
            ) =>
        {
            Value::U64(U256::from_be_bytes(*v).try_into().ok()?)
        }
        AttributeValue::U256(v) => Value::U256(*v),
        AttributeValue::Decimal(v) => Value::Dec(*v),
        AttributeValue::Bytes32(v) => Value::Bytes32(*v),
        AttributeValue::Str(v) => Value::Str(v.as_str().into()),
        AttributeValue::EthereumAddress(v) => Value::Addr(*v),
        AttributeValue::EntityKey(v) => Value::Key(*v),
    })
}

/// One predicate's term; `source` and `start` locate it for errors.
fn leaf(ast: Ast, neq: bool, source: &str, start: usize) -> std::result::Result<Query, QueryError> {
    if let Ast::Between { key, low, high } = ast {
        let n = name(&key).ok_or_else(|| {
            QueryError::at(QueryErrorKind::Parse, source, start, "an attribute name")
        })?;
        let bound = |value| {
            native_value(&key, &value).ok_or_else(|| {
                QueryError::at(
                    QueryErrorKind::Literal,
                    source,
                    start,
                    "a block height that fits in u64",
                )
            })
        };
        let (low, high) = (bound(low)?, bound(high)?);
        // Inclusive at both ends; a low bound above the high one matches
        // nothing, which the engine's empty run already gives.
        return Ok(Query::term(Term::range(
            n,
            Bound::Included(low),
            Bound::Included(high),
        )));
    }
    let (key, value, op) = match ast {
        Ast::Eq { key, value } => (key, value, 0),
        Ast::Gt { key, value } => (key, value, 1),
        Ast::Gte { key, value } => (key, value, 2),
        Ast::Lt { key, value } => (key, value, 3),
        Ast::Lte { key, value } => (key, value, 4),
        Ast::StartsWith { key, value } => (key, value, 5),
        _ => {
            return Err(QueryError::at(
                QueryErrorKind::Parse,
                source,
                start,
                "a predicate",
            ));
        }
    };
    let n = name(&key)
        .ok_or_else(|| QueryError::at(QueryErrorKind::Parse, source, start, "an attribute name"))?;
    let v = native_value(&key, &value).ok_or_else(|| {
        QueryError::at(
            QueryErrorKind::Literal,
            source,
            start,
            "a block height that fits in u64",
        )
    })?;
    let term = match op {
        0 if neq => Term::not_eq(n, v),
        0 => Term::eq(n, v),
        1 => Term::range(n, Bound::Excluded(v), Bound::Unbounded),
        2 => Term::range(n, Bound::Included(v), Bound::Unbounded),
        3 => Term::range(n, Bound::Unbounded, Bound::Excluded(v)),
        4 => Term::range(n, Bound::Unbounded, Bound::Included(v)),
        _ => match v {
            Value::Str(s) => Term::starts_with(n, &s),
            _ => {
                return Err(QueryError::at(
                    QueryErrorKind::Type,
                    source,
                    start,
                    "STARTSWITH takes a str prefix",
                ));
            }
        },
    };
    Ok(Query::term(term))
}

const TAGS: &[(&str, TypeTag)] = &[
    ("bool", TypeTag::Bool),
    ("i32", TypeTag::I32),
    ("u64", TypeTag::U64),
    ("u256", TypeTag::U256),
    ("dec", TypeTag::Dec),
    ("bytes32", TypeTag::Bytes32),
    ("str", TypeTag::Str),
    ("addr", TypeTag::Addr),
    ("key", TypeTag::Key),
];
struct Parser<'a> {
    source: &'a str,
    tokens: Vec<SpannedToken>,
    at: usize,
    depth: usize,
    predicates: usize,
}

/// A source-language error, located in the whole query: `offset` is where
/// the sub-string the parser saw starts.
fn source_error(error: ParseError, source: &str, offset: usize) -> QueryError {
    let kind = match error.kind {
        ParseErrorKind::MalformedInputError => QueryErrorKind::Parse,
        ParseErrorKind::TypeError => QueryErrorKind::Type,
        ParseErrorKind::LiteralError => QueryErrorKind::Literal,
        ParseErrorKind::QueryLimitError => QueryErrorKind::Limits,
    };
    match error.failure_position {
        Some(position) => QueryError::at(kind, source, offset + position, error.message),
        None => QueryError::new(kind, error.message),
    }
}

/// Parses an `arkiv_query` string: `*` or a Boolean expression, then an
/// optional `ORDER BY` clause, which ends it.
fn parse_query(source: &str) -> std::result::Result<Statement, QueryError> {
    if source.len() > QUERY_BYTES {
        return Err(limits("the query is longer than 8192 bytes"));
    }
    let tokens = lexer::tokenize(source).map_err(|e| source_error(e, source, 0))?;
    let mut p = Parser {
        source,
        tokens,
        at: 0,
        depth: 0,
        predicates: 0,
    };
    // `*` is the whole filter: only an `ORDER BY` may follow it.
    let query = if p.take(Token::Star) {
        Query::match_all()
    } else {
        p.or()?
    };
    // After a filter only AND or OR may follow, so a name here can only
    // open the clause: its words are not reserved anywhere else.
    let order = if p.word("order") {
        Some(p.order_by()?)
    } else {
        None
    };
    if p.at != p.tokens.len() {
        return Err(p.parse_error(if order.is_some() {
            "the end of the query: ORDER BY ends it and sorts by one attribute"
        } else {
            "AND, OR, ORDER BY or the end of the query"
        }));
    }
    Ok(Statement { query, order })
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.at).map(|t| &t.token)
    }

    /// Where the next token starts, or the end of the query.
    fn position(&self) -> usize {
        self.tokens
            .get(self.at)
            .map_or(self.source.len(), |t| t.start)
    }

    fn parse_error(&self, expected: &str) -> QueryError {
        QueryError::at(
            QueryErrorKind::Parse,
            self.source,
            self.position(),
            expected,
        )
    }

    fn take(&mut self, t: Token) -> bool {
        if self.peek() == Some(&t) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    /// Whether the next token is the name `word`, in any case.
    fn word(&self, word: &str) -> bool {
        matches!(self.peek(), Some(Token::Name(n)) if n.eq_ignore_ascii_case(word))
    }

    /// `ORDER BY attrRef [':' typeTag] [ASC | DESC]`, from its `ORDER`. A
    /// user attribute names the type it sorts as; a system attribute holds
    /// one, which a `:` may repeat but not contradict. `bytes` is never
    /// sorted on.
    fn order_by(&mut self) -> std::result::Result<Order, QueryError> {
        self.at += 1;
        if !self.word("by") {
            return Err(self.parse_error("BY: the clause is ORDER BY name:type"));
        }
        self.at += 1;
        let key_at = self.position();
        let (name, held) = match self.peek().cloned() {
            Some(Token::Name(n)) if arkiv::sdk_safe_name(&n) => (n, None),
            Some(Token::SysName(n)) => {
                let tag = match n.as_str() {
                    "key" => TypeTag::Key,
                    "owner" | "creator" => TypeTag::Addr,
                    "expiresAt" | "createdAt" => TypeTag::U64,
                    _ => {
                        return Err(QueryError::at(
                            QueryErrorKind::Type,
                            self.source,
                            key_at,
                            "a sortable system attribute: $key, $owner, $creator, $expiresAt \
                             or $createdAt",
                        ));
                    }
                };
                (format!("${n}"), Some(tag))
            }
            _ => return Err(self.parse_error("an attribute to sort by")),
        };
        self.at += 1;
        let name = CellName::new(&name).map_err(|_| {
            QueryError::at(
                QueryErrorKind::Parse,
                self.source,
                key_at,
                "an attribute name",
            )
        })?;
        let named = if self.take(Token::Colon) {
            let tag_at = self.position();
            let tag = match self.peek().cloned() {
                Some(Token::Name(tag)) => TAGS
                    .iter()
                    .find(|(s, _)| s.eq_ignore_ascii_case(&tag))
                    .map(|(_, t)| *t)
                    .ok_or_else(|| {
                        QueryError::at(
                            QueryErrorKind::Type,
                            self.source,
                            tag_at,
                            "a sortable type tag: bool, i32, u64, u256, dec, bytes32, str, \
                             addr or key (bytes is never sorted on)",
                        )
                    })?,
                Some(Token::Tagged { .. }) => {
                    return Err(QueryError::at(
                        QueryErrorKind::Type,
                        self.source,
                        tag_at,
                        "a bare type tag, as in ORDER BY name:str",
                    ));
                }
                _ => return Err(self.parse_error("a type tag")),
            };
            self.at += 1;
            Some((tag, tag_at))
        } else {
            None
        };
        let tag = match (held, named) {
            (Some(held), Some((tag, tag_at))) if tag != held => {
                return Err(QueryError::at(
                    QueryErrorKind::Type,
                    self.source,
                    tag_at,
                    format!("{name} holds {held}, so it sorts as one, not as {tag}"),
                ));
            }
            (Some(held), _) => held,
            (None, Some((tag, _))) => tag,
            (None, None) => {
                return Err(QueryError::at(
                    QueryErrorKind::Type,
                    self.source,
                    key_at,
                    format!(
                        "{name} can hold any type, so ORDER BY names the one to sort it as: \
                         ORDER BY {name}:str, or another tag"
                    ),
                ));
            }
        };
        let descending = if self.word("desc") {
            self.at += 1;
            true
        } else {
            if self.word("asc") {
                self.at += 1;
            }
            false
        };
        Ok(Order {
            name,
            tag,
            descending,
        })
    }

    fn expect(&mut self, t: Token, expected: &str) -> std::result::Result<(), QueryError> {
        if self.take(t) {
            Ok(())
        } else {
            Err(self.parse_error(expected))
        }
    }

    fn or(&mut self) -> std::result::Result<Query, QueryError> {
        let mut q = self.and()?;
        while self.take(Token::Or) {
            q = union(q, self.and()?)?;
        }
        Ok(q)
    }

    fn and(&mut self) -> std::result::Result<Query, QueryError> {
        let mut q = self.unary()?;
        while self.take(Token::And) {
            q = intersect(q, self.unary()?)?;
        }
        Ok(q)
    }

    fn unary(&mut self) -> std::result::Result<Query, QueryError> {
        if self.depth >= MAX_DEPTH {
            return Err(limits("the query nests more than 32 levels deep"));
        }
        self.depth += 1;
        let result = if self.take(Token::Not) {
            self.unary().and_then(complement)
        } else if self.take(Token::LParen) {
            self.or()
                .and_then(|q| self.expect(Token::RParen, "')'").map(|()| q))
        } else {
            self.predicate()
        };
        self.depth -= 1;
        result
    }

    fn predicate(&mut self) -> std::result::Result<Query, QueryError> {
        self.predicates += 1;
        if self.predicates > MAX_PREDICATES {
            return Err(limits("the query has more than 64 predicates"));
        }
        if matches!(self.peek(), Some(Token::Exists | Token::TypeOf)) {
            let typed = self.take(Token::TypeOf);
            if !typed {
                self.expect(Token::Exists, "EXISTS")?;
            }
            self.expect(Token::LParen, "'('")?;
            let n = match self.peek().cloned() {
                Some(Token::Name(n)) if arkiv::sdk_safe_name(&n) => n,
                Some(Token::SysName(_)) => {
                    return Err(QueryError::at(
                        QueryErrorKind::Type,
                        self.source,
                        self.position(),
                        "a user attribute: system attributes are always present",
                    ));
                }
                _ => return Err(self.parse_error("an attribute name")),
            };
            self.at += 1;
            self.expect(Token::RParen, "')'")?;
            let n = CellName::new(&n).map_err(|_| self.parse_error("an attribute name"))?;
            if typed {
                self.expect(Token::Eq, "'='")?;
                let Some(Token::Name(tag)) = self.peek().cloned() else {
                    return Err(self.parse_error("a type tag"));
                };
                let tag = TAGS
                    .iter()
                    .find(|(s, _)| s.eq_ignore_ascii_case(&tag))
                    .map(|(_, t)| *t)
                    .ok_or_else(|| {
                        QueryError::at(
                            QueryErrorKind::Type,
                            self.source,
                            self.position(),
                            "an indexable type tag: bool, i32, u64, u256, dec, bytes32, str, \
                             addr or key (bytes is never indexed)",
                        )
                    })?;
                self.at += 1;
                Ok(Query::term(Term::exists(n, tag)))
            } else {
                Ok(Query {
                    groups: TAGS
                        .iter()
                        .map(|(_, t)| vec![Literal::of(Term::exists(n.clone(), *t))])
                        .collect(),
                })
            }
        } else {
            if !matches!(self.peek(), Some(Token::Name(_) | Token::SysName(_))) {
                return Err(self.parse_error("a predicate"));
            }
            let start = self.position();
            let op = self.tokens.get(self.at + 1).ok_or_else(|| {
                QueryError::at(
                    QueryErrorKind::Parse,
                    self.source,
                    self.source.len(),
                    "a comparison operator",
                )
            })?;
            let neq = op.token == Token::Neq;
            let between = op.token == Token::Between;
            let op_start = op.start;
            // `name BETWEEN low AND high` is five tokens, its AND its own.
            if between && self.tokens.get(self.at + 3).map(|t| &t.token) != Some(&Token::And) {
                return Err(QueryError::at(
                    QueryErrorKind::Parse,
                    self.source,
                    self.tokens
                        .get(self.at + 3)
                        .map_or(self.source.len(), |t| t.start),
                    "AND: BETWEEN joins its two bounds with AND",
                ));
            }
            self.at += if between { 5 } else { 3 };
            if self.at > self.tokens.len() {
                return Err(QueryError::at(
                    QueryErrorKind::Parse,
                    self.source,
                    self.source.len(),
                    "a value",
                ));
            }
            let end = self.position();
            let mut predicate = self.source[start..end].to_string();
            if neq {
                // Same width, so source-parser offsets stay aligned.
                predicate.replace_range(op_start - start..op_start - start + 2, "= ");
            }
            let ast = parse::parse(&predicate).map_err(|e| source_error(e, self.source, start))?;
            leaf(ast, neq, self.source, start)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // The reads as served under the lock: planned and run back to back.
    fn bounds(host: &Host) -> QueryLimits {
        ReadLimits::default().query_limits(&host.genesis.genesis.limits)
    }

    fn entity_count(host: &Host, params: &Json) -> Read<Json> {
        plan_count(host, params, bounds(host)).and_then(Planned::run)
    }

    fn query(host: &Host, params: &Json) -> Read<Json> {
        plan_query(host, params, bounds(host)).and_then(PageRead::run)
    }

    /// The SDK's `select`: every key sent as an explicit boolean.
    fn sdk_select(attributes: Json) -> Json {
        json!({
            "key": true,
            "owner": true,
            "creator": true,
            "createdAt": true,
            "updatedAt": true,
            "expiresAt": true,
            "creationFlags": true,
            "attributeSchema": true,
            "attributes": attributes,
        })
    }

    fn kind(result: Read<Json>) -> QueryErrorKind {
        match result {
            Err(ReadError::Query(q)) => q.kind,
            other => panic!("expected a query error, got {other:?}"),
        }
    }

    #[test]
    fn sdk_operators_and_bounds() {
        for s in [
            "*",
            "name STARTSWITH str('pre')",
            "n >= u64(3) AND n < u64(9)",
            "n != u64(7)",
            "EXISTS(name)",
            "TYPEOF(n) = u64",
            "NOT EXISTS(name)",
            "NOT (a = i32(1) OR b = true)",
            "$createdAt >= u64(0)",
        ] {
            assert!(parse_query(s).is_ok(), "{s}");
        }
        let q = parse_query("n != u64(7)").unwrap();
        assert!(matches!(q.query.groups[0][0].term, Term::NotEq { .. }));
        assert_eq!(
            parse_query("NOT EXISTS(name)").unwrap().query.groups[0].len(),
            TAGS.len()
        );
        assert!(parse_query("TYPEOF(n) = bytes").is_err());
        assert!(parse_query(&format!("{}a = true", "NOT ".repeat(40))).is_err());
        let explosive = std::iter::repeat_n("(a = true OR b = true)", 8)
            .collect::<Vec<_>>()
            .join(" AND ");
        assert!(parse_query(&explosive).is_err());
    }

    #[test]
    fn scalar_encodings_do_not_depend_on_magnitude() {
        assert_eq!(scalar(&Value::U64(1)).unwrap(), ("u64", json!("0x1")));
        assert_eq!(
            scalar(&Value::U256(U256::MAX.to_be_bytes())).unwrap().1,
            json!(format!("0x{:x}", U256::MAX))
        );
        assert_eq!(scalar(&Value::I32(-2)).unwrap().1, json!(-2));
        assert_eq!(
            scalar(&Value::Dec(arkiv::parse_decimal("-1.25").unwrap()))
                .unwrap()
                .1,
            json!("-1.25")
        );
        assert!(selection(Some(&json!({"payload":1}))).is_err());
        assert_eq!(selection(None).unwrap().fields, json!({"key":true}));
        // `payload` and `contentType` are not select keys.
        for removed in ["payload", "contentType"] {
            for choice in [true, false] {
                let mut select = sdk_select(json!(true));
                select[removed] = json!(choice);
                assert_eq!(selection(Some(&select)), Err(Error("InvalidParams")));
            }
        }
        // Every SDK `RpcSelect` key is accepted, `arkiv.schema` included.
        assert!(selection(Some(&sdk_select(json!(true)))).is_ok());
        assert!(selection(Some(&sdk_select(json!({"arkiv.schema":true,"title":true})))).is_ok());
        assert!(selection(Some(&sdk_select(json!(false)))).is_ok());
    }

    fn fixture() -> Host {
        fixture_with_period(1_000)
    }

    fn fixture_with_period(period_ms: u64) -> Host {
        fixture_with(period_ms, Vec::new())
    }

    /// The four fixture entities, then one more per entry of `extra` holding
    /// those indexed `str` values as `s0`, `s1`, ….
    fn fixture_with(period_ms: u64, extra: Vec<Vec<&str>>) -> Host {
        use crate::{
            accounts::AccountConfig,
            genesis_bundle::*,
            signed::{crypto::Signer, history::History, protocol::*},
        };
        use arkiv_db::Cell;
        use arkiv_db_memory::MemoryVersionedStore;
        use std::sync::Arc;
        let signer = Signer::from_bytes([1; 32]).unwrap();
        let mut native = crate::signed::protocol::default_native_genesis(9515, 1_000, period_ms);
        native.limits.max_page = 2;
        let genesis = SignedGenesis {
            genesis: native,
            run_id: [7; 16],
            proposer: *signer.address().as_bytes(),
            block_period_ms: period_ms,
            limits: Default::default(),
            accounts: AccountConfig::new(vec![(
                signer.address(),
                U256::from(1_000_000_000_000_000_000u64),
            )])
            .unwrap(),
            prefill: None,
        };
        let indexed = |name: &str, value| (CellName::new(name).unwrap(), Cell::attribute(value));
        let field = |name: &str, value| (CellName::new(name).unwrap(), Cell::field(value));
        let mut rows = (0..4)
            .map(|n| PrefillEntity {
                creator: *signer.address().as_bytes(),
                create_nonce: n,
                salt: n as u128,
                expires_at_height: 10,
                flags: 0,
                attributes: match n {
                    0 => vec![
                        indexed("n", Value::U64(7)),
                        field("note", Value::str("seven")),
                    ],
                    1 => vec![indexed("n", Value::U64(8))],
                    2 => vec![indexed("n", Value::str("eight"))],
                    // The same name and value as row 0, but unindexed.
                    _ => vec![
                        field("blob", Value::bytes(&[0xab, 0xcd])),
                        field("n", Value::U64(7)),
                    ],
                },
            })
            .collect::<Vec<_>>();
        for (i, values) in extra.into_iter().enumerate() {
            rows.push(PrefillEntity {
                creator: *signer.address().as_bytes(),
                create_nonce: 4 + i as u64,
                salt: 4 + i as u128,
                expires_at_height: 10,
                flags: 0,
                attributes: values
                    .into_iter()
                    .enumerate()
                    .map(|(j, v)| indexed(&format!("s{j}"), Value::str(v)))
                    .collect(),
            });
        }
        let genesis = build_genesis(
            genesis,
            PrefillLimits {
                max_entities: 256,
                max_canonical_bytes: MAX_CANONICAL_BYTES,
                max_storage_units: MAX_STORAGE_UNITS,
            },
            *signer.address().as_bytes(),
            &rows,
        )
        .unwrap();
        Host::create_with_prefill(
            Arc::new(MemoryVersionedStore::new()),
            History::create(Arc::new(MemoryVersionedStore::new()), genesis.hash()).unwrap(),
            genesis.clone(),
            [3; 16],
            SignedHeader::sign(&genesis, genesis.header(), &signer).unwrap(),
            Some(signer),
            arkiv_db::durable::Options::default(),
            Some(encode_stream(&rows).unwrap()),
        )
        .unwrap()
    }

    #[test]
    fn str_values_with_control_characters_are_stored_and_queryable() {
        // SDK 1.0's health suite writes "line one\n\tline two"; the network
        // accepts any UTF-8, and a query literal carries the characters raw.
        let multi = "line one\n\tline two";
        let tricky = "x' -- y\nORDER BY s0:str DESC";
        let host = fixture_with(
            1_000,
            vec![vec![multi, "\u{0}nul\u{7f}"], vec![tricky, "\r\n"]],
        );
        let keys = |source: String| {
            let page = query(&host, &json!([source, {"select":sdk_select(json!(true))}])).unwrap();
            page["data"]
                .as_array()
                .unwrap()
                .iter()
                .map(|row| row["attributes"][0]["value"].as_str().unwrap().to_owned())
                .collect::<Vec<_>>()
        };
        let quoted = |v: &str| format!("str('{}')", v.replace('\'', "''"));
        assert_eq!(keys(format!("s0 = {}", quoted(multi))), [multi]);
        assert_eq!(keys(format!("s0 = {}", quoted(tricky))), [tricky]);
        assert_eq!(keys("s1 = str('\u{0}nul\u{7f}')".into()), [multi]);
        assert_eq!(keys("s1 = str('\r\n')".into()), [tricky]);
        assert_eq!(keys("s0 STARTSWITH str('line one\n\t')".into()), [multi]);
        assert_eq!(keys("s0 STARTSWITH str('line one\n ')".into()), [""; 0]);
        // Control characters order by their bytes, below printable text.
        assert_eq!(keys("s1 < str(' ')".into()), [multi, tricky]);
        assert_eq!(
            keys("s0 >= str('') ORDER BY s1:str DESC".into()),
            [tricky, multi]
        );
        // The literal's own `--` and ORDER BY text stay inside the string.
        assert_eq!(
            keys(format!("s0 = {} ORDER BY s0:str ASC", quoted(tricky))),
            [tricky]
        );
        assert_eq!(
            entity_count(
                &host,
                &json!([{"query": format!("s0 = {}", quoted(multi))}])
            )
            .unwrap(),
            json!(1)
        );
    }

    fn get_entity(host: &Host, params: &Json) -> Read<Json> {
        plan_entity(host, params, bounds(host))
            .and_then(|page| Planned::Entity(Box::new(page)).run())
    }

    #[test]
    fn get_entity_returns_the_full_live_entity_or_null() {
        let mut host = fixture();
        let page = query(
            &host,
            &json!(["n = u64(7)", {"select":sdk_select(json!(true))}]),
        )
        .unwrap();
        let mut expected = page["data"][0].clone();
        let key = expected["key"].as_str().unwrap().to_owned();
        expected.as_object_mut().unwrap().remove("attributeSchema");
        let entity = get_entity(&host, &json!([key])).unwrap();
        assert_eq!(entity, expected);
        assert!(entity.get("attributeSchema").is_none());
        // Unindexed attributes are returned too.
        assert_eq!(
            entity["attributes"],
            json!([
                {"name": "n", "type": "u64", "value": "0x7"},
                {"name": "note", "type": "str", "value": "seven"},
            ])
        );
        // Keys are case-insensitive hex; an unknown key is `null`.
        assert_eq!(
            get_entity(
                &host,
                &json!([key.to_ascii_uppercase().replacen("0X", "0x", 1)])
            )
            .unwrap(),
            expected
        );
        assert_eq!(
            get_entity(&host, &json!([format!("0x{}", "00".repeat(32))])).unwrap(),
            Json::Null
        );
        // Past its expiry the entity is gone at the head but still readable at
        // a retained block, given as a number or a hex quantity.
        for _ in 0..10 {
            host.produce().unwrap();
        }
        assert_eq!(get_entity(&host, &json!([key])).unwrap(), Json::Null);
        assert_eq!(get_entity(&host, &json!([key, 0])).unwrap(), expected);
        assert_eq!(get_entity(&host, &json!([key, "0x0"])).unwrap(), expected);
        assert_eq!(get_entity(&host, &json!([key, null])).unwrap(), Json::Null);
        assert!(get_entity(&host, &json!([key, 11])).is_err());
        for params in [
            json!([]),
            json!(null),
            json!([key, 0, 0]),
            json!(["0x1234"]),
            json!([key.trim_start_matches("0x")]),
            json!([format!("0x{}", "zz".repeat(32))]),
            json!([format!("{key} OR n = u64(8)")]),
            json!([7]),
        ] {
            assert!(get_entity(&host, &params).is_err(), "{params}");
        }
    }

    #[test]
    fn indexer_genesis_walk_uses_historical_count_and_numeric_page_limit() {
        let mut host = fixture();
        for _ in 0..10 {
            host.produce().unwrap();
        }
        assert_eq!(entity_count(&host, &json!([])).unwrap(), json!(0));
        assert_eq!(
            entity_count(&host, &json!([{"block":0}])).unwrap(),
            json!(4)
        );
        assert_eq!(
            entity_count(&host, &json!([{"block":0,"query":"n = u64(7)"}])).unwrap(),
            json!(1)
        );
        let page = query(
            &host,
            &json!([
                "*",
                {
                    "atBlock": "0x0",
                    "limit": 200,
                    "select": {
                        "key": true,
                        "owner": true,
                        "creator": true,
                        "createdAt": true,
                        "updatedAt": true,
                        "expiresAt": true,
                        "creationFlags": true,
                        "attributes": true,
                    },
                },
            ]),
        )
        .unwrap();
        assert_eq!(page["data"].as_array().unwrap().len(), 4);
        assert_eq!(page["blockNumber"], "0x0");
        for params in [
            json!([{"block":11}]),
            json!([{"block":-1}]),
            json!([{"block":0.5}]),
            json!([{}, {}]),
        ] {
            assert!(entity_count(&host, &params).is_err());
        }
        assert!(query(&host, &json!(["*",{"limit":201}])).is_err());
        assert!(query(&host, &json!(["*",{"limit":0.5}])).is_err());
    }

    #[test]
    fn actual_snapshot_reads_extended_semantics_projection_and_pinned_cursors() {
        let mut host = fixture();
        let count = |q: &str| {
            query(&host, &json!([q])).unwrap()["data"]
                .as_array()
                .unwrap()
                .len()
        };
        assert_eq!(count("*"), 4);
        assert_eq!(count("n != u64(7)"), 1);
        assert_eq!(count("NOT n = u64(7)"), 3);
        assert_eq!(count("EXISTS(n)"), 3);
        assert_eq!(count("NOT EXISTS(n)"), 1);
        assert_eq!(count("TYPEOF(n) = u64"), 2);
        assert_eq!(count("n >= u64(7) AND n < u64(8)"), 1);
        assert_eq!(count("n STARTSWITH str('ei')"), 1);
        assert_eq!(count("$createdAt = u64(0)"), 4);
        assert_eq!(
            dispatch(&host, "arkiv_getEntityCount", &json!([]))
                .unwrap()
                .unwrap(),
            json!(4)
        );
        assert_eq!(
            dispatch(&host, "arkiv_getBlockTiming", &json!([]))
                .unwrap()
                .unwrap()["duration"],
            0
        );
        let selected = json!({"key":true,"attributes":{"n":true},"createdAt":true});
        let first = query(
            &host,
            &json!(["EXISTS(n)",{"limit":"0x1","select":selected}]),
        )
        .unwrap();
        assert_eq!(first["data"][0]["attributes"][0]["value"], "0x7");
        assert_eq!(first["data"][0]["createdAt"], "0x0");
        assert!(first["data"][0].get("owner").is_none());
        let token = first["cursor"].clone();
        let options = json!({"limit":"0x1","select":selected,"cursor":token,"atBlock":"0x0"});
        let second = query(&host, &json!(["EXISTS(n)", options])).unwrap();
        assert_eq!(second["data"][0]["attributes"][0]["value"], "0x8");
        host.produce().unwrap();
        assert_eq!(
            query(&host, &json!(["EXISTS(n)", options])).unwrap()["data"],
            second["data"]
        );
        assert!(query(&host, &json!(["*", options])).is_err());
        let mut changed = options.clone();
        changed["select"] = json!({"owner":true});
        assert!(query(&host, &json!(["EXISTS(n)", changed])).is_err());
        let mut unpinned = options;
        unpinned.as_object_mut().unwrap().remove("atBlock");
        assert!(query(&host, &json!(["EXISTS(n)", unpinned])).is_err());
        let last = query(
            &host,
            &json!([
                "EXISTS(n)",
                {"limit": "0x1", "select": selected, "cursor": second["cursor"], "atBlock": "0x0"},
            ]),
        )
        .unwrap();
        assert_eq!(last["data"][0]["attributes"][0]["type"], "str");
        assert!(last.get("cursor").is_none());
    }

    #[test]
    fn clause_order_and_read_bounds_do_not_change_answers() {
        let host = fixture();
        let run = |method: &str, params: Json, reads: ReadLimits| {
            plan(&host, method, &params, reads)
                .unwrap()
                .and_then(Planned::run)
        };
        let defaults = ReadLimits::default();
        // Broad and selective clauses in either order: one answer.
        for (forwards, backwards) in [
            (
                "$createdAt = u64(0) AND n = u64(7)",
                "n = u64(7) AND $createdAt = u64(0)",
            ),
            (
                "$createdAt >= u64(0) AND TYPEOF(n) = u64 AND n != u64(8)",
                "n != u64(8) AND TYPEOF(n) = u64 AND $createdAt >= u64(0)",
            ),
        ] {
            let page = |source: &str| run("arkiv_query", json!([source]), defaults).unwrap();
            assert_eq!(page(forwards), page(backwards));
            let count = |source: &str| {
                run("arkiv_getEntityCount", json!([{"query":source}]), defaults).unwrap()
            };
            assert_eq!(count(forwards), count(backwards));
        }
        // Nine AND clauses exceed the genesis bound of eight literals: a
        // count is refused and a page scans, unless the node raises it.
        let nine = std::iter::repeat_n("$createdAt = u64(0)", 8)
            .chain(["n = u64(8)"])
            .collect::<Vec<_>>()
            .join(" AND ");
        let wide = ReadLimits {
            max_literals: Some(16),
            ..defaults
        };
        assert_eq!(
            kind(run(
                "arkiv_getEntityCount",
                json!([{"query":nine}]),
                defaults
            )),
            QueryErrorKind::Limits
        );
        assert_eq!(
            run("arkiv_getEntityCount", json!([{"query":nine}]), wide),
            Ok(json!(1))
        );
        let scanned = run("arkiv_query", json!([nine]), defaults).unwrap();
        assert_eq!(scanned["data"].as_array().unwrap().len(), 1);
        assert_eq!(run("arkiv_query", json!([nine]), wide).unwrap(), scanned);
        // Nine OR terms exceed the bound of eight groups the same way.
        let ors = (0..9)
            .map(|n| format!("n = u64({n})"))
            .collect::<Vec<_>>()
            .join(" OR ");
        assert_eq!(
            kind(run(
                "arkiv_getEntityCount",
                json!([{"query":ors}]),
                defaults
            )),
            QueryErrorKind::Limits
        );
        let many = ReadLimits {
            max_groups: Some(9),
            ..defaults
        };
        assert_eq!(
            run("arkiv_getEntityCount", json!([{"query":ors}]), many),
            Ok(json!(2))
        );
        assert!(
            ReadLimits {
                max_groups: Some(0),
                ..defaults
            }
            .validate()
            .is_err()
        );
        assert!(
            ReadLimits {
                max_literals: Some(ReadLimits::MAX_LITERALS + 1),
                ..defaults
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn unindexed_attributes_are_returned_but_never_matched() {
        let host = fixture();
        let page = |source: &str| {
            query(
                &host,
                &json!([source, {"select": sdk_select(json!(true)), "limit": "0xc8"}]),
            )
            .unwrap()["data"]
                .as_array()
                .unwrap()
                .clone()
        };
        // Row 3 holds `n = u64(7)` unindexed: only row 0's indexed one matches,
        // and predicates on `blob`/`note` never match.
        assert_eq!(page("n = u64(7)").len(), 1);
        assert_eq!(page("EXISTS(n)").len(), 3);
        assert_eq!(page("note = str('seven')").len(), 0);
        assert_eq!(page("EXISTS(note) OR EXISTS(blob)").len(), 0);
        assert_eq!(
            dispatch(
                &host,
                "arkiv_getEntityCount",
                &json!([{"query":"n = u64(7)"}])
            )
            .unwrap()
            .unwrap(),
            json!(1)
        );
        let unindexed = page("NOT EXISTS(n)");
        assert_eq!(unindexed.len(), 1);
        assert_eq!(
            unindexed[0]["attributeSchema"],
            json!([
                {"name":"blob", "type":"bytes", "indexed":false},
                {"name":"n", "type":"u64", "indexed":false},
            ])
        );
        assert_eq!(
            unindexed[0]["attributes"],
            json!([
                {"name":"blob", "type":"bytes", "value":"0xabcd"},
                {"name":"n", "type":"u64", "value":"0x7"},
            ])
        );
        let indexed = page("n = u64(7)");
        assert_eq!(
            indexed[0]["attributeSchema"],
            json!([
                {"name":"n", "type":"u64", "indexed":true},
                {"name":"note", "type":"str", "indexed":false},
            ])
        );
        // The SDK's `RpcEntity` keys and nothing else.
        let keys: Vec<_> = indexed[0].as_object().unwrap().keys().cloned().collect();
        assert_eq!(
            keys,
            [
                "attributeSchema",
                "attributes",
                "createdAt",
                "creationFlags",
                "creator",
                "expiresAt",
                "key",
                "owner",
                "updatedAt"
            ]
        );
        // A named subset selects unindexed attributes by name too.
        let subset = query(
            &host,
            &json!(["n = u64(7)", {"select": sdk_select(json!({"note": true}))}]),
        )
        .unwrap();
        assert_eq!(
            subset["data"][0]["attributes"],
            json!([{"name":"note","type":"str","value":"seven"}])
        );
    }

    #[test]
    fn query_errors_use_the_sdk_code_table() {
        let mut host = fixture();
        host.produce().unwrap();
        let error = |source: &str, options: Json| match query(&host, &json!([source, options])) {
            Err(ReadError::Query(q)) => q,
            other => panic!("{source}: {other:?}"),
        };
        for (source, kind) in [
            ("n =", QueryErrorKind::Parse),
            ("(n = u64(1)", QueryErrorKind::Parse),
            ("n = u64(1) n = u64(2)", QueryErrorKind::Parse),
            ("EXISTS(and)", QueryErrorKind::Parse),
            ("n STARTSWITH u64(1)", QueryErrorKind::Type),
            ("n BETWEEN u64(1) AND i32(2)", QueryErrorKind::Type),
            ("n BETWEEN u64(1) OR u64(2)", QueryErrorKind::Parse),
            ("TYPEOF(n) = bytes", QueryErrorKind::Type),
            ("TYPEOF(n) = nope", QueryErrorKind::Type),
            ("EXISTS($owner)", QueryErrorKind::Type),
            ("$contentType = str('text/plain')", QueryErrorKind::Type),
            ("$payload = str('x')", QueryErrorKind::Type),
            ("n = i32(2147483648)", QueryErrorKind::Literal),
            ("n = dec(1.0000000000000000001)", QueryErrorKind::Literal),
        ] {
            assert_eq!(error(source, json!({})).kind, kind, "{source}");
        }
        let explosive = std::iter::repeat_n("(a = true OR b = true)", 8)
            .collect::<Vec<_>>()
            .join(" AND ");
        for source in [
            "a".repeat(8193),
            format!("{}a = true", "NOT ".repeat(40)),
            explosive,
            vec!["a = true"; 65].join(" AND "),
        ] {
            assert_eq!(error(&source, json!({})).kind, QueryErrorKind::Limits);
        }
        let cursor = error(
            "*",
            json!({"atBlock":"0x0","cursor":format!("0x{}", "00".repeat(32))}),
        );
        assert_eq!(cursor.kind, QueryErrorKind::Cursor);
        assert_eq!(
            error("*", json!({"cursor":"0x00"})).kind,
            QueryErrorKind::Cursor
        );
        let block = error("*", json!({"atBlock":"0x99"}));
        assert_eq!(block.kind, QueryErrorKind::Block);
        assert_eq!(
            kind(entity_count(&host, &json!([{"block":99}]))),
            QueryErrorKind::Block
        );
        assert_eq!(
            kind(entity_count(&host, &json!([{"query":"n ="}]))),
            QueryErrorKind::Parse
        );
        // Codes are the SDK's, and a parse error says where.
        let codes: Vec<i64> = [
            QueryErrorKind::Parse,
            QueryErrorKind::Type,
            QueryErrorKind::Literal,
            QueryErrorKind::Limits,
            QueryErrorKind::Cursor,
            QueryErrorKind::Block,
        ]
        .iter()
        .map(|k| k.code())
        .collect();
        assert_eq!(codes, [-32001, -32002, -32003, -32004, -32005, -32006]);
        let parse = error("n = u64(1) AND (m = u64(2)", json!({}));
        assert_eq!(
            parse.to_json()["data"],
            json!({"position":26,"expected":"')'","got":"end of query"})
        );
        assert_eq!(parse.to_json()["code"], -32001);
        let literal = error("n = i32(2147483648)", json!({}));
        assert_eq!(literal.to_json()["data"]["position"], 8);
        // Malformed parameters are not query errors.
        assert!(matches!(
            query(&host, &json!(["*", {"limit": 0}])),
            Err(ReadError::Node(Error("InvalidParams")))
        ));
    }

    /// A chain without a prefill: its entity namespace only appears with
    /// the proposer's bootstrap transaction in block 1.
    fn bare_fixture() -> Host {
        use crate::{
            accounts::AccountConfig,
            signed::{crypto::Signer, history::History, protocol::*},
        };
        use arkiv_db_memory::MemoryVersionedStore;
        use std::sync::Arc;
        let signer = Signer::from_bytes([1; 32]).unwrap();
        let genesis = SignedGenesis {
            genesis: crate::signed::protocol::default_native_genesis(9515, 1_000, 1_000),
            run_id: [8; 16],
            proposer: *signer.address().as_bytes(),
            block_period_ms: 1_000,
            limits: Default::default(),
            accounts: AccountConfig::new(vec![(
                signer.address(),
                U256::from(1_000_000_000_000_000_000u64),
            )])
            .unwrap(),
            prefill: None,
        };
        Host::create(
            Arc::new(MemoryVersionedStore::new()),
            History::create(Arc::new(MemoryVersionedStore::new()), genesis.hash()).unwrap(),
            genesis.clone(),
            [4; 16],
            SignedHeader::sign(&genesis, genesis.header(), &signer).unwrap(),
            Some(signer),
            arkiv_db::durable::Options::default(),
        )
        .unwrap()
    }

    #[test]
    fn a_chain_without_its_entity_namespace_has_no_entities() {
        let host = bare_fixture();
        assert_eq!(host.head.height, 0);
        for params in [
            json!(["*", {"atBlock":"0x0"}]),
            json!(["*"]),
            json!(["n = u64(1)", {"atBlock":"0x0","select":sdk_select(json!(true))}]),
        ] {
            assert_eq!(
                query(&host, &params).unwrap(),
                json!({"data":[],"blockNumber":"0x0"}),
                "{params}"
            );
        }
        for params in [
            json!([{"block":0}]),
            json!([]),
            json!([{"query":"n = u64(1)"}]),
        ] {
            assert_eq!(entity_count(&host, &params).unwrap(), json!(0), "{params}");
        }
    }

    #[test]
    fn engine_rejections_map_onto_the_sdk_table() {
        use arkiv_db::{QueryFault as F, Rejection as R};
        let kind_of = |reason: R| match rejection(&reason) {
            ReadError::Query(q) => Some(q.kind),
            ReadError::Node(e) => {
                assert_eq!(e, Error("ReadRejected"));
                None
            }
        };
        use QueryErrorKind as K;
        for (reason, kind) in [
            (R::CursorMismatch, Some(K::Cursor)),
            (R::OutOfBudget, Some(K::Limits)),
            (R::InvalidQuery(F::TooManyGroups), Some(K::Limits)),
            (R::InvalidQuery(F::TooManyLiterals), Some(K::Limits)),
            (R::InvalidQuery(F::ValueTooLarge), Some(K::Literal)),
            (R::InvalidQuery(F::NotIndexable), Some(K::Type)),
            (R::InvalidQuery(F::NotOrdered), Some(K::Type)),
            (R::InvalidQuery(F::MixedBounds), Some(K::Type)),
            (R::InvalidQuery(F::UnboundedRange), Some(K::Type)),
            // No fault of the query: a server error, never a query kind.
            (R::Unauthorized, None),
            (R::CostOverflow, None),
            (R::InvalidQuery(F::NoGroups), None),
            (R::InvalidQuery(F::ProjectionTooLarge), None),
        ] {
            assert_eq!(kind_of(reason), kind);
        }
    }

    #[test]
    fn a_page_past_the_response_cap_is_shortened_and_its_cursor_continues() {
        use crate::{
            TxEnvelope, TxOp,
            signed::{crypto::Signer, protocol::SignedTx},
        };
        use arkiv_db::Cell;
        let mut host = fixture();
        let signer = Signer::from_bytes([1; 32]).unwrap();
        const BIG: u64 = 40;
        for nonce in 0..BIG {
            let mut request_id = [0x55; 32];
            request_id[24..].copy_from_slice(&nonce.to_be_bytes());
            let tx = TxEnvelope {
                chain_id: host.genesis.genesis.chain_id,
                actor: signer.address(),
                request_id,
                tx_budget: 2_000_000,
                ops: vec![TxOp::Arkiv {
                    namespace: arkiv::entity_namespace(),
                    operation: arkiv::ArkivOperation::Create {
                        salt: 100 + nonce as u128,
                        flags: 0,
                        expiry: crate::Expiry {
                            expires_at: 0,
                            min_lifetime: 100,
                        },
                        attributes: vec![
                            (
                                CellName::new("blob").unwrap(),
                                Cell::field(Value::bytes(&vec![nonce as u8; 128 * 1024])),
                            ),
                            (
                                CellName::new("big").unwrap(),
                                Cell::attribute(Value::Bool(true)),
                            ),
                        ],
                    },
                }],
            };
            host.submit(SignedTx::sign(&host.genesis, nonce, tx, &signer).unwrap())
                .unwrap();
            if nonce % 4 == 3 {
                host.produce().unwrap();
            }
        }
        host.produce().unwrap();
        assert_eq!(
            entity_count(&host, &json!([{"query":"big = true"}])).unwrap(),
            json!(BIG)
        );
        let at = format!("0x{:x}", host.head.height);
        let options = json!({"atBlock":at,"limit":"0xc8","select":sdk_select(json!(true))});
        let first = query(&host, &json!(["big = true", options])).unwrap();
        let first_rows = first["data"].as_array().unwrap().clone();
        let size = serde_json::to_vec(&first).unwrap().len();
        // Each entity is about 256 KiB of JSON: the page stops below 8 MiB.
        assert!(size <= RESPONSE_CAP, "{size}");
        assert!(size > RESPONSE_CAP - 300 * 1024, "{size}");
        assert!(first_rows.len() < BIG as usize && !first_rows.is_empty());
        // The cursor follows the last row returned: the walk sees every
        // entity exactly once.
        let mut keys: Vec<_> = first_rows.iter().map(|e| e["key"].clone()).collect();
        let mut cursor = first.get("cursor").cloned();
        assert!(cursor.is_some());
        while let Some(token) = cursor {
            let mut more = options.clone();
            more["cursor"] = token;
            let page = query(&host, &json!(["big = true", more])).unwrap();
            keys.extend(
                page["data"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|e| e["key"].clone()),
            );
            cursor = page.get("cursor").cloned();
        }
        let distinct: std::collections::BTreeSet<_> = keys.iter().map(|k| k.to_string()).collect();
        assert_eq!((keys.len(), distinct.len()), (BIG as usize, BIG as usize));
        // A sorted page has no cursor to carry cut rows: past the cap it is
        // refused, and a window that fits is served whole.
        let sorted = |limit: u64| {
            query(
                &host,
                &json!([
                    "big = true ORDER BY $key",
                    {"atBlock": at, "limit": limit, "select": sdk_select(json!(true))},
                ]),
            )
        };
        assert_eq!(kind(sorted(200)), QueryErrorKind::Limits);
        let window = sorted(20).unwrap();
        assert_eq!(window["data"].as_array().unwrap().len(), 20);
        assert!(window.get("cursor").is_none());
        // A row that cannot fit even alone keeps the error.
        assert_eq!(rows_within(&[json!("x".repeat(64))], 16).unwrap(), 0);
        assert_eq!(rows_within(&[json!(1), json!(2)], 5).unwrap(), 2);
        assert_eq!(rows_within(&[json!(1), json!(2)], 4).unwrap(), 1);
    }

    /// Submits one signed transaction of `ops` from the fixture's signer.
    fn submit(host: &mut Host, nonce: &mut u64, ops: Vec<arkiv::ArkivOperation>) {
        use crate::{
            TxEnvelope, TxOp,
            signed::{crypto::Signer, protocol::SignedTx},
        };
        let signer = Signer::from_bytes([1; 32]).unwrap();
        let mut request_id = [0x66; 32];
        request_id[24..].copy_from_slice(&nonce.to_be_bytes());
        let tx = TxEnvelope {
            chain_id: host.genesis.genesis.chain_id,
            actor: signer.address(),
            request_id,
            tx_budget: 2_000_000,
            ops: ops
                .into_iter()
                .map(|operation| TxOp::Arkiv {
                    namespace: arkiv::entity_namespace(),
                    operation,
                })
                .collect(),
        };
        host.submit(SignedTx::sign(&host.genesis, *nonce, tx, &signer).unwrap())
            .unwrap();
        *nonce += 1;
        // The pool holds a few pending transactions per account.
        if nonce.is_multiple_of(4) {
            host.produce().unwrap();
        }
    }

    fn create(salt: u128, attributes: Vec<(&str, arkiv_db::Cell)>) -> arkiv::ArkivOperation {
        arkiv::ArkivOperation::Create {
            salt,
            flags: 0,
            expiry: crate::Expiry {
                expires_at: 0,
                min_lifetime: 100,
            },
            attributes: attributes
                .into_iter()
                .map(|(n, c)| (CellName::new(n).unwrap(), c))
                .collect(),
        }
    }

    fn delete(key: &Json) -> arkiv::ArkivOperation {
        let bytes = crate::simulator::wire::unhex(key.as_str().unwrap(), 32).unwrap();
        arkiv::ArkivOperation::Delete {
            key: RecordKey::new(&bytes).unwrap(),
        }
    }

    /// The keys of a page.
    fn keys(page: &Json) -> Vec<Json> {
        page["data"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["key"].clone())
            .collect()
    }

    /// Every page of `source` with `options`, following cursors at the
    /// first page's block: the keys and the pages.
    fn walk(host: &Host, source: &str, options: Json) -> (Vec<Json>, Vec<Json>) {
        let first = query(host, &json!([source, options])).unwrap();
        let mut all = keys(&first);
        let mut pages = vec![first.clone()];
        let mut cursor = first.get("cursor").cloned();
        while let Some(token) = cursor {
            let mut more = options.clone();
            more["cursor"] = token;
            more["atBlock"] = first["blockNumber"].clone();
            let page = query(host, &json!([source, more])).unwrap();
            all.extend(keys(&page));
            cursor = page.get("cursor").cloned();
            pages.push(page);
        }
        (all, pages)
    }

    #[test]
    fn offset_skips_matches_in_creation_order() {
        let host = fixture();
        let every = keys(&query(&host, &json!(["*", {"limit": 10}])).unwrap());
        assert_eq!(every.len(), 4);
        // The fixture's page size is two, so offsets of three or more skip
        // over more than one engine page.
        for offset in 0..6u64 {
            for limit in 1..4usize {
                for spelling in [json!(offset), json!(format!("0x{offset:x}"))] {
                    let page =
                        query(&host, &json!(["*", {"limit": limit, "offset": spelling}])).unwrap();
                    let from = (offset as usize).min(4);
                    let to = (from + limit).min(4);
                    assert_eq!(keys(&page), every[from..to], "{offset} {limit}");
                    // A cursor exactly when matches remain after the page.
                    assert_eq!(page.get("cursor").is_some(), to < 4, "{offset} {limit}");
                }
            }
        }
        // An offset at or past the end: an empty page without a cursor.
        assert_eq!(
            query(&host, &json!(["*", {"offset": "0x4"}])).unwrap(),
            json!({"data":[],"blockNumber":"0x0"})
        );
        assert_eq!(
            query(&host, &json!(["*", {"offset": u64::MAX}])).unwrap()["data"],
            json!([])
        );
        // A filtered query skips its own matches: rows 0, 1 and 2.
        let page = query(&host, &json!(["EXISTS(n)", {"offset": 2}])).unwrap();
        assert_eq!(keys(&page), every[2..3]);
        // The cursor of an offset page continues past it; an offset beside
        // a cursor is ignored, since the cursor carries the position.
        let first = query(&host, &json!(["*", {"limit": 1, "offset": 1}])).unwrap();
        let next = |extra: Json| {
            let mut options = json!({"limit": 2, "atBlock": "0x0", "cursor": first["cursor"]});
            if let Some(offset) = extra.get("offset") {
                options["offset"] = offset.clone();
            }
            query(&host, &json!(["*", options])).unwrap()
        };
        assert_eq!(keys(&next(json!({}))), every[2..4]);
        assert_eq!(next(json!({"offset": 3})), next(json!({})));
        assert!(next(json!({})).get("cursor").is_none());
        // The scan fallback (nine literals) skips its own matches too.
        let nine = |last: &str| {
            std::iter::repeat_n("$createdAt = u64(0)", 8)
                .chain([last])
                .collect::<Vec<_>>()
                .join(" AND ")
        };
        let scanned = nine("EXISTS(n)");
        assert_eq!(
            keys(&query(&host, &json!([scanned, {"offset": 1, "limit": 1}])).unwrap()),
            every[1..2]
        );
        let (walked, _) = walk(&host, &scanned, json!({"offset": 1, "limit": 1}));
        assert_eq!(walked, every[1..3]);
        // Malformed offsets are invalid params.
        for offset in [
            json!(-1),
            json!(0.5),
            json!("0x"),
            json!("1"),
            json!("0x01"),
            json!(null),
        ] {
            assert!(
                matches!(
                    query(&host, &json!(["*", {"offset": offset}])),
                    Err(ReadError::Node(Error("InvalidParams")))
                ),
                "{offset}"
            );
        }
    }

    #[test]
    fn select_count_is_the_match_set_on_every_page() {
        let host = fixture();
        let sdk = |count: bool| {
            let mut select = sdk_select(json!(true));
            select["count"] = json!(count);
            select
        };
        // Beside entity fields: the page, and the count of every match.
        let page = query(
            &host,
            &json!(["EXISTS(n)", {"select": sdk(true), "limit": 1}]),
        )
        .unwrap();
        assert_eq!(page["count"], "0x3");
        assert_eq!(page["data"].as_array().unwrap().len(), 1);
        // Repeated, unchanged, on every page; independent of limit and offset.
        let (_, pages) = walk(&host, "EXISTS(n)", json!({"select": sdk(true), "limit": 1}));
        assert_eq!(pages.len(), 3);
        assert!(pages.iter().all(|p| p["count"] == "0x3"));
        let offset = query(
            &host,
            &json!(["EXISTS(n)", {"select": sdk(true), "offset": 9}]),
        )
        .unwrap();
        assert_eq!(offset, json!({"data":[],"blockNumber":"0x0","count":"0x3"}));
        // Unselected or false: no count at all.
        for select in [sdk(false), json!({"key": true})] {
            let page = query(&host, &json!(["*", {"select": select}])).unwrap();
            assert!(page.get("count").is_none());
        }
        // Alone (the SDK's client.count() sends exactly this), or beside
        // fields all false: the count is the answer, with no rows or cursor.
        for select in [
            json!({"count": true}),
            json!({"key": false, "count": true}),
            json!({
                "key": false,
                "owner": false,
                "creator": false,
                "createdAt": false,
                "updatedAt": false,
                "expiresAt": false,
                "creationFlags": false,
                "attributeSchema": false,
                "attributes": false,
                "count": true,
            }),
        ] {
            assert_eq!(
                query(&host, &json!(["*", {"select": select, "limit": 1}])).unwrap(),
                json!({"data":[],"blockNumber":"0x0","count":"0x4"}),
            );
        }
        assert_eq!(
            query(&host, &json!(["n = u64(7)", {"select": {"count": true}}])).unwrap()["count"],
            "0x1"
        );
        // A count reads ORDER BY and ignores it, here and in getEntityCount.
        assert_eq!(
            query(
                &host,
                &json!(["EXISTS(n) ORDER BY n:u64 DESC", {"select": {"count": true}}])
            )
            .unwrap()["count"],
            "0x3"
        );
        assert_eq!(
            entity_count(&host, &json!([{"query": "TYPEOF(n) = u64 ORDER BY n:u64"}])).unwrap(),
            json!(2)
        );
        // `count` must be a boolean; a chain without entities counts zero.
        assert!(selection(Some(&json!({"count": 1}))).is_err());
        assert_eq!(
            query(&bare_fixture(), &json!(["*", {"select": {"count": true}}])).unwrap(),
            json!({"data":[],"blockNumber":"0x0","count":"0x0"})
        );
        // Beyond the read bounds (EXISTS without a type is nine groups) the
        // count scans, as its page does, where getEntityCount refuses.
        let nine = std::iter::repeat_n("$createdAt = u64(0)", 9)
            .collect::<Vec<_>>()
            .join(" AND ");
        assert_eq!(
            query(&host, &json!([nine, {"select": {"count": true}}])).unwrap()["count"],
            "0x4"
        );
        assert_eq!(
            kind(entity_count(&host, &json!([{"query": nine}]))),
            QueryErrorKind::Limits
        );
        assert_eq!(
            query(
                &host,
                &json!(["NOT EXISTS(n)", {"select": {"count": true}}])
            )
            .unwrap()["count"],
            "0x1"
        );
    }

    #[test]
    fn order_by_sorts_by_value_with_missing_values_below_and_ties_in_creation_order() {
        let host = fixture();
        let every = keys(&query(&host, &json!(["*", {"limit": 10}])).unwrap());
        let sorted = |source: &str| keys(&query(&host, &json!([source, {"limit": 10}])).unwrap());
        let rows = |ids: &[usize]| ids.iter().map(|&i| every[i].clone()).collect::<Vec<_>>();
        // n: row 0 holds 7, row 1 holds 8, row 2 a str, row 3 an unindexed 7.
        // The sort reads row 3's 7 though no predicate can; row 2 has no
        // u64 and sorts below every value: first ASC, last DESC. The two 7s
        // tie and come in creation order either way.
        assert_eq!(sorted("* ORDER BY n:u64"), rows(&[2, 0, 3, 1]));
        assert_eq!(sorted("* ORDER BY n:u64 ASC"), rows(&[2, 0, 3, 1]));
        assert_eq!(sorted("* order by n : U64 asc"), rows(&[2, 0, 3, 1]));
        assert_eq!(sorted("* ORDER BY n:u64 DESC"), rows(&[1, 0, 3, 2]));
        assert_eq!(sorted("* ORDER BY n:str DESC"), rows(&[2, 0, 1, 3]));
        assert_eq!(sorted("EXISTS(n) ORDER BY n:u64 DESC"), rows(&[1, 0, 2]));
        assert_eq!(sorted("* ORDER BY missing:i32 DESC"), every);
        // System attributes have their type; $createdAt ties everywhere here.
        let mut by_key = every.clone();
        by_key.sort_by_key(|k| k.as_str().unwrap().to_owned());
        assert_eq!(sorted("* ORDER BY $key"), by_key);
        assert_eq!(sorted("* ORDER BY $key:key ASC"), by_key);
        by_key.reverse();
        assert_eq!(sorted("* ORDER BY $key DESC"), by_key);
        assert_eq!(sorted("* ORDER BY $createdAt DESC"), every);
        assert_eq!(sorted("* ORDER BY $owner:addr"), every);
        assert_eq!(sorted("* ORDER BY $expiresAt:u64"), every);
        // Its words are read where the clause stands, nowhere else.
        assert_eq!(
            sorted("n = u64(8) OR desc = u64(1) ORDER BY desc:u64 DESC"),
            rows(&[1])
        );
        // Every window of the order, in both directions: a sorted page is an
        // offset window and carries no cursor, whatever follows it.
        for (source, order) in [
            ("* ORDER BY n:u64", [2, 0, 3, 1]),
            ("* ORDER BY n:u64 DESC", [1, 0, 3, 2]),
        ] {
            for offset in 0..5usize {
                for limit in 1..4usize {
                    let page =
                        query(&host, &json!([source, {"limit": limit, "offset": offset}])).unwrap();
                    let from = offset.min(4);
                    let to = (from + limit).min(4);
                    assert_eq!(keys(&page), rows(&order[from..to]), "{source} {offset}");
                    assert!(page.get("cursor").is_none());
                }
            }
            // Offset windows at a pinned block, with the count, partition
            // the order.
            let mut walked = Vec::new();
            for offset in (0..6).step_by(2) {
                let page = query(
                    &host,
                    &json!([
                        source,
                        {
                            "limit": 2,
                            "offset": offset,
                            "atBlock": "0x0",
                            "select": {"key": true, "count": true},
                        },
                    ]),
                )
                .unwrap();
                assert_eq!(page["count"], "0x4");
                walked.extend(keys(&page));
            }
            assert_eq!(walked, rows(&order), "{source}");
            // The sorted page is the full entity, projected as selected.
            let page = query(
                &host,
                &json!([source, {"limit": 1, "select": sdk_select(json!(true)), "offset": 3}]),
            )
            .unwrap();
            let expected = query(
                &host,
                &json!([
                    format!("$key = key({})", every[order[3]].as_str().unwrap()),
                    {"select": sdk_select(json!(true))},
                ]),
            )
            .unwrap();
            assert_eq!(page["data"], expected["data"]);
        }
        // The scan fallback sorts its matches the same way.
        let nine = std::iter::repeat_n("$createdAt = u64(0)", 8)
            .chain(["EXISTS(n)"])
            .collect::<Vec<_>>()
            .join(" AND ");
        let scanned = format!("{nine} ORDER BY n:u64 DESC");
        assert_eq!(sorted(&scanned), rows(&[1, 0, 2]));
        assert_eq!(
            keys(&query(&host, &json!([scanned, {"limit": 1, "offset": 2}])).unwrap()),
            rows(&[2])
        );
        // ORDER BY takes no cursor: not one of an unsorted page, nor a made-up
        // one, nor anything else.
        let first = query(&host, &json!(["*", {"limit": 1}])).unwrap();
        for cursor in [first["cursor"].clone(), json!("0x00"), json!(7)] {
            let error = match query(
                &host,
                &json!(["* ORDER BY n:u64", {"limit": 1, "atBlock": "0x0", "cursor": cursor}]),
            ) {
                Err(ReadError::Query(q)) => q,
                other => panic!("{other:?}"),
            };
            assert_eq!(error.kind, QueryErrorKind::Cursor);
            assert!(error.message.contains("offset"), "{}", error.message);
        }
    }

    #[test]
    fn order_by_errors_use_the_sdk_code_table() {
        let host = fixture();
        for (source, expected) in [
            ("* ORDER BY n", QueryErrorKind::Type),
            ("* ORDER BY n:bytes", QueryErrorKind::Type),
            ("* ORDER BY n:nope", QueryErrorKind::Type),
            ("* ORDER BY n:str('x')", QueryErrorKind::Type),
            ("* ORDER BY $owner:u64", QueryErrorKind::Type),
            ("* ORDER BY $updatedAt", QueryErrorKind::Type),
            ("* ORDER BY $payload", QueryErrorKind::Type),
            ("* ORDER", QueryErrorKind::Parse),
            ("* ORDER n:u64", QueryErrorKind::Parse),
            ("* ORDER BY", QueryErrorKind::Parse),
            ("* ORDER BY 5:i32", QueryErrorKind::Parse),
            ("* ORDER BY n:", QueryErrorKind::Parse),
            ("* ORDER BY n::u64", QueryErrorKind::Parse),
            ("* ORDER BY n:u64 sideways", QueryErrorKind::Parse),
            ("* ORDER BY n:u64 DESC n", QueryErrorKind::Parse),
            ("* ORDER BY n:u64 AND a = 1", QueryErrorKind::Parse),
            ("* ORDER BY n:u64 ORDER BY n:u64", QueryErrorKind::Parse),
            ("(n = u64(1) ORDER BY n:u64)", QueryErrorKind::Parse),
            ("* AND n = u64(1)", QueryErrorKind::Parse),
        ] {
            assert_eq!(kind(query(&host, &json!([source]))), expected, "{source}");
        }
        let error = match query(&host, &json!(["* ORDER BY rank"])) {
            Err(ReadError::Query(q)) => q,
            other => panic!("{other:?}"),
        };
        assert!(
            error.message.contains("ORDER BY rank:str"),
            "{}",
            error.message
        );
        assert_eq!(error.to_json()["data"]["position"], "* ORDER BY ".len());
        assert_eq!(error.to_json()["code"], -32002);
    }

    #[test]
    fn between_and_ranges_take_every_indexable_type() {
        let host = fixture();
        let every = keys(&query(&host, &json!(["*", {"limit": 10}])).unwrap());
        let count = |source: &str| {
            query(&host, &json!([source])).unwrap()["data"]
                .as_array()
                .unwrap()
                .len()
        };
        assert_eq!(count("n BETWEEN u64(7) AND u64(8)"), 2);
        assert_eq!(count("n BETWEEN u64(8) AND u64(8)"), 1);
        assert_eq!(count("n between u64(8) and u64(7)"), 0);
        assert_eq!(count("n BETWEEN u64(7) AND u64(8) AND n = u64(8)"), 1);
        assert_eq!(count("NOT n BETWEEN u64(7) AND u64(8)"), 2);
        assert_eq!(count("n BETWEEN str('a') AND str('f')"), 1);
        assert_eq!(count("n > str('a')"), 1);
        assert_eq!(count("n < str('a')"), 0);
        let mut sorted = every;
        sorted.sort_by_key(|k| k.as_str().unwrap().to_owned());
        let (low, high) = (sorted[1].as_str().unwrap(), sorted[2].as_str().unwrap());
        assert_eq!(count(&format!("$key > key({low})")), 2);
        assert_eq!(
            count(&format!("$key BETWEEN key({low}) AND key({high})")),
            2
        );
        assert_eq!(
            entity_count(&host, &json!([{"query": "n BETWEEN u64(7) AND u64(8)"}])).unwrap(),
            json!(2)
        );
        assert_eq!(count("$expiresAt BETWEEN u64(10) AND u64(10)"), 4);
    }

    #[test]
    fn offset_sort_and_count_walks_are_charged_to_the_read_budget() {
        let mut host = fixture();
        let mut nonce = 0;
        for i in 0..40u64 {
            submit(
                &mut host,
                &mut nonce,
                vec![create(
                    1000 + u128::from(i),
                    vec![("w", arkiv_db::Cell::attribute(Value::U64(i % 7)))],
                )],
            );
        }
        host.produce().unwrap();
        let run = |source: &str, options: Json, budget: u64| {
            plan_query(&host, &json!([source, options]), bounds(&host))
                .and_then(|page| page.run_within(budget))
        };
        // The least budget each read is served under: a walk past 39
        // matches, or through all 40 to sort them, costs more than a page of
        // one, and is refused as `limits` under that page's budget.
        let least = |source: &str, options: Json| {
            let (mut low, mut high) = (1, READ_BUDGET);
            assert!(run(source, options.clone(), high).is_ok());
            while low < high {
                let mid = low + (high - low) / 2;
                match run(source, options.clone(), mid) {
                    Ok(_) => high = mid,
                    Err(error) => {
                        assert_eq!(kind(Err(error)), QueryErrorKind::Limits);
                        low = mid + 1;
                    }
                }
            }
            low
        };
        let typed = "TYPEOF(w) = u64";
        let page = least(typed, json!({"limit": 1}));
        let skip = least(typed, json!({"limit": 1, "offset": 39}));
        let sort = least("TYPEOF(w) = u64 ORDER BY w:u64", json!({"limit": 1}));
        let count = least(
            typed,
            json!({"limit": 1, "select": {"key": true, "count": true}}),
        );
        assert!(skip > page + 39, "{skip} {page}");
        assert!(sort > page + 40 * 3, "{sort} {page}");
        assert!(count > page, "{count} {page}");
        for (source, options) in [
            (typed, json!({"limit": 1, "offset": 39})),
            ("TYPEOF(w) = u64 ORDER BY w:u64", json!({"limit": 1})),
        ] {
            assert_eq!(kind(run(source, options, page)), QueryErrorKind::Limits);
        }
        // Under the real budget both answer, and agree with a full walk.
        let every = keys(&run("TYPEOF(w) = u64", json!({"limit": 40}), READ_BUDGET).unwrap());
        assert_eq!(
            keys(
                &run(
                    "TYPEOF(w) = u64",
                    json!({"limit": 1, "offset": 39}),
                    READ_BUDGET
                )
                .unwrap()
            ),
            every[39..]
        );
        let sorted = keys(
            &run(
                "TYPEOF(w) = u64 ORDER BY w:u64 DESC",
                json!({"limit": 40}),
                READ_BUDGET,
            )
            .unwrap(),
        );
        let mut expected: Vec<(u64, usize)> = (0..40).map(|i| (i % 7, i as usize)).collect();
        expected.sort_by_key(|&(w, i)| (std::cmp::Reverse(w), i));
        assert_eq!(
            sorted,
            expected
                .iter()
                .map(|&(_, i)| every[i].clone())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn offset_pages_at_a_pinned_block_stay_stable_while_the_head_changes() {
        let mut host = fixture();
        let mut nonce = 0;
        for i in 0..12u64 {
            submit(
                &mut host,
                &mut nonce,
                vec![create(
                    2000 + u128::from(i),
                    vec![("p", arkiv_db::Cell::attribute(Value::U64(i % 3)))],
                )],
            );
        }
        host.produce().unwrap();
        let pinned = format!("0x{:x}", host.head.height);
        let select = |count: bool| json!({"key": true, "attributes": true, "count": count});
        // Each page, its fresh cursor token reduced to whether it has one.
        let pages = |host: &Host, source: &str, at: Option<&str>| {
            (0..5u64)
                .map(|page| {
                    let mut options =
                        json!({"limit": 3, "offset": page * 3, "select": select(true)});
                    if let Some(at) = at {
                        options["atBlock"] = json!(at);
                    }
                    let mut page = query(host, &json!([source, options])).unwrap();
                    if let Some(cursor) = page.get_mut("cursor") {
                        *cursor = json!(true);
                    }
                    page
                })
                .collect::<Vec<_>>()
        };
        // The indexed path, the scan fallback (EXISTS without a type is
        // nine groups) and a sorted walk.
        let sources = [
            "TYPEOF(p) = u64",
            "EXISTS(p)",
            "TYPEOF(p) = u64 ORDER BY p:u64 DESC",
        ];
        let before: Vec<_> = sources
            .iter()
            .map(|source| pages(&host, source, Some(&pinned)))
            .collect();
        for (source, pages) in sources.iter().zip(&before) {
            let seen: Vec<_> = pages.iter().flat_map(keys).collect();
            let distinct: std::collections::BTreeSet<_> =
                seen.iter().map(Json::to_string).collect();
            assert_eq!((seen.len(), distinct.len()), (12, 12), "{source}");
            assert!(pages.iter().all(|p| p["count"] == "0xc"));
            assert!(pages.iter().all(|p| p["blockNumber"] == pinned.as_str()));
        }
        // Between the pages the head moves on: an entity (sorting first) is
        // created and one from the first page is deleted.
        let doomed = keys(&before[0][0])[0].clone();
        submit(
            &mut host,
            &mut nonce,
            vec![
                create(3000, vec![("p", arkiv_db::Cell::attribute(Value::U64(9)))]),
                delete(&doomed),
            ],
        );
        host.produce().unwrap();
        host.produce().unwrap();
        // Pinned to page one's block, every offset page is unchanged.
        for (source, before) in sources.iter().zip(&before) {
            assert_eq!(&pages(&host, source, Some(&pinned)), before, "{source}");
        }
        // At the head the same offsets have shifted: rows repeat or are
        // missed across pages read before and after a write.
        for (source, before) in sources.iter().zip(&before) {
            let head = pages(&host, source, None);
            assert_eq!(head[0]["count"], "0xc");
            assert_ne!(
                head.iter().flat_map(keys).collect::<Vec<_>>(),
                before.iter().flat_map(keys).collect::<Vec<_>>(),
                "{source}"
            );
        }
        // An unsorted offset page at the pinned block also continues by
        // cursor, from the same snapshot, past later writes.
        for source in &sources[..2] {
            let first = query(
                &host,
                &json!([
                    source,
                    {"limit": 4, "offset": 4, "atBlock": pinned, "select": select(false)},
                ]),
            )
            .unwrap();
            let (rest, _) = walk(
                &host,
                source,
                json!({"limit": 4, "offset": 4, "atBlock": pinned, "select": select(false)}),
            );
            assert_eq!(keys(&first), rest[..4]);
            let reference: Vec<_> = before[sources.iter().position(|s| s == source).unwrap()]
                .iter()
                .flat_map(keys)
                .collect();
            assert_eq!(rest, reference[4..], "{source}");
        }
    }

    #[test]
    fn timing_uses_numeric_height_and_observed_whole_seconds() {
        let mut host = fixture_with_period(1_500);
        let timing = |host: &Host| {
            dispatch(host, "arkiv_getBlockTiming", &json!([]))
                .unwrap()
                .unwrap()
        };
        assert_eq!(
            timing(&host),
            json!({"current_block":0,"current_block_time":1,"duration":0})
        );
        host.produce().unwrap();
        assert_eq!(
            timing(&host),
            json!({"current_block":1,"current_block_time":2,"duration":1})
        );
        host.produce().unwrap();
        assert_eq!(
            timing(&host),
            json!({"current_block":2,"current_block_time":4,"duration":2})
        );
    }
}
