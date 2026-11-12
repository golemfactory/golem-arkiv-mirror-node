//! `--genesis-install adopt`: a fresh producer installs a streaming genesis by
//! verifying and relabelling its private staged store and renaming it over
//! the headless `node.redb`, instead of copying it.
//!
//! Every test runs `arkiv-proposer` itself.

mod cli_support;
mod signed_support;

use alloy_primitives::U256;
use arkiv_db::{Cell, CellName, Value, decode_exact, durable::Options, encode};
use arkiv_db_redb::{RedbConfig, RedbStore};
use arkiv_db_storage::versioned::VersionedStore;
use arkiv_node::{
    accounts::AccountConfig,
    genesis_adopt::{self, Staged},
    genesis_bundle::PrefillEntity,
    genesis_streaming::{build_bundle, prepare_streaming, write_stream},
    signed::protocol::{SignedGenesis, SignedLimits, default_native_genesis},
};
use cli_support::{Child, Temp, free_port, http, ready, refused};
use signed_support::signer;
use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

const BIN: &str = env!("CARGO_BIN_EXE_arkiv-proposer");
const ROWS: u64 = 48;
const MARKER: &[u8] = b"adopt-tamper-marker-7f3e9a21";

fn temp(tag: &str) -> Temp {
    Temp::new(&format!("arkiv-adopt-{tag}"))
}

fn address() -> String {
    arkiv_node::text::hex(signer(1).address().as_bytes())
}

/// A funded base genesis, a canonical stream and a fixture key.
fn inputs(dir: &Path) -> (PathBuf, PathBuf, PathBuf) {
    let proposer = signer(1).address();
    let base = SignedGenesis {
        genesis: default_native_genesis(9452, 1000, 1000),
        run_id: [9; 16],
        proposer: *proposer.as_bytes(),
        block_period_ms: 1000,
        limits: SignedLimits::default(),
        accounts: AccountConfig::new(vec![(proposer, U256::from(10).pow(U256::from(30)))]).unwrap(),
        prefill: None,
    };
    let rows = (0..ROWS).map(|n| {
        let mut body = format!("entity {n} ").into_bytes();
        if n == 5 {
            body.extend_from_slice(MARKER);
        }
        PrefillEntity {
            creator: [2; 20],
            create_nonce: n,
            salt: n as u128,
            expires_at_height: 1000 + n / 4,
            flags: 0,
            attributes: vec![
                (
                    CellName::new("body").unwrap(),
                    Cell::field(Value::bytes(&body)),
                ),
                (
                    CellName::new("kind").unwrap(),
                    Cell::attribute(Value::str("fixture")),
                ),
                (
                    CellName::new("mod").unwrap(),
                    Cell::attribute(Value::U64(n % 3)),
                ),
                (CellName::new("n").unwrap(), Cell::attribute(Value::U64(n))),
            ],
        }
    });
    let stream = write_stream(rows.collect::<Vec<_>>().iter());
    let (base_path, stream_path, key) = (
        dir.join("base.bin"),
        dir.join("input.bin"),
        dir.join("fixture.key"),
    );
    std::fs::write(&base_path, encode(&base)).unwrap();
    std::fs::write(&stream_path, stream).unwrap();
    std::fs::write(&key, format!("0x{}", "01".repeat(32))).unwrap();
    (base_path, stream_path, key)
}

fn bundle(dir: &Path) -> (PathBuf, PathBuf, SignedGenesis, serde_json::Value) {
    let (base, input, key) = inputs(dir);
    let bundle = dir.join("bundle");
    let report = build_bundle(
        &base,
        &input,
        &bundle,
        *signer(1).address().as_bytes(),
        arkiv_node::genesis_streaming::DEFAULT_BATCH_BYTES,
    )
    .unwrap();
    let genesis = decode_exact(&std::fs::read(bundle.join("genesis.bin")).unwrap()).unwrap();
    (bundle, key, genesis, report)
}

/// Leaves the complete staged store of an installation interrupted before
/// the relabelling in `run`.
fn stage(bundle: &Path, run: &Path, genesis: &SignedGenesis) {
    std::fs::create_dir_all(run).unwrap();
    prepare_streaming(
        genesis,
        &bundle.join("prefill.bin"),
        &run.join("prefill-work"),
        Options::default(),
    )
    .unwrap();
}

// The producer binary.

const TOKEN: &str = "adopt-test-token-0123456789abcdef0123";

