//! The SDK query language compiled to a native [`Query`] in bounded
//! disjunctive normal form.

use super::{
    GROUP_CAP, LITERAL_CAP, Order, Statement,
    error::{QueryError, QueryErrorKind, limits},
};
use crate::arkiv;
use crate::simulator::query_language::{
    ast::{AnnotKey, AttributeValue, BuiltIn, Query as Ast},
    error::{ParseError, ParseErrorKind},
    lexer::{self, SpannedToken, Token},
    parse,
};
use alloy_primitives::U256;
use arkiv_db::{CellName, Literal, Query, Term, TypeTag, Value};
use std::ops::Bound;

const QUERY_BYTES: usize = 8192;
const MAX_DEPTH: usize = 32;
const MAX_PREDICATES: usize = 64;

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

pub(super) const TAGS: &[(&str, TypeTag)] = &[
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
pub(super) fn parse_query(source: &str) -> std::result::Result<Statement, QueryError> {
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
