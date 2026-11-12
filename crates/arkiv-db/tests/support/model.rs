//! The reference for typed records: a `BTreeMap` model of one namespace's
//! live records and a full-scan evaluator of the query semantics. It never
//! consults the engine or its index.

use std::{
    collections::{BTreeMap, HashSet},
    ops::Bound,
};

use arkiv_db::{
    Cell, CellName, CellSelect, Projection, Query, RecordId, RecordKey, Row, Term, TypeTag, Value,
};

use super::vocab::{key, rid};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    pub key: RecordKey,
    pub cells: BTreeMap<CellName, Cell>,
}

/// The live records of one namespace. Ids are minted densely from 1 and
/// never reused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Model {
    next: u64,
    live: BTreeMap<RecordId, Record>,
    keys: BTreeMap<RecordKey, RecordId>,
}

impl Model {
    pub fn new() -> Self {
        Self {
            next: 1,
            live: BTreeMap::new(),
            keys: BTreeMap::new(),
        }
    }

    /// Mints the next id and binds `key`, which must not be live.
    pub fn create(&mut self, key: impl AsRef<[u8]>, cells: &[(CellName, Cell)]) -> RecordId {
        let key = self::key(key);
        assert!(!self.keys.contains_key(&key), "model: {key:?} is bound");
        let id = rid(self.next);
        self.next += 1;
        self.keys.insert(key.clone(), id);
        let cells = cells.iter().cloned().collect();
        self.live.insert(id, Record { key, cells });
        id
    }

    /// Replaces or adds the set cells, then drops the removed names.
    pub fn patch(
        &mut self,
        key: impl AsRef<[u8]>,
        set: &[(CellName, Cell)],
        remove: &[CellName],
    ) -> RecordId {
        let id = self.id(key).expect("model: patch of a bound key");
        let cells = &mut self.live.get_mut(&id).expect("bound keys are live").cells;
        cells.extend(set.iter().cloned());
        for name in remove {
            cells.remove(name);
        }
        id
    }

    pub fn delete(&mut self, key: impl AsRef<[u8]>) -> RecordId {
        let id = self
            .keys
            .remove(&self::key(key))
            .expect("model: delete of a bound key");
        self.live.remove(&id);
        id
    }

    pub fn id(&self, key: impl AsRef<[u8]>) -> Option<RecordId> {
        self.keys.get(&self::key(key)).copied()
    }

    pub fn record(&self, id: RecordId) -> Option<&Record> {
        self.live.get(&id)
    }

    /// The live records in ascending id order.
    pub fn records(&self) -> impl Iterator<Item = (RecordId, &Record)> {
        self.live.iter().map(|(&id, record)| (id, record))
    }

    pub fn len(&self) -> usize {
        self.live.len()
    }

    pub fn row(&self, key: impl AsRef<[u8]>, projection: &Projection) -> Option<Row> {
        let id = self.id(key)?;
        Some(project(id, &self.live[&id], projection))
    }

    /// Every matching live id in ascending order.
    pub fn ids(&self, query: &Query) -> Vec<RecordId> {
        self.records()
            .filter(|(_, record)| query_holds(query, &record.cells))
            .map(|(id, _)| id)
            .collect()
    }

    /// Every matching live record in ascending id order, projected.
    pub fn evaluate(&self, query: &Query, projection: &Projection) -> Vec<Row> {
        self.records()
            .filter(|(_, record)| query_holds(query, &record.cells))
            .map(|(id, record)| project(id, record, projection))
            .collect()
    }

    /// Distinct value terms plus distinct existence terms over the attribute
    /// cells of the live records.
    pub fn term_count(&self) -> usize {
        let mut terms: HashSet<(&CellName, TypeTag, Option<&Value>)> = HashSet::new();
        for record in self.live.values() {
            for (name, cell) in &record.cells {
                if cell.is_attribute() {
                    terms.insert((name, cell.value.tag(), Some(&cell.value)));
                    terms.insert((name, cell.value.tag(), None));
                }
            }
        }
        terms.len()
    }
}

