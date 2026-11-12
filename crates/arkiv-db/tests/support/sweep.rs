//! Budget sweeps: what a metered call reports at every budget.

use std::fmt::Debug;

use arkiv_db::{Budget, Cost, ReadResult, Rejection};

use super::vocab::{read_applied, read_rejected};

/// What an operation has spent when `budget` runs out: the largest
/// cumulative charge point that still fits it (zero before the first one).
pub fn largest_prefix_sum_at_most(prefix_sums: &[u64], budget: u64) -> u64 {
    prefix_sums
        .iter()
        .copied()
        .filter(|&point| point <= budget)
        .max()
        .unwrap_or(0)
}

/// Runs a read under every budget in `0..=total` plus unlimited and asserts
/// the whole receipt and, on success, the value. `prefix_sums` are the
/// read's cumulative charge points; the last one is its total.
pub fn assert_read_sweep<T: Debug + PartialEq>(
    label: &str,
    prefix_sums: &[u64],
    expected: &T,
    read: impl Fn(Budget) -> ReadResult<T>,
) {
    let total = *prefix_sums.last().expect("a vector has a total");
    let budgets = (0..=total).map(Budget::limit).chain([Budget::unlimited()]);
    for budget in budgets {
        let limit = budget.limit_units().map_or(u64::MAX, Cost::units);
        match read(budget) {
            Ok(metered) => {
                assert!(
                    limit >= total,
                    "{label}: applied at budget {limit} below {total}"
                );
                assert_eq!(&metered.value, expected, "{label}: value at budget {limit}");
                assert_eq!(
                    metered.receipt,
                    read_applied(total),
                    "{label}: receipt at {limit}"
                );
            }
            Err(rejected) => {
                assert!(
                    limit < total,
                    "{label}: rejected at budget {limit}: {rejected}"
                );
                let spent = largest_prefix_sum_at_most(prefix_sums, limit);
                assert_eq!(
                    rejected.receipt,
                    read_rejected(spent, Rejection::OutOfBudget),
                    "{label}: receipt at budget {limit}"
                );
            }
        }
    }
}
