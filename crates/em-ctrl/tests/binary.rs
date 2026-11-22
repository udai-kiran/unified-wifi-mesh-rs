// SPDX-License-Identifier: Apache-2.0

//! The `em-ctrl` binary refuses to start without a usable reset file (0013 D2,
//! `reset-file-required`).

use std::path::PathBuf;
use std::process::Command;

fn temp_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("em-ctrl-binary-test-{}-{name}", std::process::id()))
}

fn run_with_reset_file(path: &PathBuf) -> (Option<i32>, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_em-ctrl"))
        .arg("--control")
        .arg(temp_path("control"))
        .arg("--data")
        .arg(temp_path("data"))
        .arg("--reset-file")
        .arg(path)
        .output()
        .unwrap();
    (
        output.status.code(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
    )
}

// intentional-difference:reset-file-required
// Upstream logs a missing file and continues with no SSIDs
// (`dm_easy_mesh_ctrl.cpp:10855-10858`).
#[test]
fn a_missing_reset_file_is_fatal() {
    let (code, stdout) = run_with_reset_file(&temp_path("missing.json"));
    assert_eq!(code, Some(1));
    assert!(stdout.contains("the reset file is required"), "{stdout}");
}

#[test]
fn an_invalid_reset_file_is_fatal() {
    let path = temp_path("invalid.json");
    std::fs::write(&path, "{ not json").unwrap();
    let (code, stdout) = run_with_reset_file(&path);
    std::fs::remove_file(&path).unwrap();
    assert_eq!(code, Some(1));
    assert!(stdout.contains("the reset file is required"), "{stdout}");
}