/// The row a projection returns for a live record. A named selection returns
/// only the names the record has.
pub fn project(id: RecordId, record: &Record, projection: &Projection) -> Row {
    let cells = record.cells.iter();
    let cells = match &projection.cells {
        CellSelect::None => Vec::new(),
        CellSelect::All => cells.map(|(n, c)| (n.clone(), c.clone())).collect(),
        CellSelect::Named(names) => cells
            .filter(|(name, _)| names.contains(*name))
            .map(|(n, c)| (n.clone(), c.clone()))
            .collect(),
    };
    Row {
        id,
        key: projection.key.then(|| record.key.clone()),
        cells,
    }
}

/// Query = OR of groups; group = AND of literals; an empty group matches
/// every live record; a negated literal is the complement.
pub fn query_holds(query: &Query, cells: &BTreeMap<CellName, Cell>) -> bool {
    query.groups.iter().any(|group| {
        group
            .iter()
            .all(|literal| term_holds(&literal.term, cells) != literal.negated)
    })
}

/// A term holds only on an attribute cell of the term's name and type;
/// fields and cells of another type count as absent.
pub fn term_holds(term: &Term, cells: &BTreeMap<CellName, Cell>) -> bool {
    let Some(cell) = cells.get(term.name()) else {
        return false;
    };
    if !cell.is_attribute() {
        return false;
    }
    let held = &cell.value;
    match term {
        Term::Eq { value, .. } => held.tag() == value.tag() && held == value,
        Term::NotEq { value, .. } => held.tag() == value.tag() && held != value,
        Term::Range { lower, upper, .. } => within(held, lower, upper),
        Term::StartsWith { prefix, .. } => match held {
            Value::Str(text) => text.as_bytes().starts_with(prefix.as_bytes()),
            _ => false,
        },
        Term::Exists { tag, .. } => held.tag() == *tag,
    }
}

/// The type and the widened magnitude of an `I64` or `U64`.
fn ordered(value: &Value) -> Option<(TypeTag, i128)> {
    match value {
        Value::I64(number) => Some((TypeTag::I64, i128::from(*number))),
        Value::U64(number) => Some((TypeTag::U64, i128::from(*number))),
        _ => None,
    }
}

/// Whether an `I64` or `U64` lies within bounds of its own type.
pub fn within(held: &Value, lower: &Bound<Value>, upper: &Bound<Value>) -> bool {
    let Some((tag, held)) = ordered(held) else {
        return false;
    };
    let same_type = |bound: &Value| {
        ordered(bound)
            .filter(|(bound_tag, _)| *bound_tag == tag)
            .map(|(_, magnitude)| magnitude)
    };
    let above = match lower {
        Bound::Unbounded => true,
        Bound::Included(bound) => same_type(bound).is_some_and(|bound| held >= bound),
        Bound::Excluded(bound) => same_type(bound).is_some_and(|bound| held > bound),
    };
    let below = match upper {
        Bound::Unbounded => true,
        Bound::Included(bound) => same_type(bound).is_some_and(|bound| held <= bound),
        Bound::Excluded(bound) => same_type(bound).is_some_and(|bound| held < bound),
    };
    above && below
}

/// Canonical encoded length of a value: the type's fixed width, or the byte
/// length for `Str` and `Bytes`.
pub fn encoded_len(value: &Value) -> u64 {
    match value {
        Value::Bool(_) => 1,
        Value::I32(_) | Value::Bytes4(_) | Value::U32(_) | Value::Dec32(_) => 4,
        Value::F32(_) | Value::Date32(_) => 4,
        Value::I64(_) | Value::U64(_) | Value::Bytes8(_) | Value::Dec64(_) => 8,
        Value::F64(_) | Value::Timestamp64(_) => 8,
        Value::Bytes16(_) | Value::U128(_) | Value::I128(_) | Value::Dec128(_) => 16,
        Value::Addr(_) => 20,
        Value::U256(_) | Value::Dec(_) | Value::Bytes32(_) | Value::Key(_) | Value::I256(_) => 32,
        Value::Str(text) => text.len() as u64,
        Value::Bytes(bytes) => bytes.len() as u64,
    }
}

/// Name bytes plus encoded value bytes: what `CellByte` and `ReadByte` count.
pub fn cell_bytes(name: &CellName, cell: &Cell) -> u64 {
    name.len() as u64 + encoded_len(&cell.value)
}
