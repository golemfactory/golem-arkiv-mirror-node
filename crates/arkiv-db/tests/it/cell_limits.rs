//! Configurable cell limits (golem-db's genesis `cell_limits`): the cell
//! name, `Str`, `Bytes` and indexed-string limits are part of [`Limits`],
//! enforced at admission, persisted in a durable genesis and checked when
//! the database is reopened.

use std::{ops::Bound, sync::Arc};

use arkiv_db::{
    ArgumentFault, Branch, Budget, Cell, CellName, Engine, Limits, PriceSchedule, Projection,
    Query, QueryFault, Rejection, StorageErrorKind, Term, TypeTag, Value, decode_exact,
    durable::{DurableEngine, DurableError, EngineGenesis, Initialization, Options},
    encode,
};
use arkiv_db_memory::MemoryVersionedStore;

use crate::support::vocab::{
    ALICE, attr, begin, create_record, engine_with_namespace, field, name, ns, patch_record,
    publish,
};

/// golem-db's facade test limits, with shorter names and indexed strings.
const SMALL: Limits = Limits {
    max_cell_name_len: 10,
    max_str_len: 64,
    max_bytes_len: 128,
    max_indexed_len: 16,
    ..Limits::DEFAULT
};

fn try_create(
    branch: &mut Branch,
    key: &[u8],
    cells: Vec<(CellName, Cell)>,
) -> Result<(), Rejection> {
    branch
        .execute(ALICE, create_record(ns(1), key, cells), Budget::unlimited())
        .map(|_| ())
        .map_err(|rejected| rejected.reason)
}

fn kind(error: DurableError) -> StorageErrorKind {
    match error {
        DurableError::Storage(error) => error.kind,
        other => panic!("not a storage error: {other:?}"),
    }
}

fn invalid(fault: ArgumentFault) -> Result<(), Rejection> {
    Err(Rejection::InvalidArgument(fault))
}

#[test]
fn defaults_are_explicit_and_below_the_caps() {
    assert_eq!(Limits::DEFAULT.max_cell_name_len, CellName::MAX_LEN);
    // The 128-byte indexed-string profile is admission policy, separate
    // from the representation ceiling.
    assert_eq!(Limits::DEFAULT.max_indexed_len, Limits::DEFAULT_INDEXED_LEN);
    assert_eq!(
        (
            CellName::MAX_LEN,
            Limits::DEFAULT_INDEXED_LEN,
            Value::INDEXED_LEN_CEILING
        ),
        (32, 128, 512)
    );
    assert_eq!(Limits::DEFAULT.max_str_len, 4096);
    assert_eq!(Limits::DEFAULT.max_bytes_len, 4096);
    assert_eq!(Limits::DEFAULT.validate(), Ok(()));
    assert_eq!(SMALL.validate(), Ok(()));
    assert_eq!(SMALL.indexed_value_len(), 16);
    // The indexed bound never exceeds the Str limit.
    let tight = Limits {
        max_str_len: 8,
        ..SMALL
    };
    assert_eq!(tight.indexed_value_len(), 8);
}

#[test]
fn limits_above_the_caps_are_invalid() {
    for (limits, reason) in [
        (
            Limits {
                max_cell_name_len: 0,
                ..Limits::DEFAULT
            },
            "max_cell_name_len must be positive",
        ),
        (
            Limits {
                max_cell_name_len: CellName::MAX_LEN + 1,
                ..Limits::DEFAULT
            },
            "max_cell_name_len exceeds the cell-name grammar cap (32)",
        ),
        (
            Limits {
                max_indexed_len: Value::INDEXED_LEN_CEILING + 1,
                ..Limits::DEFAULT
            },
            "max_indexed_len exceeds the indexed-string ceiling (512)",
        ),
    ] {
        assert_eq!(limits.validate(), Err(reason));
        assert!(
            std::panic::catch_unwind(|| Engine::with_limits(PriceSchedule::FIXTURE, limits))
                .is_err()
        );
        let mut genesis = EngineGenesis::new([1; 32], PriceSchedule::FIXTURE);
        genesis.limits = limits;
        assert_eq!(
            kind(genesis.validate().unwrap_err()),
            StorageErrorKind::Incompatible
        );
    }
    // Zero is a valid string, bytes or indexed limit, as in golem-db.
    let zero = Limits {
        max_str_len: 0,
        max_bytes_len: 0,
        max_indexed_len: 0,
        ..Limits::DEFAULT
    };
    assert_eq!(zero.validate(), Ok(()));
}

#[test]
fn names_longer_than_the_limit_are_rejected_in_every_write() {
    let (engine, _) = engine_with_namespace(SMALL);
    let mut branch = begin(&engine, None);
    let ok = name(&"n".repeat(10));
    let long = name(&"n".repeat(11));
    assert_eq!(
        try_create(&mut branch, b"a", vec![(ok, field(Value::Bool(true)))]),
        Ok(())
    );
    assert_eq!(
        try_create(
            &mut branch,
            b"b",
            vec![(long.clone(), field(Value::Bool(true)))]
        ),
        invalid(ArgumentFault::NameTooLong)
    );
    for (set, remove) in [
        (vec![(long.clone(), attr(Value::U64(1)))], vec![]),
        (vec![], vec![long]),
    ] {
        let result = branch.execute(
            ALICE,
            patch_record(ns(1), b"a", set, remove),
            Budget::unlimited(),
        );
        assert_eq!(
            result.map(|_| ()).map_err(|rejected| rejected.reason),
            invalid(ArgumentFault::NameTooLong)
        );
    }
    // The name limit counts the `$` prefix, as golem-db does.
    assert_eq!(
        try_create(
            &mut branch,
            b"c",
            vec![(
                name(&format!("${}", "n".repeat(10))),
                field(Value::Bool(true))
            )]
        ),
        invalid(ArgumentFault::NameTooLong)
    );
}