struct Producer {
    child: Child,
    log: PathBuf,
    port: u16,
    private: u16,
}

impl Producer {
    fn start(
        dir: &Path,
        node: &Path,
        bundle: &Path,
        key: &Path,
        install: Option<&str>,
        tag: &str,
    ) -> Self {
        let (port, private) = (free_port(), free_port());
        let log = dir.join(format!("{tag}.log"));
        let mut command = Command::new(BIN);
        command
            .arg("producer")
            .arg("--run-dir")
            .arg(node)
            .arg("--bundle")
            .arg(bundle)
            .arg("--key-file")
            .arg(key)
            .arg("--listen")
            .arg(format!("127.0.0.1:{port}"))
            .arg("--private-listen")
            .arg(format!("127.0.0.1:{private}"))
            .env("ARKIV_PROPOSER_TOKEN", TOKEN);
        if let Some(install) = install {
            command.arg("--genesis-install").arg(install);
        }
        let file = std::fs::File::create(&log).unwrap();
        let child = Child(
            command
                .stdout(Stdio::null())
                .stderr(Stdio::from(file))
                .spawn()
                .unwrap(),
        );
        Self {
            child,
            log,
            port,
            private,
        }
    }

    fn ready(&mut self) -> Result<serde_json::Value, std::process::ExitStatus> {
        ready(&mut self.child, self.port, "/signed/v1/status", &self.log)
    }

    /// The full node's equality-page proof bytes for every fixture entity
    /// at height zero: identical state gives identical bytes.
    fn query(&self) -> serde_json::Value {
        let body = r#"{"height":"0","namespace":"1","attribute":"kind","valueType":"str","value":"fixture","limit":64,"cursor":null}"#;
        let resolved = http(
            self.port,
            "POST",
            "/signed/v1/query/resolve",
            Some(body),
            None,
        )
        .unwrap();
        let request = serde_json::json!({"request": resolved["request"]}).to_string();
        http(
            self.port,
            "POST",
            "/signed/v1/query/eq",
            Some(&request),
            None,
        )
        .unwrap()
    }

    fn log(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }
}

fn assert_genesis_head(status: &serde_json::Value, report: &serde_json::Value) {
    let head = &status["head"];
    assert_eq!(head["height"], "0");
    assert_eq!(head["hash"], report["genesisHash"]);
    assert_eq!(head["stateRoot"], report["stateRoot"]);
}

