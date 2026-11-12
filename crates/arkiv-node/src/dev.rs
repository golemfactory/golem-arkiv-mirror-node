//! The development chain: its genesis and two actors, shared by the demo,
//! the simulator and the signed profile's default genesis.

use crate::{ChainParams, Genesis};
use arkiv_db::{Limits, OwnerId, PriceSchedule};

pub const ALICE: OwnerId = OwnerId::new([0xa1; 20]);
pub const BOB: OwnerId = OwnerId::new([0xb0; 20]);

pub fn genesis(chain_id: u64, timestamp_ms: u64) -> Genesis {
    Genesis {
        format_version: crate::block::FORMAT_VERSION,
        protocol_version: crate::block::PROTOCOL_VERSION,
        chain_id,
        timestamp_ms,
        limits: Limits {
            namespace_cap: 1024,
            // Room for a full entity: 32 user attributes beside the system cells.
            max_cells: crate::arkiv::MIN_ENTITY_CELLS,
            max_page: 64,
            max_groups: 8,
            max_literals: 8,
            ..Limits::DEFAULT.with_value_len(128 * 1024)
        },
        pricing: PriceSchedule::FIXTURE,
        params: ChainParams {
            block_budget: 1_000_000_000,
            user_budget: 100_000_000,
            expiry_per_block: 16,
            max_admin_per_block: 2,
            max_txs_per_block: 16,
            max_ops_per_tx: 16,
            max_ops_per_block: 128,
            max_tx_bytes: 1024 * 1024,
            max_block_input_bytes: 2 * 1024 * 1024,
            max_lifetime: 1000,
            // The simulator's one-second blocks and a one-year rent cap; only
            // GLM chains charge rent.
            block_period_ms: 1_000,
            max_rent_lifetime_seconds: crate::gas::DEFAULT_RENT_CAP_SECONDS,
            // No storage cap: the demo's 2 MiB blocks are not a signed profile.
            max_new_storage_units_per_block: u64::MAX,
        },
        admins: vec![ALICE],
    }
}
