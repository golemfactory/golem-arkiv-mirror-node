//! Charge groups and every-budget sweeps for metered operations.
//!
//! A vector transcribes an operation's charge steps by hand as atomic groups.
//! The FIXTURE price table turns them into the receipt and the ledger the
//! operation must report under each budget.

use std::fmt::Debug;

use arkiv_db::{
    Branch, Budget, Cost, Effect, Engine, Ledger, LedgerEntry, OperationClass, Outcome, OwnerId,
    PriceSchedule, ReadResult, Receipt, Rejection, SnapshotId, WriteOp,
};

use super::{
    pricing::price,
    vocab::{
        ALICE, Canonical, OUT_OF_BUDGET, begin, canonical, publish, read_receipt, traced,
        write_receipt,
    },
};

/// One atomic charge group: every class in it is charged together or not at
/// all. A single charge is a group of one.
pub type Group = Vec<(OperationClass, u64)>;

pub fn lookup() -> Group {
    vec![(OperationClass::DirectoryLookup, 1)]
}

pub fn point_read() -> Group {
    vec![(OperationClass::PointRead, 1)]
}

fn group_cost(group: &Group) -> u64 {
    group
        .iter()
        .map(|&(class, count)| count * price(class))
        .sum()
}

/// Cumulative charge points: the k-th entry is what has been spent once the
/// first k groups were charged.
pub fn prefix_sums(groups: &[Group]) -> Vec<u64> {
    groups
        .iter()
        .scan(0, |spent, group| {
            *spent += group_cost(group);
            Some(*spent)
        })
        .collect()
}

/// How many leading groups fit `budget`: the abort point rule says the
/// operation charges exactly those and stops at the first that does not fit.
fn groups_fitting(groups: &[Group], budget: u64) -> usize {
    prefix_sums(groups)
        .iter()
        .take_while(|&&point| point <= budget)
        .count()
}

/// The ledger after the first `applied` groups: aggregated per class in
/// first-charge order, zero-count classes omitted.
fn ledger_after(groups: &[Group], applied: usize) -> Ledger {
    let mut entries: Vec<LedgerEntry> = Vec::new();
    for &(class, count) in groups.iter().take(applied).flatten() {
        if count == 0 {
            continue;
        }
        match entries.iter_mut().find(|entry| entry.class == class) {
            Some(entry) => {
                entry.count += count;
                entry.subtotal = Cost::new(entry.count * price(class));
            }
            None => entries.push(LedgerEntry {
                class,
                count,
                unit_price: price(class),
                subtotal: Cost::new(count * price(class)),
            }),
        }
    }
    Ledger { entries }
}

/// One run of a sweep: a budget and what the groups charge under it.
struct BudgetCase {
    /// The budget in units, or "unlimited", for failure messages.
    label: String,
    budget: Budget,
    traced: bool,
    /// Whether every group fits the budget.
    complete: bool,
    /// What the groups that fit cost.
    spent: u64,
    ledger: Ledger,
}

impl BudgetCase {
    /// `receipt` as this run must report it: with the ledger of the charged
    /// groups when the budget is traced.
    fn reported(&self, receipt: Receipt) -> Receipt {
        if self.traced {
            traced(receipt, self.ledger.clone())
        } else {
            receipt
        }
    }
}

/// Every budget in `0..=total + 1` and then unlimited, each untraced and
/// traced. `total` is the hand-written sum the groups must reproduce.
fn budget_cases(name: &str, groups: &[Group], total: u64) -> Vec<BudgetCase> {
    let sums = prefix_sums(groups);
    assert_eq!(
        sums.last().copied().unwrap_or(0),
        total,
        "{name}: the charge table disagrees with the hand-written total"
    );
    (0..=total + 1)
        .map(Some)
        .chain([None])
        .flat_map(|units| [(units, false), (units, true)])
        .map(|(units, trace)| {
            let budget = units.map_or(Budget::unlimited(), Budget::limit);
            let applied = units.map_or(groups.len(), |units| groups_fitting(groups, units));
            BudgetCase {
                label: units.map_or("unlimited".to_owned(), |units| units.to_string()),
                budget: if trace { budget.traced() } else { budget },
                traced: trace,
                complete: applied == groups.len(),
                spent: if applied == 0 { 0 } else { sums[applied - 1] },
                ledger: ledger_after(groups, applied),
            }
        })
        .collect()
}

