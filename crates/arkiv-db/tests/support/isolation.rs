//! The harness of the isolation tests: one namespace runs a fixed workload of
//! named steps on a branch that is dropped, and the steps are compared with
//! hand-written expectations and with reruns after unrelated state has grown.

use std::fmt::Debug;

use arkiv_db::{
    Branch, Cost, Effect, Engine, OperationClass, Outcome, PhysicalTrace, ReadResult, Receipt,
    ReceiptKind, Rejection, SnapshotId, WriteResult,
};

use super::{
    pricing::price,
    vocab::{ledger, traced},
};

/// Any directory path of 8-byte keys: at most `2·8 + 1` nodes.
pub const DIRECTORY_PATH_BOUND: u64 = 17;

/// One executed step: its result, its receipts (one per metered operation)
/// and, when the engine exposes it, the physical trace.
#[derive(Clone, Debug)]
pub struct Step<O> {
    pub name: &'static str,
    pub observed: O,
    pub receipts: Vec<Receipt>,
    pub physical: Option<PhysicalTrace>,
}

impl<O> Step<O> {
    /// The receipt of a step that ran one metered operation.
    pub fn receipt(&self) -> &Receipt {
        assert_eq!(self.receipts.len(), 1, "{} has one receipt", self.name);
        &self.receipts[0]
    }

    pub fn physical(&self) -> PhysicalTrace {
        self.physical
            .unwrap_or_else(|| panic!("{} has no physical trace", self.name))
    }
}

/// The hand-derived expectation for one step.
pub struct Expected<O> {
    pub name: &'static str,
    pub observed: O,
    pub receipts: Vec<Receipt>,
}

pub fn read_step<T, O: From<Rejection>>(
    name: &'static str,
    result: ReadResult<T>,
    wrap: fn(T) -> O,
) -> Step<O> {
    let (observed, receipt, physical) = match result {
        Ok(metered) => (wrap(metered.value), metered.receipt, metered.physical),
        Err(rejected) => (rejected.reason.into(), rejected.receipt, rejected.physical),
    };
    Step {
        name,
        observed,
        receipts: vec![receipt],
        physical: Some(physical),
    }
}

pub fn write_step<O: From<Effect> + From<Rejection>>(
    name: &'static str,
    result: WriteResult,
) -> Step<O> {
    let (observed, receipt, physical) = match result {
        Ok(executed) => (executed.effect.into(), executed.receipt, executed.physical),
        Err(rejected) => (rejected.reason.into(), rejected.receipt, rejected.physical),
    };
    Step {
        name,
        observed,
        receipts: vec![receipt],
        physical: Some(physical),
    }
}

/// Attaches a ledger of `(class, count)` lines in first-charge order, priced
/// from the fixture table. The lines must add up to what the receipt spent.
pub fn itemized(receipt: Receipt, lines: &[(OperationClass, u64)]) -> Receipt {
    let lines: Vec<_> = lines
        .iter()
        .map(|&(class, count)| (class, count, price(class), count * price(class)))
        .collect();
    let total: u64 = lines.iter().map(|&(.., subtotal)| subtotal).sum();
    assert_eq!(
        Cost::new(total),
        receipt.spent,
        "the stated ledger lines must add up to the stated total"
    );
    traced(receipt, ledger(&lines))
}

/// Seals `branch`, checks what the block spent and publishes it on the head.
pub fn publish_block(engine: &mut Engine, branch: Branch, spent: u64, block: &str) -> SnapshotId {
    let sealed = branch.seal();
    assert_eq!(sealed.spent(), Cost::new(spent), "{block} cost");
    engine
        .publish(&sealed)
        .unwrap_or_else(|fault| panic!("{block} does not publish: {fault:?}"))
}

pub fn find<'a, O>(steps: &'a [Step<O>], name: &str) -> &'a Step<O> {
    steps
        .iter()
        .find(|step| step.name == name)
        .unwrap_or_else(|| panic!("workload has no step {name:?}"))
}

/// The single receipt expected of the step called `name`.
pub fn expected_receipt<O>(expected: &[Expected<O>], name: &str) -> Receipt {
    let want = expected
        .iter()
        .find(|want| want.name == name)
        .unwrap_or_else(|| panic!("baseline has no step {name:?}"));
    assert_eq!(want.receipts.len(), 1, "{name} expects one receipt");
    want.receipts[0].clone()
}

