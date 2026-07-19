use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;

fn fixture_path(name: &str) -> String {
    format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn scan_basic_site_exits_clean() {
    Command::cargo_bin("crawlcontract")
        .unwrap()
        .args([
            "scan",
            &fixture_path("basic-site"),
            "--public-origin",
            "https://example.test",
        ])
        .assert()
        .success();
}

#[test]
fn scan_with_json_output() {
    Command::cargo_bin("crawlcontract")
        .unwrap()
        .args([
            "scan",
            &fixture_path("basic-site"),
            "--public-origin",
            "https://example.test",
            "--format",
            "json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"version\""))
        .stdout(predicate::str::contains("\"findings\""));
}

#[test]
fn scan_with_sarif_output() {
    Command::cargo_bin("crawlcontract")
        .unwrap()
        .args([
            "scan",
            &fixture_path("basic-site"),
            "--public-origin",
            "https://example.test",
            "--format",
            "sarif",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"$schema\""))
        .stdout(predicate::str::contains("crawlcontract"));
}

#[test]
fn scan_with_snapshot_flag() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let snapshot_path = tmp.path().with_extension("json");

    Command::cargo_bin("crawlcontract")
        .unwrap()
        .args([
            "scan",
            &fixture_path("basic-site"),
            "--public-origin",
            "https://example.test",
            "--snapshot",
            snapshot_path.to_str().unwrap(),
        ])
        .assert()
        .success();

    let content = fs::read_to_string(&snapshot_path).unwrap();
    assert!(content.contains("\"version\""));
    assert!(content.contains("\"urls\""));
    let _ = fs::remove_file(snapshot_path);
}

#[test]
fn scan_orphan_site_with_policy_excludes() {
    Command::cargo_bin("crawlcontract")
        .unwrap()
        .args([
            "scan",
            &fixture_path("orphan-pages"),
            "--public-origin",
            "https://example.test",
        ])
        .assert()
        .success();
}

#[test]
fn diff_two_snapshots() {
    // Create two snapshot files
    let scan_to_snapshot = |fixture: &str, path: &std::path::Path| {
        Command::cargo_bin("crawlcontract")
            .unwrap()
            .args([
                "scan",
                &fixture_path(fixture),
                "--public-origin",
                "https://example.test",
                "--snapshot",
                path.to_str().unwrap(),
            ])
            .assert()
            .success();
    };

    let tmp1 = tempfile::NamedTempFile::new().unwrap();
    let baseline = tmp1.path().with_extension("json");
    let tmp2 = tempfile::NamedTempFile::new().unwrap();
    let candidate = tmp2.path().with_extension("json");

    scan_to_snapshot("basic-site", &baseline);
    scan_to_snapshot("basic-site", &candidate);

    Command::cargo_bin("crawlcontract")
        .unwrap()
        .args([
            "diff",
            baseline.to_str().unwrap(),
            candidate.to_str().unwrap(),
        ])
        .assert()
        .success();

    let _ = fs::remove_file(baseline);
    let _ = fs::remove_file(candidate);
}

#[test]
fn scan_invalid_directory_fails() {
    Command::cargo_bin("crawlcontract")
        .unwrap()
        .args(["scan", "/nonexistent/path"])
        .assert()
        .failure();
}

#[test]
fn version_flag() {
    Command::cargo_bin("crawlcontract")
        .unwrap()
        .args(["--version"])
        .assert()
        .success()
        .stdout(predicate::str::contains("crawlcontract"));
}
