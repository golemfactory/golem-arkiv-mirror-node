//! The cursor walk: every page of a query, with the paging contract checked
//! on the way.

use arkiv_db::{Budget, Cursor, NamespaceId, Projection, Query, QueryPage, Receipt, Row};

use super::reader::Reader;

/// One page as the engine returned it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Page {
    pub rows: Vec<Row>,
    pub next: Option<Cursor>,
    pub receipt: Receipt,
}

pub fn rows_of(pages: &[Page]) -> Vec<Row> {
    pages
        .iter()
        .flat_map(|page| page.rows.iter().cloned())
        .collect()
}

/// Pages `query` from `reader` to the end, `limit_of(page index)` rows per
/// page, calling `between(rows so far)` after every page that issued a
/// continuation. Checks the cursor invariants of every page: rows ascend and
/// exceed the cursor, a continued page is never empty, a continuation is
/// issued only for an exactly full page and names the last row, and it is
/// bound to this reader's snapshot, namespace, query and projection.
pub fn walk(
    reader: &impl Reader,
    namespace: NamespaceId,
    query: &Query,
    projection: &Projection,
    budget: Budget,
    mut limit_of: impl FnMut(usize) -> usize,
    mut between: impl FnMut(&[Row]),
) -> Vec<Page> {
    let mut pages: Vec<Page> = Vec::new();
    let mut paged: Vec<Row> = Vec::new();
    let mut cursor: Option<Cursor> = None;
    loop {
        let index = pages.len();
        assert!(index < 1_000, "runaway walk of {query:?}");
        let limit = limit_of(index);
        let read = reader
            .query(namespace, query, projection, limit, cursor.as_ref(), budget)
            .unwrap_or_else(|rejected| panic!("{query:?} page {index}: {rejected}"));
        let QueryPage { rows, next } = read.value;
        assert!(rows.len() <= limit, "page {index} exceeds its limit");
        if cursor.is_some() {
            assert!(
                !rows.is_empty(),
                "page {index}: a continuation leads to at least one more row"
            );
        }
        let mut floor = cursor.as_ref().map(Cursor::after);
        for row in &rows {
            assert!(
                floor.is_none_or(|floor| row.id > floor),
                "page {index}: rows ascend past the cursor"
            );
            floor = Some(row.id);
        }
        if let Some(continuation) = &next {
            assert_eq!(rows.len(), limit, "page {index}: a continued page is full");
            assert_eq!(
                continuation.after(),
                rows.last().expect("a full page has rows").id,
                "page {index}: after() is the last row"
            );
            assert_eq!(continuation.snapshot(), reader.pinned());
            assert_eq!(continuation.namespace(), namespace);
            assert_eq!(continuation.query(), query);
            assert_eq!(continuation.projection(), projection);
        }
        paged.extend(rows.iter().cloned());
        pages.push(Page {
            rows,
            next: next.clone(),
            receipt: read.receipt,
        });
        match next {
            Some(continuation) => {
                between(&paged);
                cursor = Some(continuation);
            }
            None => return pages,
        }
    }
}
