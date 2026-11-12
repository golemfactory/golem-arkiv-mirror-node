//! Refusals of SDK reads: the SDK's query error table beside node errors.

use super::super::{Error, ErrorCode};
use serde_json::{Value as Json, json};

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
    pub(super) fn at(
        kind: QueryErrorKind,
        source: &str,
        position: usize,
        message: impl Into<String>,
    ) -> Self {
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

impl From<ErrorCode> for ReadError {
    fn from(code: ErrorCode) -> Self {
        Self::Node(code.into())
    }
}

impl From<QueryError> for ReadError {
    fn from(error: QueryError) -> Self {
        Self::Query(error)
    }
}

pub(super) type Read<T> = std::result::Result<T, ReadError>;

pub(super) fn limits(message: &str) -> QueryError {
    QueryError::new(QueryErrorKind::Limits, message)
}

pub(super) fn cursor_mismatch() -> ReadError {
    QueryError::new(
        QueryErrorKind::Cursor,
        "the cursor expired, is malformed, or belongs to another query, block or selection",
    )
    .into()
}

pub(super) fn unknown_block(height: u64, head: u64) -> ReadError {
    QueryError::new(
        QueryErrorKind::Block,
        format!("block {height} is above the head {head}"),
    )
    .into()
}
