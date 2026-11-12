//! `--state-retention` wiring in the node binaries.

mod cli_support;

use cli_support::{Child, free_port, refused};
use std::{
    process::{Command, Stdio},
    time::Duration,
};

fn run_dir(tag: &str) -> std::path::PathBuf {
    let path =
        std::env::temp_dir().join(format!("arkiv-retention-cli-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    path
}

#[test]
fn light_roles_and_malformed_values_are_rejected() {
    let sim = env!("CARGO_BIN_EXE_arkiv-simulator");
    let dir = run_dir("sim-light");
    let d = dir.to_str().unwrap();
    for args in [
        vec![
            "light",
            "--unsigned-simulator",
            "--run-dir",
            d,
            "--state-retention",
            "5",
        ],
        vec![
            "producer",
            "--unsigned-simulator",
            "--run-dir",
            d,
            "--state-retention",
            "-1",
        ],
        vec![
            "producer",
            "--unsigned-simulator",
            "--run-dir",
            d,
            "--state-retention",
            "Archive",
        ],
    ] {
        let args: Vec<String> = args.into_iter().map(String::from).collect();
        assert!(
            refused(sim, &args, &[]).contains("InvalidRequest"),
            "{args:?}"
        );
    }
    let proposer = env!("CARGO_BIN_EXE_arkiv-proposer");
    let dir = run_dir("proposer-light");
    assert!(
        refused(
            proposer,
            &[
                "light".into(),
                "--run-dir".into(),
                dir.display().to_string(),
                "--state-retention".into(),
                "2".into(),
            ],
            &[],
        )
        .contains("InvalidRequest")
    );
    let _ = std::fs::remove_dir_all(run_dir("sim-light"));
    let _ = std::fs::remove_dir_all(run_dir("proposer-light"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn simulator_producer_reports_the_configured_window() {
    let listen = format!("127.0.0.1:{}", free_port());
    let dir = run_dir("sim-producer");
    let child = Child(
        Command::new(env!("CARGO_BIN_EXE_arkiv-simulator"))
            .args([
                "producer",
                "--unsigned-simulator",
                "--run-dir",
                dir.to_str().unwrap(),
                "--listen",
                &listen,
                "--state-retention",
                "3",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();
    let mut status = None;
    // Up to a minute for the child to start serving.
    for _ in 0..2_400 {
        if let Ok(r) = client
            .get(format!("http://{listen}/sim/v1/status"))
            .send()
            .await
        {
            status = Some(r.json::<serde_json::Value>().await.unwrap());
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let status = status.expect("simulator producer answers status");
    assert_eq!(status["stateRetention"], "window");
    assert_eq!(status["stateRetentionWindow"], "3");
    assert_eq!(status["stateFloor"], "0");
    drop(child);
    let _ = std::fs::remove_dir_all(dir);
}
