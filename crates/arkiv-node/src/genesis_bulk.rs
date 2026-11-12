//! Private bulk genesis construction. Only complete state has a durable head;
//! interrupted scratch is disposable and never becomes a chain publication.

use crate::{
    accounts, arkiv,
    genesis_bundle::PrefillLimits,
    genesis_streaming::{
        PrefillImport, Progress, StreamReader, StreamSummary, audit_file, check_disk,
        import_redb_config, validate_limits,
    },
};
use alloy_primitives::U256;
use arkiv_db::{
    NamespaceDescriptor, NamespaceName, OwnerId, RecordKey, Revision, decode_exact, digest,
    durable::{BulkGenesisBuilder, DurableEngine, EngineGenesis, Initialization, Options},
    encode,
};
use arkiv_db_redb::RedbStore;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{BufReader, Read, Seek, Write},
    path::Path,
    sync::Arc,
};

use crate::{fsutil, genesis_error::GenesisToolError};

type Result<T> = std::result::Result<T, GenesisToolError>;
const SORT_BUFFER_BYTES: usize = 64 * 1024 * 1024;
const DISK_CHECK_INTERVAL: u64 = 64 * 1024 * 1024;

fn err(e: impl std::fmt::Display) -> GenesisToolError {
    format!("bulk prefill: {e}").into()
}

fn debug_err(e: impl std::fmt::Debug) -> GenesisToolError {
    format!("bulk prefill: {e:?}").into()
}

