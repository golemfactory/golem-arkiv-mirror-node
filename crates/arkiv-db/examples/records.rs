//! The record flow: typed records with attributes and fields written in
//! metered blocks, queries over the attribute indexes (equality, range,
//! prefix, inequality and negation, AND, OR and projection), a patch that
//! moves a record between type indexes, a keyset walk pinned to one snapshot
//! while a later block publishes, a historical query through the host's block
//! list, and host-driven expiry in the block where the records expire.

use std::ops::Bound::{Included, Unbounded};

use arkiv_db::{
    BlockNumber, Budget, Cell, CellName, Cost, Effect, Engine, Literal, OwnerId, PriceSchedule,
    Projection, Query, QueryPage, Receipt, RecordKey, Rejection, Row, Term, TypeTag, Value,
    WriteOp, expire_records,
};

const ALICE: OwnerId = OwnerId::new([0xa1; 20]);

const BLOCK_BUDGET: Cost = Cost::new(2_000);
/// Per-operation budgets: writes are capped, reads run to completion.
const WRITE: Budget = Budget::limit(500);
const READ: Budget = Budget::unlimited();

fn name(text: &str) -> CellName {
    CellName::new(text).expect("a valid cell name")
}

fn key(text: &str) -> RecordKey {
    RecordKey::new(text.as_bytes()).expect("a valid key")
}

fn show(label: &str, receipt: &Receipt) {
    println!("{label}: {} ({:?})", receipt.spent, receipt.outcome);
}

fn ids(page: &QueryPage) -> Vec<u64> {
    page.rows.iter().map(|row| row.id.ordinal()).collect()
}

fn show_value(value: &Value) -> String {
    match value {
        Value::Bool(flag) => flag.to_string(),
        Value::I64(number) => format!("{number}i64"),
        Value::U64(number) => format!("{number}u64"),
        Value::Str(text) => format!("{text:?}"),
        Value::Bytes(bytes) => format!("{} bytes", bytes.len()),
        Value::I32(n) => n.to_string(),
        value => format!("{value:?}"),
    }
}