#[test]
fn str_bytes_and_indexed_strings_have_separate_limits() {
    let (engine, _) = engine_with_namespace(SMALL);
    let mut branch = begin(&engine, None);
    let cases = [
        (field(Value::str(&"s".repeat(64))), Ok(())),
        (
            field(Value::str(&"s".repeat(65))),
            invalid(ArgumentFault::ValueTooLarge),
        ),
        (field(Value::bytes(&[1; 128])), Ok(())),
        (
            field(Value::bytes(&[1; 129])),
            invalid(ArgumentFault::ValueTooLarge),
        ),
        (attr(Value::str(&"s".repeat(16))), Ok(())),
        (
            attr(Value::str(&"s".repeat(17))),
            invalid(ArgumentFault::ValueTooLarge),
        ),
        // A 17-byte string is a valid field: only indexing is bounded.
        (field(Value::str(&"s".repeat(17))), Ok(())),
        // Fixed-width types have no configurable limit.
        (attr(Value::Bytes32([1; 32])), Ok(())),
    ];
    for (index, (cell, expected)) in cases.into_iter().enumerate() {
        assert_eq!(
            try_create(
                &mut branch,
                &[index as u8 + 1],
                vec![(name("c"), cell.clone())]
            ),
            expected,
            "{cell:?}"
        );
    }
}

#[test]
fn default_limits_bound_indexed_strings_and_admit_fixed_width_scalars() {
    let (engine, _) = engine_with_namespace(Limits::DEFAULT);
    let mut branch = begin(&engine, None);
    let indexed = |len: usize| Value::str(&"a".repeat(len));
    let cases = [
        (indexed(Limits::DEFAULT_INDEXED_LEN - 1), Ok(())),
        (indexed(Limits::DEFAULT_INDEXED_LEN), Ok(())),
        (
            indexed(Limits::DEFAULT_INDEXED_LEN + 1),
            invalid(ArgumentFault::ValueTooLarge),
        ),
        (Value::I32(1), Ok(())),
        (Value::Bytes32([1; 32]), Ok(())),
        (Value::Bytes16([1; 16]), Ok(())),
        (Value::f64(1.0).unwrap(), Ok(())),
        (Value::Timestamp64(1), Ok(())),
    ];
    for (index, (value, expected)) in cases.into_iter().enumerate() {
        assert_eq!(
            try_create(
                &mut branch,
                &[index as u8 + 1],
                vec![(name("v"), attr(value.clone()))]
            ),
            expected,
            "{value:?}"
        );
    }
}

#[test]
fn query_strings_obey_the_indexed_limit() {
    let (mut engine, _) = engine_with_namespace(SMALL);
    let mut branch = begin(&engine, None);
    try_create(
        &mut branch,
        b"a",
        vec![(name("c"), attr(Value::str("abc")))],
    )
    .unwrap();
    publish(&mut engine, branch);
    let snapshot = engine.head_snapshot();
    let long = "x".repeat(17);
    for term in [
        Term::eq(name("c"), Value::str(&long)),
        Term::starts_with(name("c"), &long),
        Term::range(
            name("c"),
            Bound::Included(Value::str(&long)),
            Bound::Unbounded,
        ),
    ] {
        let rejected = snapshot
            .query(
                ns(1),
                &Query::term(term),
                &Projection::ID_ONLY,
                5,
                None,
                Budget::unlimited(),
            )
            .unwrap_err();
        assert_eq!(
            rejected.reason,
            Rejection::InvalidQuery(QueryFault::ValueTooLarge)
        );
    }
    let rejected = snapshot
        .terms(
            ns(1),
            &name("c"),
            TypeTag::Str,
            Bound::Included(Value::str(&long)),
            5,
            Budget::unlimited(),
        )
        .unwrap_err();
    assert_eq!(
        rejected.reason,
        Rejection::InvalidQuery(QueryFault::ValueTooLarge)
    );
}

#[test]
fn durable_genesis_persists_the_limits_and_rejects_a_mismatch() {
    let store = Arc::new(MemoryVersionedStore::new());
    let mut genesis = EngineGenesis::new([5; 32], PriceSchedule::FIXTURE);
    genesis.limits = SMALL;
    assert_eq!(
        decode_exact::<EngineGenesis>(&encode(&genesis)).unwrap(),
        genesis
    );
    let Initialization::Committed(engine) =
        DurableEngine::initialize(store.clone(), genesis.clone(), Options::default()).unwrap()
    else {
        panic!("initialization commits");
    };
    assert_eq!(engine.genesis().limits, SMALL);
    drop(engine);
    let reopened = DurableEngine::open(store.clone(), genesis.clone(), Options::default()).unwrap();
    assert_eq!(reopened.genesis().limits, SMALL);
    drop(reopened);
    for changed in [
        Limits {
            max_cell_name_len: 11,
            ..SMALL
        },
        Limits {
            max_str_len: 65,
            ..SMALL
        },
        Limits {
            max_bytes_len: 129,
            ..SMALL
        },
        Limits {
            max_indexed_len: 17,
            ..SMALL
        },
    ] {
        let mut other = genesis.clone();
        other.limits = changed;
        let error = DurableEngine::open(store.clone(), other, Options::default()).unwrap_err();
        assert_eq!(kind(error), StorageErrorKind::Incompatible, "{changed:?}");
    }
}
