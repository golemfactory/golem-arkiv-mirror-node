//! The unit prices of `PriceSchedule::FIXTURE`, written out. Expected costs
//! are built from this table, never from the schedule under test.

use arkiv_db::OperationClass;

pub const fn price(class: OperationClass) -> u64 {
    match class {
        OperationClass::DirectoryLookup => 1,
        OperationClass::NamespaceCreate => 50,
        OperationClass::NamespaceTransfer => 20,
        OperationClass::PutRecord => 10,
        OperationClass::DeleteRecord => 5,
        OperationClass::WriteKeyByte => 2,
        OperationClass::WriteValueByte => 1,
        OperationClass::PointRead => 3,
        OperationClass::ScanSeek => 4,
        OperationClass::ScanEntry => 2,
        OperationClass::ReadByte => 1,
        OperationClass::RecordCreate => 20,
        OperationClass::RecordPatch => 10,
        OperationClass::RecordDelete => 20,
        OperationClass::CellWrite => 4,
        OperationClass::CellByte => 1,
        OperationClass::CellRemove => 4,
        OperationClass::IndexInsert => 6,
        OperationClass::IndexTermCreate => 30,
        OperationClass::IndexRemove => 6,
        OperationClass::QueryGroup => 5,
        OperationClass::TermSeek => 4,
        OperationClass::TermVisit => 3,
        OperationClass::PostingEntry => 1,
        OperationClass::RowVisit => 1,
        OperationClass::RecordProbe => 2,
        OperationClass::ResultRow => 3,
        OperationClass::RootUpdate => 2,
        OperationClass::RowByte => 1,
        OperationClass::ProofNode => 2,
        OperationClass::ProofByte => 1,
    }
}