/// `rec:n key [name=value ...]`; attributes use `=`, fields use `~`.
fn describe(row: &Row) -> String {
    let key = row.key.as_ref().map_or_else(
        || "-".to_owned(),
        |key| String::from_utf8_lossy(key.as_bytes()).into_owned(),
    );
    let cells: Vec<String> = row
        .cells
        .iter()
        .map(|(name, cell)| {
            let sign = if cell.is_attribute() { '=' } else { '~' };
            format!("{name}{sign}{}", show_value(&cell.value))
        })
        .collect();
    format!("{} {key} [{}]", row.id, cells.join(" "))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut engine = Engine::new(PriceSchedule::FIXTURE);
    // The host maps block numbers to snapshots: block `n` is `blocks[n]`.
    let mut blocks = vec![engine.head()];

    // `$expiresAt` is an ordinary U64 attribute to the engine; only the host
    // gives it a meaning.
    let kind = name("kind");
    let price = name("price");
    let expires_at = name("$expiresAt");
    let note = name("note");

    // Block 1: alice creates the shop.
    let mut branch = engine.begin(engine.head(), Some(BLOCK_BUDGET))?;
    let created = branch.execute(
        ALICE,
        WriteOp::CreateNamespace {
            name: arkiv_db::NamespaceName::new("shop")?,
        },
        WRITE,
    )?;
    let Effect::NamespaceCreated(shop) = created.effect else {
        unreachable!()
    };
    show("block 1: create namespace", &created.receipt);
    blocks.push(engine.publish(&branch.seal())?);

    // Block 2: four records, each with three attributes (kind: Str,
    // price: I64, $expiresAt: U64) and one field (note: Bytes). A create
    // costs more for every index term that no record holds yet.
    let catalog: [(&str, &str, i64, u64); 4] = [
        // key, kind, price, $expiresAt
        ("sku/1", "book", 10, 5),
        ("sku/2", "book", 20, 5),
        ("sku/3", "pen", 5, 5),
        ("sku/4", "pencil", 7, 9),
    ];
    let mut branch = engine.begin(engine.head(), Some(BLOCK_BUDGET))?;
    for &(sku, kind_of, price_of, expiry) in &catalog {
        let create = WriteOp::CreateRecord {
            namespace: shop,
            key: key(sku),
            cells: vec![
                (kind.clone(), Cell::attribute(Value::str(kind_of))),
                (price.clone(), Cell::attribute(Value::I64(price_of))),
                (expires_at.clone(), Cell::attribute(Value::U64(expiry))),
                (note.clone(), Cell::field(Value::bytes(b"xyz"))),
            ],
        };
        let executed = branch.execute(ALICE, create, WRITE)?;
        let Effect::RecordCreated { id } = executed.effect else {
            unreachable!()
        };
        show(&format!("block 2: create {sku} as {id}"), &executed.receipt);
    }
    let sealed = branch.seal();
    println!("block 2: aggregate write cost {}", sealed.spent());
    blocks.push(engine.publish(&sealed)?);
    let after_block_2 = engine.head_snapshot();

    // Queries against the state after block 2.

    // Eq reads the posting of the value term kind="book".
    let books = Query::term(Term::eq(kind.clone(), Value::str("book")));
    let page = after_block_2.query(shop, &books, &Projection::ALL, 10, None, READ)?;
    show("query kind == \"book\", all cells", &page.receipt);
    assert_eq!(ids(&page.value), [1, 2]);
    assert!(page.value.next.is_none(), "no further match, so no cursor");
    for row in &page.value.rows {
        println!("  {}", describe(row));
    }

    // Range visits every term of (price, I64) inside the bounds in key order.
    let cheap = Query::term(Term::range(
        price.clone(),
        Included(Value::I64(5)),
        Included(Value::I64(10)),
    ));
    let page = after_block_2.query(shop, &cheap, &Projection::ID_ONLY, 10, None, READ)?;
    show("query 5 <= price <= 10, ids only", &page.receipt);
    assert_eq!(ids(&page.value), [1, 3, 4]);

    // StartsWith visits the terms "pen" and "pencil".
    let pens = Query::term(Term::starts_with(kind.clone(), "pen"));
    let page = after_block_2.query(shop, &pens, &Projection::KEY_ONLY, 10, None, READ)?;
    show("query kind starts with \"pen\", keys", &page.receipt);
    assert_eq!(ids(&page.value), [3, 4]);
    for row in &page.value.rows {
        println!("  {}", describe(row));
    }

    // NotEq reads the existence term of (kind, Str) and probes each
    // candidate against the literal.
    let not_books = Query::term(Term::not_eq(kind.clone(), Value::str("book")));
    let page = after_block_2.query(shop, &not_books, &Projection::ID_ONLY, 10, None, READ)?;
    show("query kind != \"book\", ids only", &page.receipt);
    assert_eq!(ids(&page.value), [3, 4]);

    // A negated first literal has no index to seek: it walks the row map and
    // probes each row.
    let not_pens = Query::all(vec![Literal::not(Term::starts_with(kind.clone(), "pen"))]);
    let page = after_block_2.query(shop, &not_pens, &Projection::ID_ONLY, 10, None, READ)?;
    show("query NOT kind starts with \"pen\"", &page.receipt);
    assert_eq!(ids(&page.value), [1, 2]);

    // AND: the first literal drives, the second is a probe on each
    // candidate. The row projects the key and the price cell only.
    let dear_books = Query::all(vec![
        Literal::of(Term::eq(kind.clone(), Value::str("book"))),
        Literal::of(Term::range(
            price.clone(),
            Included(Value::I64(15)),
            Unbounded,
        )),
    ]);
    let key_and_price = Projection::named(true, [price.clone()]);
    let page = after_block_2.query(shop, &dear_books, &key_and_price, 10, None, READ)?;
    show(
        "query kind == \"book\" AND price >= 15, key + price",
        &page.receipt,
    );
    assert_eq!(ids(&page.value), [2]);
    println!("  {}", describe(&page.value.rows[0]));

    // OR: two groups merged in id order.
    let pens_or_twenty = Query {
        groups: vec![
            vec![Literal::of(Term::eq(kind.clone(), Value::str("pen")))],
            vec![Literal::of(Term::eq(price.clone(), Value::I64(20)))],
        ],
    };
    let page = after_block_2.query(shop, &pens_or_twenty, &Projection::ID_ONLY, 10, None, READ)?;
    show("query kind == \"pen\" OR price == 20", &page.receipt);
    assert_eq!(ids(&page.value), [2, 3]);

    // count runs the same enumeration without emitting rows.
    let has_expiry = Query::term(Term::exists(expires_at.clone(), TypeTag::U64));
    let counted = after_block_2.count(shop, &has_expiry, READ)?;
    show("count records with a u64 $expiresAt", &counted.receipt);
    assert_eq!(counted.value, 4);

    // Block 3: a patch changes the type of sku/1's price from I64 to U64 and
    // drops its note. The record leaves the (price, I64) index and joins a
    // new (price, U64) index.
    let mut branch = engine.begin(engine.head(), Some(BLOCK_BUDGET))?;
    let patch = WriteOp::PatchRecord {
        namespace: shop,
        key: key("sku/1"),
        set: vec![(price.clone(), Cell::attribute(Value::U64(10)))],
        remove: vec![note],
    };
    let patched = branch.execute(ALICE, patch, WRITE)?;
    show(
        "block 3: patch sku/1 price := 10u64, remove note",
        &patched.receipt,
    );

    // Still inside the branch: the I64 range no longer finds sku/1, and the
    // U64 existence term holds exactly the patched record.
    let page = branch.query(shop, &cheap, &Projection::ID_ONLY, 10, None, READ)?;
    show("block 3: 5 <= price <= 10 after the patch", &page.receipt);
    assert_eq!(ids(&page.value), [3, 4]);
    let u64_prices = Query::term(Term::exists(price.clone(), TypeTag::U64));
    let page = branch.query(shop, &u64_prices, &Projection::ID_ONLY, 10, None, READ)?;
    show("block 3: records with a u64 price", &page.receipt);
    assert_eq!(ids(&page.value), [1]);
    blocks.push(engine.publish(&branch.seal())?);

    // Keyset walk pinned to block 3 while block 4 publishes.
    let pinned = engine.head_snapshot();
    let everything = Query::match_all();
    let page_1 = pinned.query(shop, &everything, &Projection::KEY_ONLY, 2, None, READ)?;
    show("walk page 1 at block 3", &page_1.receipt);
    assert_eq!(ids(&page_1.value), [1, 2]);
    let cursor = page_1.value.next.expect("records 3 and 4 follow");
    println!(
        "  cursor after {} pinned to {}",
        cursor.after(),
        pinned.id()
    );

    // Block 4 publishes a fifth record in the meantime.
    let mut branch = engine.begin(engine.head(), Some(BLOCK_BUDGET))?;
    let create = WriteOp::CreateRecord {
        namespace: shop,
        key: key("sku/5"),
        cells: vec![
            (kind, Cell::attribute(Value::str("book"))),
            (price.clone(), Cell::attribute(Value::I64(12))),
            (expires_at.clone(), Cell::attribute(Value::U64(7))),
        ],
    };
    let executed = branch.execute(ALICE, create, WRITE)?;
    show("block 4: create sku/5", &executed.receipt);
    blocks.push(engine.publish(&branch.seal())?);

    // Page 2 continues from the pinned snapshot with a different limit (the
    // limit is not bound by the cursor) and does not see the fifth record.
    let page_2 = pinned.query(
        shop,
        &everything,
        &Projection::KEY_ONLY,
        10,
        Some(&cursor),
        READ,
    )?;
    show(
        "walk page 2 at block 3 (after block 4 published)",
        &page_2.receipt,
    );
    assert_eq!(ids(&page_2.value), [3, 4]);
    assert!(page_2.value.next.is_none());

    // The same cursor presented to the new head is refused before any charge.
    let head = engine.head_snapshot();
    let mismatch = head
        .query(
            shop,
            &everything,
            &Projection::KEY_ONLY,
            10,
            Some(&cursor),
            READ,
        )
        .expect_err("the cursor is bound to the block-3 snapshot");
    println!("walk at head with the old cursor: {mismatch}");
    assert_eq!(mismatch.reason, Rejection::CursorMismatch);

    // A fresh count at head sees all five rows.
    let counted = head.count(shop, &everything, READ)?;
    show("count everything at head", &counted.receipt);
    assert_eq!(counted.value, 5);

    // History: at block 2 sku/1 still had an I64 price and its note.
    let at_block_2 = engine.snapshot(blocks[2])?;
    let page = at_block_2.query(shop, &cheap, &Projection::ID_ONLY, 10, None, READ)?;
    show("history: 5 <= price <= 10 at block 2", &page.receipt);
    assert_eq!(ids(&page.value), [1, 3, 4]);
    let page = head.query(shop, &cheap, &Projection::ID_ONLY, 10, None, READ)?;
    show("history: 5 <= price <= 10 at head", &page.receipt);
    assert_eq!(ids(&page.value), [3, 4]);

    let then = at_block_2.get_record(shop, &key("sku/1"), &Projection::ALL, READ)?;
    show("history: get sku/1 at block 2", &then.receipt);
    let now = head.get_record(shop, &key("sku/1"), &Projection::ALL, READ)?;
    show("history: get sku/1 at head", &now.receipt);
    println!(
        "  block 2: {}",
        describe(&then.value.expect("live at block 2"))
    );
    println!("  head:    {}", describe(&now.value.expect("live at head")));

    // Block 5: host expiry. The engine has no clock; the host deletes, with
    // host authority, every record whose $expiresAt is at most the block
    // number, in the block where it expires. Records 1, 2 and 3 carry 5;
    // records 4 (9) and 5 (7) stay. The expiry query is a read and never
    // enters the block's receipts.
    let mut branch = engine.begin(engine.head(), Some(BLOCK_BUDGET))?;
    let expired = expire_records(&mut branch, shop, &expires_at, BlockNumber(5), READ, WRITE)?;
    for (id, receipt) in &expired {
        show(&format!("block 5: expire {id}"), receipt);
    }
    assert_eq!(branch.receipts().len(), expired.len());
    blocks.push(engine.publish(&branch.seal())?);

    // Snapshot B-1 still has the expired records; snapshot B does not.
    let before = engine.snapshot(blocks[4])?;
    let after = engine.snapshot(blocks[5])?;
    assert_eq!(before.typed_record_count(shop), Some(5));
    assert_eq!(after.typed_record_count(shop), Some(2));
    let gone = after.get_record(shop, &key("sku/1"), &Projection::KEY_ONLY, READ)?;
    show("block 5: get sku/1", &gone.receipt);
    assert!(gone.value.is_none());
    let survivors = after.query(shop, &everything, &Projection::ALL, 10, None, READ)?;
    assert_eq!(ids(&survivors.value), [4, 5]);
    for row in &survivors.value.rows {
        println!("  {}", describe(row));
    }

    println!(
        "blocks 0..=5 map to snapshots {:?}",
        blocks.iter().map(|id| id.sequence()).collect::<Vec<_>>()
    );
    Ok(())
}