fn read_bounded(path: &Path, cap: u64) -> Result<Vec<u8>> {
    let file = File::open(path).map_err(err)?;
    if file.metadata().map_err(err)?.len() > cap {
        return Err("bulk metadata exceeds size cap".into());
    }
    let mut bytes = Vec::new();
    file.take(cap + 1).read_to_end(&mut bytes).map_err(err)?;
    if bytes.len() as u64 > cap {
        return Err("bulk metadata grew beyond size cap".into());
    }
    Ok(bytes)
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Complete {
    version: u32,
    identity: String,
    rows: u64,
    units: u64,
    burn: String,
    checksum: String,
}

fn receipt_hash(genesis: &EngineGenesis, rows: u64, units: u64, burn: U256) -> String {
    let mut bytes = encode(genesis);
    bytes.extend_from_slice(&rows.to_be_bytes());
    bytes.extend_from_slice(&units.to_be_bytes());
    bytes.extend_from_slice(&burn.to_be_bytes::<32>());
    digest("arkiv/bulk-receipt/v1", &bytes).to_string()
}

fn open_complete(
    dir: &Path,
    expected: &EngineGenesis,
    summary: &StreamSummary,
    limits: PrefillLimits,
    balance: U256,
    options: Options,
) -> Result<(DurableEngine, U256, u64)> {
    let complete: Complete =
        serde_json::from_slice(&read_bounded(&dir.join("complete.json"), 4096)?).map_err(err)?;
    let genesis: EngineGenesis =
        decode_exact(&read_bounded(&dir.join("genesis.bin"), 65536)?).map_err(err)?;
    let burn = U256::from_str_radix(&complete.burn, 10).map_err(err)?;
    if complete.version != 1
        || complete.identity != alloy_primitives::B256::from(expected.deployment).to_string()
        || complete.checksum != receipt_hash(&genesis, complete.rows, complete.units, burn)
        || complete.rows != summary.rows
        || complete.units > limits.max_storage_units
        || burn > balance
        || genesis.deployment != expected.deployment
        || genesis.limits != expected.limits
        || genesis.pricing != expected.pricing
        || genesis.max_object_bytes != expected.max_object_bytes
        || genesis.initial_state.is_none()
    {
        return Err("bulk completed state identity or bounds mismatch".into());
    }
    let store =
        Arc::new(RedbStore::open(dir.join("state.redb"), import_redb_config()).map_err(err)?);
    let engine = DurableEngine::open(store, genesis, options).map_err(err)?;
    Ok((engine, burn, complete.units))
}

/// Builds the state of `import` from its stream `file` in private scratch
/// under `work_dir`: the engine, the rent burned and the storage units.
pub(crate) fn build_file(
    import: PrefillImport<'_>,
    file: &mut File,
    work_dir: &Path,
    options: Options,
    ingestion_bytes: usize,
) -> Result<(DurableEngine, U256, u64)> {
    let PrefillImport {
        base,
        limits,
        sponsor,
        summary,
        permanent,
    } = import;
    if ingestion_bytes == 0 || ingestion_bytes > 4 * 1024 * 1024 {
        return Err("batch bytes must be 1..=4194304".into());
    }
    validate_limits(limits)?;
    base.accounts.validate().map_err(debug_err)?;
    if base.genesis.pricing != crate::gas::schedule() {
        return Err("streaming prefill requires GLM pricing".into());
    }
    let identity = import.identity();
    let mut genesis = EngineGenesis::new(identity.0, base.genesis.pricing.clone());
    genesis.limits = base.genesis.limits;
    genesis.validate().map_err(err)?;
    let balance = base.accounts.initial_balance(OwnerId::new(sponsor));
    // `bulk-v1/` keeps bulk output apart from anything else in the work
    // directory. The lock also covers scratch deletion.
    let bulk = work_dir.join("bulk-v1");
    fs::create_dir_all(&bulk).map_err(err)?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(bulk.join("LOCK"))
        .map_err(err)?;
    lock.try_lock().map_err(err)?;
    check_disk(&bulk, 0)?;
    let identity_path = bulk.join("IDENTITY");
    if identity_path.exists() {
        if read_bounded(&identity_path, 32)? != identity.as_slice() {
            return Err("bulk import identity mismatch".into());
        }
    } else {
        // IDENTITY.next is unselected and may be left by a power loss.
        let next = bulk.join("IDENTITY.next");
        if next.exists() {
            fs::remove_file(&next).map_err(err)?;
        }
        fsutil::write_new(&next, identity.as_slice()).map_err(err)?;
        fs::rename(next, &identity_path).map_err(err)?;
        fsutil::sync_dir(&bulk).map_err(err)?;
    }
    let ready = bulk.join("complete");
    let pending = bulk.join("pending");
    if ready.exists() {
        // A verified descriptor may have changed after its first audit. Even a
        // completed restart must read/hash it before returning stored state.
        if audit_file(file, limits, permanent)? != *summary {
            return Err("prefill stream changed during import".into());
        }
        return open_complete(&ready, &genesis, summary, limits, balance, options);
    }
    if pending.exists() {
        fs::remove_dir_all(&pending).map_err(err)?;
    }
    fs::create_dir(&pending).map_err(err)?;
    fsutil::sync_dir(&bulk).map_err(err)?;
    let scratch = pending.join("sort");
    let mut builder = BulkGenesisBuilder::new(
        &scratch,
        NamespaceDescriptor {
            id: arkiv::entity_namespace(),
            name: NamespaceName::new("entities").map_err(debug_err)?,
            owner: OwnerId::new(base.proposer),
            revision: Revision::INITIAL,
        },
        base.genesis.limits,
        SORT_BUFFER_BYTES,
    )
    .map_err(err)?;
    let disk_path = pending.clone();
    builder.set_disk_check(Box::new(move || {
        check_disk(&disk_path, DISK_CHECK_INTERVAL).map_err(|e| e.to_string())
    }));
    builder.set_progress(|phase, count| {
        let record =
            serde_json::json!({"event":"prefill_bulk_phase", "phase":phase, "count":count});
        let _ = writeln!(std::io::stderr().lock(), "{record}");
    });
    file.rewind().map_err(err)?;
    let mut reader = StreamReader::with_policy(BufReader::new(file), limits, permanent)?;
    let mut progress = Progress::new("bulk_normalize", summary.rows, summary.bytes);
    progress.report(0, reader.bytes, None, false);
    let clock = crate::genesis_bundle::rent_clock(base).map_err(err)?;
    let mut units = 0u64;
    let mut burn = U256::ZERO;
    let mut creator_nonce: Option<([u8; 20], u64)> = None;
    let mut disk_at = 0;
    let mut report_at = 0;
    while let Some(row) = reader.next_row()? {
        if reader.bytes.saturating_sub(disk_at) >= DISK_CHECK_INTERVAL {
            check_disk(&pending, DISK_CHECK_INTERVAL)?;
            disk_at = reader.bytes;
        }
        let key = arkiv::derive_entity_key(
            base.genesis.chain_id,
            &row.creator,
            row.create_nonce,
            row.salt,
        );
        let context = arkiv::ArkivExecutionContext {
            caller: row.creator,
            block_height: 0,
            chain_id: base.genesis.chain_id,
            rent: clock,
            expiry_per_block: base.genesis.params.expiry_per_block,
            expiry_spill_window: 0,
            // Only used for expiry resolution; no operation executes here.
            payer: None,
            allow_permanent_create: false,
        };
        let expiry = crate::Expiry {
            expires_at: row.expires_at_height,
            min_lifetime: 0,
        };
        let op = arkiv::ArkivOperation::Create {
            salt: row.salt,
            flags: row.flags,
            attributes: row.attributes,
            expiry,
        };
        op.validate().map_err(debug_err)?;
        let expires = arkiv::expiry(&expiry, &context).map_err(debug_err)?;
        let is_permanent = expires == crate::genesis_bundle::PERMANENT_EXPIRY;
        if is_permanent && !permanent {
            return Err("permanent prefill rows require a permanent-prefill launch".into());
        }
        let arkiv::ArkivOperation::Create { attributes, .. } = &op else {
            unreachable!()
        };
        let cells = arkiv::create_cells(key, row.creator, 0, expires, row.flags, attributes);
        let row_units = crate::gas::storage_units(&cells).map_err(debug_err)?.total;
        units = units
            .checked_add(row_units)
            .ok_or("storage units overflow")?;
        if units > limits.max_storage_units {
            return Err("prefill storage limit".into());
        }
        // Capped finite rent; permanent rows prepay none (see prefill_rent_wei).
        let rent = crate::genesis_bundle::prefill_rent_wei(row_units, expires, permanent, clock)
            .map_err(err)?;
        burn = burn.checked_add(rent).ok_or("prefill rent overflow")?;
        if burn > balance {
            return Err("prefill sponsor underfunded".into());
        }
        builder
            .add_record(RecordKey::new(&key).ok_or("invalid entity key")?, cells)
            .map_err(err)?;
        // A permanent row is never due, exactly as live execution never
        // schedules `u64::MAX`; the expiry sweep therefore never visits it.
        if !is_permanent {
            for host in [
                crate::exec::due_key(expires, arkiv::entity_namespace()),
                crate::exec::total_key(expires),
            ] {
                builder
                    .add_host_count(host, 1, u64::from(base.genesis.params.expiry_per_block))
                    .map_err(err)?;
            }
        }
        let next = row
            .create_nonce
            .checked_add(1)
            .ok_or("prefill creator nonce exhausted")?;
        if let Some((creator, nonce)) = creator_nonce
            && creator != row.creator
        {
            builder
                .add_host(arkiv::nonce_key(&creator), nonce.to_be_bytes().to_vec())
                .map_err(err)?;
        }
        creator_nonce = Some((row.creator, next));
        if reader.bytes.saturating_sub(report_at) >= ingestion_bytes as u64 {
            progress.report(reader.consumed, reader.bytes, None, false);
            report_at = reader.bytes;
        }
    }
    if let Some((creator, nonce)) = creator_nonce {
        builder
            .add_host(arkiv::nonce_key(&creator), nonce.to_be_bytes().to_vec())
            .map_err(err)?;
    }
    if reader.finish()? != *summary {
        return Err("prefill stream changed during import".into());
    }
    progress.report(summary.rows, summary.bytes, None, true);
    if burn != U256::ZERO {
        let account = accounts::Account {
            nonce: 0,
            balance: balance
                .checked_sub(burn)
                .ok_or("prefill sponsor underfunded")?,
        };
        builder
            .add_host(
                accounts::account_key(OwnerId::new(sponsor)),
                encode(&account),
            )
            .map_err(err)?;
        builder
            .add_host(accounts::BURN_KEY, burn.to_be_bytes::<32>().to_vec())
            .map_err(err)?;
    }
    check_disk(&pending, DISK_CHECK_INTERVAL)?;
    let store =
        Arc::new(RedbStore::create(pending.join("state.redb"), import_redb_config()).map_err(err)?);
    let mut progress = Progress::new("bulk_indexes", summary.rows, summary.bytes);
    progress.report(0, 0, None, false);
    let build = builder
        .finish(store, base.genesis.pricing.clone(), options.clone())
        .map_err(err)?;
    genesis.initial_state = Some(build.state_header().clone());
    let engine = match DurableEngine::initialize_bulk(build, genesis.clone(), options.clone())
        .map_err(err)?
    {
        Initialization::Committed(engine) => engine,
        _ => return Err("bulk initialization uncertain; reopen to rebuild private scratch".into()),
    };
    drop(engine);
    progress.report(summary.rows, summary.bytes, Some(0), true);
    // The complete directory is the atomic selection point. No stage has a
    // durable chain-visible head, and an unselected stage is safe to discard.
    fs::remove_dir_all(&scratch).map_err(err)?;
    fsutil::write_new(&pending.join("genesis.bin"), &encode(&genesis)).map_err(err)?;
    fsutil::write_new(
        &pending.join("complete.json"),
        &serde_json::to_vec(&Complete {
            version: 1,
            identity: identity.to_string(),
            rows: summary.rows,
            units,
            burn: burn.to_string(),
            checksum: receipt_hash(&genesis, summary.rows, units, burn),
        })
        .map_err(err)?,
    )
    .map_err(err)?;
    fsutil::sync_dir(&pending).map_err(err)?;
    fs::rename(&pending, &ready).map_err(err)?;
    fsutil::sync_dir(&bulk).map_err(err)?;
    open_complete(&ready, &genesis, summary, limits, balance, options)
}