/// How a write vector ends once every group is charged.
#[derive(Clone, Copy, Debug)]
pub enum Expect {
    Applied(Effect),
    /// A post-charge rejection (recorded, state unchanged).
    Rejected(Rejection),
}

pub struct WriteVector<'a> {
    name: &'a str,
    setup: &'a [WriteOp],
    actor: OwnerId,
    op: WriteOp,
    groups: Vec<Group>,
    total: u64,
    expect: Expect,
}

impl<'a> WriteVector<'a> {
    /// `op` executed by ALICE on the published state. `groups` are its charge
    /// steps and `total` the hand-written sum they must reproduce.
    pub fn new(name: &'a str, op: WriteOp, groups: Vec<Group>, total: u64, expect: Expect) -> Self {
        Self {
            name,
            setup: &[],
            actor: ALICE,
            op,
            groups,
            total,
            expect,
        }
    }

    /// Operations ALICE applies before `op`, on every fresh branch.
    pub fn setup(self, setup: &'a [WriteOp]) -> Self {
        Self { setup, ..self }
    }

    pub fn actor(self, actor: OwnerId) -> Self {
        Self { actor, ..self }
    }
}

/// A fresh branch with `setup` applied by ALICE under unlimited budgets, and
/// what those setup receipts spent (only used for the block aggregate
/// invariant, never as an expectation).
fn prepared(engine: &Engine, setup: &[WriteOp]) -> (Branch, u64) {
    let mut branch = begin(engine, None);
    let mut spent = 0;
    for op in setup {
        let executed = branch
            .execute(ALICE, op.clone(), Budget::unlimited())
            .expect("setup operations apply");
        spent += executed.receipt.spent.units();
    }
    (branch, spent)
}

/// Runs the vector under every budget in `0..=total + 1`, traced and
/// untraced, then unlimited, asserting the whole receipt, the ledger, the
/// abort point rule, recording, the block aggregate, an unchanged
/// `observe`d state on rejection and the effect on success. Returns the
/// unlimited branch.
pub fn assert_write_vector<O: Debug + PartialEq>(
    engine: &Engine,
    vector: &WriteVector<'_>,
    observe: impl Fn(&Branch) -> O,
) -> Branch {
    let WriteVector {
        name,
        setup,
        actor,
        op,
        groups,
        total,
        expect,
    } = vector;
    let baseline = observe(&prepared(engine, setup).0);
    let mut unlimited = None;
    for case in budget_cases(name, groups, *total) {
        let BudgetCase {
            label,
            traced,
            spent,
            ..
        } = &case;
        let (mut branch, setup_spent) = prepared(engine, setup);
        let expected: Result<Effect, Rejection> = match expect {
            _ if !case.complete => Err(Rejection::OutOfBudget),
            Expect::Applied(effect) => Ok(*effect),
            Expect::Rejected(reason) => Err(*reason),
        };
        let outcome = expected.map_or_else(Outcome::Rejected, |_| Outcome::Applied);
        let expected_receipt = case.reported(write_receipt(*spent, outcome));
        let result = branch.execute(*actor, op.clone(), case.budget);
        match (&result, expected) {
            (Ok(executed), Ok(effect)) => {
                assert_eq!(executed.effect, effect, "{name}: effect at budget {label}");
                assert_eq!(
                    executed.receipt, expected_receipt,
                    "{name}: receipt at budget {label} (traced {traced})"
                );
            }
            (Err(rejected), Err(reason)) => {
                assert_eq!(rejected.reason, reason, "{name}: reason at budget {label}");
                assert_eq!(
                    rejected.receipt, expected_receipt,
                    "{name}: receipt at budget {label} (traced {traced})"
                );
                assert_eq!(
                    observe(&branch),
                    baseline,
                    "{name}: state changed after a rejection at budget {label}"
                );
            }
            (Ok(executed), Err(reason)) => {
                panic!("{name}: applied at budget {label}, expected {reason:?}: {executed:?}")
            }
            (Err(rejected), Ok(effect)) => {
                panic!("{name}: rejected at budget {label}, expected {effect:?}: {rejected}")
            }
        }
        assert_eq!(
            branch.receipts().len(),
            setup.len() + 1,
            "{name}: the receipt is recorded at budget {label}"
        );
        assert_eq!(
            branch.receipts().last().map(|receipt| receipt.spent),
            Some(Cost::new(*spent)),
            "{name}: the recorded receipt at budget {label}"
        );
        assert_eq!(
            branch.spent(),
            Cost::new(setup_spent + spent),
            "{name}: block spent at budget {label}"
        );
        if case.budget.limit_units().is_none() && !traced {
            unlimited = Some(branch);
        }
    }
    unlimited.expect("the unlimited run was recorded")
}