fn assert_answers<'a, O: PartialEq + Debug + 'a>(
    steps: &[Step<O>],
    want: impl ExactSizeIterator<Item = (&'static str, &'a O, &'a [Receipt])>,
    phase: &str,
) {
    assert_eq!(steps.len(), want.len(), "{phase}: workload length");
    for (step, (name, observed, receipts)) in steps.iter().zip(want) {
        assert_eq!(step.name, name, "{phase}: workload order");
        assert_eq!(step.receipts, receipts, "{phase}: receipts of {name}");
        assert_eq!(&step.observed, observed, "{phase}: result of {name}");
    }
}

pub fn assert_matches_expected<O: PartialEq + Debug>(
    steps: &[Step<O>],
    expected: &[Expected<O>],
    phase: &str,
) {
    let want = expected
        .iter()
        .map(|want| (want.name, &want.observed, want.receipts.as_slice()));
    assert_answers(steps, want, phase);
}

pub fn assert_receipts_and_results_equal<O: PartialEq + Debug>(
    baseline: &[Step<O>],
    rerun: &[Step<O>],
    phase: &str,
) {
    let want = baseline
        .iter()
        .map(|step| (step.name, &step.observed, step.receipts.as_slice()));
    assert_answers(rerun, want, phase);
}

/// Every step of `steps` gave the result, receipts and physical trace of the
/// baseline step of the same name.
pub fn assert_steps_repeat<O: PartialEq + Debug>(
    steps: &[Step<O>],
    baseline: &[Step<O>],
    what: &str,
) {
    for step in steps {
        let name = step.name;
        let expected = find(baseline, name);
        assert_eq!(step.observed, expected.observed, "{what} result of {name}");
        assert_eq!(
            step.receipts, expected.receipts,
            "{what} receipts of {name}"
        );
        assert_eq!(
            step.physical, expected.physical,
            "{what} physical trace of {name}"
        );
    }
}

/// The namespace's own maps are the same objects in both runs, so every
/// visit count of its record and index maps is identical.
pub fn assert_namespace_local_nodes_equal<O>(baseline: &[Step<O>], rerun: &[Step<O>], phase: &str) {
    for (before, after) in baseline.iter().zip(rerun) {
        let (Some(was), Some(now)) = (before.physical, after.physical) else {
            assert!(before.physical.is_none() && after.physical.is_none());
            continue;
        };
        assert_eq!(
            now.record_nodes, was.record_nodes,
            "{phase}: record nodes of {}",
            after.name
        );
        assert_eq!(
            now.index_nodes, was.index_nodes,
            "{phase}: index nodes of {}",
            after.name
        );
    }
}

pub fn assert_directory_nodes_equal<O>(baseline: &[Step<O>], rerun: &[Step<O>], phase: &str) {
    for (before, after) in baseline.iter().zip(rerun) {
        let (Some(was), Some(now)) = (before.physical, after.physical) else {
            continue;
        };
        assert_eq!(
            now.directory_nodes, was.directory_nodes,
            "{phase}: directory nodes of {}",
            after.name
        );
    }
}

/// Directory work of each traced step, derived from the trie shape: none
/// when nothing was charged (admission rejections, and a budget that does
/// not cover `DirectoryLookup`, which precedes the lookup), twice the
/// lookup path for an applied write (the lookup, then the insert of the
/// updated catalog leaf along the same path), and the lookup path alone for
/// every read and every write rejected after the lookup.
pub fn assert_directory_nodes_follow_the_trie<O>(steps: &[Step<O>], path: u64, phase: &str) {
    for step in steps {
        let Some(physical) = step.physical else {
            continue;
        };
        let receipt = step.receipt();
        let expected = match (receipt.kind, receipt.outcome) {
            _ if receipt.spent == Cost::ZERO => 0,
            (ReceiptKind::Write, Outcome::Applied) => 2 * path,
            _ => path,
        };
        assert_eq!(
            physical.directory_nodes, expected,
            "{phase}: directory nodes of {}",
            step.name
        );
    }
}
