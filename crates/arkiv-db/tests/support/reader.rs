//! One read interface over [`Branch`] and [`Snapshot`], so a test can make
//! the same assertions about a candidate and about published state.

use std::ops::Bound;

use arkiv_db::{
    Branch, Budget, CellName, Cursor, Entry, HostKey, NamespaceDescriptor, NamespaceId, Projection,
    Query, QueryPage, ReadResult, RecordKey, Row, Snapshot, SnapshotId, StorageError, TypeTag,
    Value,
};

macro_rules! reader {
    ($($method:ident($($arg:ident: $type:ty),*) -> $result:ty;)*) => {
        /// The reads both handles offer, under the names the handles give them.
        pub trait Reader {
            /// The snapshot a cursor issued by this reader names; `None` on a
            /// branch.
            fn pinned(&self) -> Option<SnapshotId>;
            $(fn $method(&self $(, $arg: $type)*) -> $result;)*
        }

        impl Reader for Snapshot {
            fn pinned(&self) -> Option<SnapshotId> {
                Some(self.id())
            }

            $(fn $method(&self $(, $arg: $type)*) -> $result {
                Snapshot::$method(self $(, $arg)*)
            })*
        }

        impl Reader for Branch {
            fn pinned(&self) -> Option<SnapshotId> {
                None
            }

            $(fn $method(&self $(, $arg: $type)*) -> $result {
                Branch::$method(self $(, $arg)*)
            })*
        }
    };
}

reader! {
    namespace_count() -> usize;
    record_count(namespace: NamespaceId) -> Option<usize>;
    typed_record_count(namespace: NamespaceId) -> Option<usize>;
    term_count(namespace: NamespaceId) -> Option<usize>;
    index_consistent(namespace: NamespaceId) -> Option<bool>;
    get(namespace: NamespaceId, key: &[u8], budget: Budget) -> ReadResult<Option<Vec<u8>>>;
    scan_prefix(namespace: NamespaceId, prefix: &[u8], budget: Budget) -> ReadResult<Vec<Entry>>;
    descriptor(namespace: NamespaceId, budget: Budget) -> ReadResult<NamespaceDescriptor>;
    get_record(
        namespace: NamespaceId,
        key: &RecordKey,
        projection: &Projection,
        budget: Budget
    ) -> ReadResult<Option<Row>>;
    count(namespace: NamespaceId, query: &Query, budget: Budget) -> ReadResult<u64>;
    terms(
        namespace: NamespaceId,
        name: &CellName,
        tag: TypeTag,
        lower: Bound<Value>,
        limit: usize,
        budget: Budget
    ) -> ReadResult<Vec<Value>>;
    query(
        namespace: NamespaceId,
        query: &Query,
        projection: &Projection,
        limit: usize,
        cursor: Option<&Cursor>,
        budget: Budget
    ) -> ReadResult<QueryPage>;
    try_host_range(
        from: &HostKey,
        limit: usize
    ) -> Result<Vec<(HostKey, Vec<u8>)>, StorageError>;
}