#[test]
fn producer_adopts_serves_the_copy_s_state_and_reuses_a_staged_store() {
    let tmp = temp("cli");
    let (bundle, key, genesis, report) = bundle(&tmp.0);
    let mut copy = Producer::start(&tmp.0, &tmp.0.join("copy"), &bundle, &key, None, "copy");
    let copied = copy
        .ready()
        .unwrap_or_else(|c| panic!("{c}: {}", copy.log()));
    assert_genesis_head(&copied, &report);
    let expected = copy.query();
    let proof = expected["proof"]
        .as_str()
        .unwrap_or_else(|| panic!("{expected}"));
    // Every fixture entity's unindexed body is in the proof page.
    assert!(
        proof.contains(&alloy_primitives::hex::encode(b"entity 47 ")),
        "{expected}"
    );
    assert!(!copy.log().contains("prefill_genesis_adopt"));
    drop(copy);

    let node = tmp.0.join("adopt");
    let mut adopt = Producer::start(&tmp.0, &node, &bundle, &key, Some("adopt"), "adopt");
    let adopted = adopt
        .ready()
        .unwrap_or_else(|c| panic!("{c}: {}", adopt.log()));
    assert_eq!(adopted["head"], copied["head"]);
    assert_eq!(adopted["identity"], copied["identity"]);
    assert_eq!(adopt.query(), expected);
    let log = adopt.log();
    assert!(log.contains(r#""stage":"relabelled""#), "{log}");
    assert!(log.contains(r#""resumed":false"#), "{log}");
    assert!(!node.join("prefill-work").exists());
    // The adopted node produces and restarts like any other.
    let stepped = http(
        adopt.private,
        "POST",
        "/signed/v1/control",
        Some(r#"{"action":"step"}"#),
        Some(TOKEN),
    )
    .unwrap();
    assert_eq!(stepped["head"]["height"], "1", "{stepped}");
    drop(adopt);
    let mut restarted = Producer::start(&tmp.0, &node, &bundle, &key, Some("adopt"), "restart");
    assert_eq!(restarted.ready().unwrap()["head"], stepped["head"]);
    assert!(!restarted.log().contains("prefill_genesis_adopt"));
    drop(restarted);

    // A cut before the relabelling reuses the staged store without a rebuild.
    let node = tmp.0.join("staged");
    stage(&bundle, &node, &genesis);
    let mut reused = Producer::start(&tmp.0, &node, &bundle, &key, Some("adopt"), "reused");
    assert_eq!(reused.ready().unwrap()["head"], copied["head"]);
    let log = reused.log();
    assert!(log.contains(r#""resumed":false"#), "{log}");
    assert!(!log.contains("bulk_normalize"), "{log}");
}

#[test]
fn producer_refuses_a_damaged_staged_store_and_misplaced_flags() {
    let tmp = temp("refuse");
    let (bundle, key, genesis, report) = bundle(&tmp.0);
    let node = tmp.0.join("node");
    stage(&bundle, &node, &genesis);
    let staged = genesis_adopt::staged_path(&node.join("prefill-work"));
    let mut bytes = std::fs::read(&staged).unwrap();
    let mut found = 0;
    for at in 0..bytes.len() - MARKER.len() {
        if &bytes[at..at + MARKER.len()] == MARKER {
            bytes[at + MARKER.len() - 1] ^= 1;
            found += 1;
        }
    }
    assert!(found > 0, "fixture marker is stored in the staged store");
    std::fs::write(&staged, bytes).unwrap();
    for attempt in ["damaged", "damaged-again"] {
        let mut damaged = Producer::start(&tmp.0, &node, &bundle, &key, Some("adopt"), attempt);
        assert!(!damaged.ready().unwrap_err().success());
        assert!(damaged.log().contains("Corrupt"), "{}", damaged.log());
        // Verification failed before the relabelling: nothing was consumed.
        assert_eq!(genesis_adopt::inspect(&staged).unwrap(), Staged::Engine);
        assert!(
            RedbStore::open(node.join("node.redb"), RedbConfig::default())
                .unwrap()
                .head()
                .unwrap()
                .is_none()
        );
    }

    let allocations = tmp.0.join("allocations.json");
    std::fs::write(
        &allocations,
        format!(r#"{{"{}":"1000000000000000000"}}"#, address()),
    )
    .unwrap();
    let listen = format!("127.0.0.1:{}", free_port());
    let hash = report["genesisHash"].as_str().unwrap().to_string();
    let path = |name: &str| tmp.0.join(name).display().to_string();
    let follower = |role: &str| {
        let mut args: Vec<String> = vec![
            role.into(),
            "--run-dir".into(),
            path(role),
            "--peer".into(),
            "http://127.0.0.1:9".into(),
            "--genesis-hash".into(),
            hash.clone(),
            "--proposer".into(),
            address(),
            "--chain-id".into(),
            "9452".into(),
            "--source-id".into(),
            "00000000-0000-4000-8000-000000000001".into(),
            "--run-id".into(),
            "09".repeat(16),
            "--listen".into(),
            listen.clone(),
        ];
        if role == "full" {
            args.extend(["--replication-peer".into(), "http://127.0.0.1:9".into()]);
        }
        args
    };
    let command_lines: Vec<Vec<String>> = vec![
        // No streaming staged store to adopt.
        vec![
            "producer".into(),
            "--run-dir".into(),
            path("alloc"),
            "--key-file".into(),
            key.display().to_string(),
            "--allocations-file".into(),
            allocations.display().to_string(),
            "--listen".into(),
            listen.clone(),
            "--genesis-install".into(),
            "adopt".into(),
        ],
        // Unknown mode.
        vec![
            "producer".into(),
            "--run-dir".into(),
            path("bogus"),
            "--key-file".into(),
            key.display().to_string(),
            "--bundle".into(),
            bundle.display().to_string(),
            "--listen".into(),
            listen.clone(),
            "--genesis-install".into(),
            "move".into(),
        ],
        [
            follower("full"),
            vec!["--genesis-install".into(), "adopt".into()],
        ]
        .concat(),
        [
            follower("light"),
            vec!["--genesis-install".into(), "copy".into()],
        ]
        .concat(),
    ];
    for args in command_lines {
        let stderr = refused(BIN, &args, &[("ARKIV_PROPOSER_TOKEN", TOKEN)]);
        assert!(stderr.contains("InvalidRequest"), "{args:?}: {stderr}");
    }
}
