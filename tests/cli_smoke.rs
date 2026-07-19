use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;

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
fn live_scan_entry_503_exits_as_scan_failure() {
    let listener = match TcpListener::bind("127.0.0.1:0") {
        Ok(listener) => listener,
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => return,
        Err(error) => panic!("binding test server: {error}"),
    };
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        for _ in 0..3 {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 4096];
            let bytes_read = stream.read(&mut request).unwrap();
            assert!(bytes_read > 0);
            let body = "unavailable";
            let response = format!(
                "HTTP/1.1 503 Service Unavailable\r\nContent-Type: text/plain\r\n\
                 Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).unwrap();
        }
    });

    Command::cargo_bin("crawlcontract")
        .unwrap()
        .args(["scan", &format!("http://{address}/")])
        .assert()
        .code(2)
        .stdout(predicate::str::contains("No issues found").not())
        .stderr(predicate::str::contains("terminal status: HTTP 503"));

    server.join().unwrap();
}

#[test]
fn version_flag() {
    Command::cargo_bin("crawlcontract")
        .unwrap()
        .args(["--version"])
        .assert()
        .success()
        .stdout(predicate::str::contains("crawlcontract 0.3.1"));
}

#[test]
fn scan_rejects_unknown_output_format() {
    Command::cargo_bin("crawlcontract")
        .unwrap()
        .args(["scan", &fixture_path("basic-site"), "--format", "xml"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("invalid value 'xml'"));
}

#[test]
fn scan_rejects_output_path_without_file_format() {
    let output = tempfile::NamedTempFile::new().unwrap();
    Command::cargo_bin("crawlcontract")
        .unwrap()
        .args([
            "scan",
            &fixture_path("basic-site"),
            "--public-origin",
            "https://example.test",
            "--output",
            output.path().to_str().unwrap(),
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "--output requires json, markdown, or sarif output",
        ));
}

#[test]
fn scan_rejects_multiple_machine_formats() {
    Command::cargo_bin("crawlcontract")
        .unwrap()
        .args([
            "scan",
            &fixture_path("basic-site"),
            "--format",
            "json,sarif",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "select at most one of json, markdown, or sarif",
        ));
}

#[test]
fn policy_public_origin_takes_precedence_over_cli() {
    let temp = tempfile::tempdir().unwrap();
    let policy_path = temp.path().join("crawlcontract.toml");
    let snapshot_path = temp.path().join("snapshot.json");
    fs::write(
        &policy_path,
        "[general]\npublic_origin = \"https://example.test\"\n",
    )
    .unwrap();

    Command::cargo_bin("crawlcontract")
        .unwrap()
        .args([
            "scan",
            &fixture_path("basic-site"),
            "--public-origin",
            "https://ignored.test",
            "--policy",
            policy_path.to_str().unwrap(),
            "--snapshot",
            snapshot_path.to_str().unwrap(),
        ])
        .assert()
        .success();

    let snapshot: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(snapshot_path).unwrap()).unwrap();
    assert_eq!(snapshot["base_url"], "https://example.test/");
}

#[test]
fn scan_rejects_invalid_policy_threshold() {
    let temp = tempfile::tempdir().unwrap();
    let policy_path = temp.path().join("crawlcontract.toml");
    fs::write(&policy_path, "[diff]\nmax_word_loss_percent = 101\n").unwrap();

    Command::cargo_bin("crawlcontract")
        .unwrap()
        .args([
            "scan",
            &fixture_path("basic-site"),
            "--policy",
            policy_path.to_str().unwrap(),
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "max_word_loss_percent must be between 0 and 100",
        ));
}

#[test]
fn scan_rejects_policy_typos_and_unknown_exclusion_rules() {
    let temp = tempfile::tempdir().unwrap();
    let policy_path = temp.path().join("crawlcontract.toml");
    fs::write(
        &policy_path,
        "[general]\npublic_orgin = \"https://example.test\"\n",
    )
    .unwrap();

    Command::cargo_bin("crawlcontract")
        .unwrap()
        .args([
            "scan",
            &fixture_path("basic-site"),
            "--policy",
            policy_path.to_str().unwrap(),
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("unknown field `public_orgin`"));

    fs::write(
        &policy_path,
        "[[exclusions]]\nrule_id = \"CC-ORPHAN-999\"\nurl_pattern = \"/admin/**\"\n\
         reason = \"Known exception\"\n",
    )
    .unwrap();

    Command::cargo_bin("crawlcontract")
        .unwrap()
        .args([
            "scan",
            &fixture_path("basic-site"),
            "--policy",
            policy_path.to_str().unwrap(),
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "rule_id is not a known finding rule",
        ));
}