/// Runs a read under every budget in `0..=total + 1`, traced and untraced,
/// then unlimited, asserting the whole receipt, the ledger and, on success,
/// the value; a rejected read returns nothing.
pub fn assert_read_vector<T: Debug + PartialEq>(
    name: &str,
    groups: &[Group],
    total: u64,
    expected: &T,
    read: impl Fn(Budget) -> ReadResult<T>,
) {
    for case in budget_cases(name, groups, total) {
        let BudgetCase {
            label,
            traced,
            complete,
            spent,
            ..
        } = &case;
        let outcome = if *complete {
            Outcome::Applied
        } else {
            OUT_OF_BUDGET
        };
        let expected_receipt = case.reported(read_receipt(*spent, outcome));
        match read(case.budget) {
            Ok(metered) => {
                assert!(complete, "{name}: applied at budget {label} below {total}");
                assert_eq!(&metered.value, expected, "{name}: value at budget {label}");
                assert_eq!(
                    metered.receipt, expected_receipt,
                    "{name}: receipt at budget {label} (traced {traced})"
                );
            }
            Err(rejected) => {
                assert!(
                    !complete,
                    "{name}: rejected at budget {label} >= {total}: {rejected}"
                );
                assert_eq!(rejected.reason, Rejection::OutOfBudget, "{name}: {label}");
                assert_eq!(
                    rejected.receipt, expected_receipt,
                    "{name}: receipt at budget {label} (traced {traced})"
                );
            }
        }
    }
}

/// One scripted write: label, actor, operation, budget and the canonical
/// result it must have.
pub type ScriptStep<'a> = (&'a str, OwnerId, WriteOp, Budget, Canonical);

/// Estimates and then executes each step on `branch`: the estimate must
/// equal the execution, and the execution the step's vector.
pub fn assert_estimate_matches_execute(branch: &mut Branch, script: Vec<ScriptStep<'_>>) {
    for (label, actor, op, budget, expected) in script {
        let estimated = canonical(&branch.estimate(actor, op.clone(), budget));
        let executed = canonical(&branch.execute(actor, op, budget));
        assert_eq!(
            estimated, executed,
            "{label}: estimate differs from execute"
        );
        assert_eq!(
            executed, expected,
            "{label}: execute differs from the vector"
        );
    }
}

/// Publishes a block on the head that only commits `schedule`.
pub fn publish_pricing(engine: &mut Engine, schedule: PriceSchedule) -> SnapshotId {
    let mut admin = begin(engine, None);
    admin
        .set_pricing(schedule)
        .expect("the schedule id exceeds the committed one");
    publish(engine, admin)
}
