use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;

fn fixture_path(name: &str) -> String {
    format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))
}

/// Scan a fixture into a snapshot file.
fn write_snapshot(fixture: &str, path: &std::path::Path) {
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
        .stdout(predicate::str::contains(format!(
            "crawlcontract {}",
            env!("CARGO_PKG_VERSION")
        )));
}

#[test]
fn scan_with_baseline_emits_a_diff_payload() {
    let temp = tempfile::tempdir().unwrap();
    let baseline = temp.path().join("baseline.json");
    write_snapshot("basic-site", &baseline);

    // The scan itself gates the regression (exit 1) and still reports the full
    // baseline comparison in the JSON payload.
    let assert = Command::cargo_bin("crawlcontract")
        .unwrap()
        .args([
            "scan",
            &fixture_path("orphan-pages"),
            "--public-origin",
            "https://example.test",
            "--baseline",
            baseline.to_str().unwrap(),
            "--format",
            "json",
        ])
        .assert()
        .code(1);

    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let report: serde_json::Value = serde_json::from_str(&stdout).unwrap();

    assert_eq!(report["command"], "scan");
    assert_eq!(
        report["diff"]["baseline"]["base_url"],
        "https://example.test/"
    );
    assert!(report["diff"]["gained_urls"].is_array());
    assert!(report["diff"]["counts"]["gained"].as_u64().unwrap() > 0);
}

#[test]
fn fail_on_new_ignores_findings_that_already_existed() {
    let temp = tempfile::tempdir().unwrap();
    let baseline = temp.path().join("baseline.json");
    // This fixture has one CC-SITEMAP-COVERAGE-001 warning.
    write_snapshot("coverage-gap", &baseline);

    // Without a baseline, failing on warnings trips on the pre-existing one.
    Command::cargo_bin("crawlcontract")
        .unwrap()
        .args([
            "scan",
            &fixture_path("coverage-gap"),
            "--public-origin",
            "https://example.test",
            "--fail-on",
            "warning",
        ])
        .assert()
        .code(1);

    // Against the baseline, the same finding is not new, so the gate passes.
    let assert = Command::cargo_bin("crawlcontract")
        .unwrap()
        .args([
            "scan",
            &fixture_path("coverage-gap"),
            "--public-origin",
            "https://example.test",
            "--baseline",
            baseline.to_str().unwrap(),
            "--fail-on-new",
            "--fail-on",
            "warning",
            "--format",
            "json",
        ])
        .assert()
        .success();

    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let report: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(report["summary"]["new"], 0);
    assert_eq!(report["summary"]["total"], 1);
    assert_eq!(report["findings"][0]["new"], false);
}

#[test]
fn fail_on_new_gates_a_regression_introduced_since_the_baseline() {
    let temp = tempfile::tempdir().unwrap();
    let baseline = temp.path().join("baseline.json");
    write_snapshot("basic-site", &baseline);

    let assert = Command::cargo_bin("crawlcontract")
        .unwrap()
        .args([
            "scan",
            &fixture_path("sitemap-conflict"),
            "--public-origin",
            "https://example.test",
            "--baseline",
            baseline.to_str().unwrap(),
            "--fail-on-new",
            "--format",
            "json",
        ])
        .assert()
        .code(1);

    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let report: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(report["summary"]["new"].as_u64().unwrap() > 0);
}

#[test]
fn fail_on_new_requires_a_baseline() {
    Command::cargo_bin("crawlcontract")
        .unwrap()
        .args(["scan", &fixture_path("basic-site"), "--fail-on-new"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("--baseline"));
}

#[test]
fn fail_on_new_distinguishes_hreflang_targets_on_the_same_page() {
    let temp = tempfile::tempdir().unwrap();
    let site = temp.path().join("site");
    fs::create_dir(&site).unwrap();
    let baseline = temp.path().join("baseline.json");
    let old = "<link rel=\"alternate\" hreflang=\"es\" href=\"/es\">";
    let new = "<link rel=\"alternate\" hreflang=\"fr\" href=\"/fr\">";
    fs::write(site.join("index.html"), old).unwrap();
    Command::cargo_bin("crawlcontract")
        .unwrap()
        .args([
            "scan",
            site.to_str().unwrap(),
            "--snapshot",
            baseline.to_str().unwrap(),
        ])
        .assert()
        .code(1);

    fs::write(site.join("index.html"), format!("{old}{new}")).unwrap();
    let assert = Command::cargo_bin("crawlcontract")
        .unwrap()
        .args([
            "scan",
            site.to_str().unwrap(),
            "--baseline",
            baseline.to_str().unwrap(),
            "--fail-on-new",
            "--format",
            "json",
        ])
        .assert()
        .code(1);
    let report: serde_json::Value = serde_json::from_slice(&assert.get_output().stdout).unwrap();
    let findings: Vec<_> = report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["rule_id"] == "CC-HREFLANG-RECIPROCAL-001")
        .collect();
    assert_eq!(findings.len(), 2);
    for finding in findings {
        let is_french = finding["evidence"]["declared"]
            .as_str()
            .unwrap()
            .contains("fr →");
        assert_eq!(finding["new"], is_french);
    }
}

#[test]
fn diff_json_lists_lost_and_gained_urls() {
    let temp = tempfile::tempdir().unwrap();
    let baseline = temp.path().join("baseline.json");
    let candidate = temp.path().join("candidate.json");
    write_snapshot("basic-site", &baseline);
    write_snapshot("orphan-pages", &candidate);

    let assert = Command::cargo_bin("crawlcontract")
        .unwrap()
        .args([
            "diff",
            baseline.to_str().unwrap(),
            candidate.to_str().unwrap(),
            "--format",
            "json",
        ])
        .assert()
        // The regression found by the diff is what makes the command exit 1.
        .code(1);

    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let report: serde_json::Value = serde_json::from_str(&stdout).unwrap();

    assert_eq!(report["command"], "diff");
    assert_eq!(report["diff"]["counts"]["lost"].as_u64().unwrap(), 3);
    assert_eq!(report["diff"]["indexable_urls"]["baseline"], 4);
    assert_eq!(report["diff"]["indexable_urls"]["candidate"], 3);
    assert_eq!(report["diff"]["indexable_urls"]["delta"], -1);
    let lost = report["diff"]["lost_urls"].as_array().unwrap();
    assert!(lost
        .iter()
        .any(|url| url.as_str().unwrap().ends_with("/about")));
}

#[test]
fn diff_honours_fail_on_severity() {
    let temp = tempfile::tempdir().unwrap();
    let baseline = temp.path().join("baseline.json");
    let candidate = temp.path().join("candidate.json");
    write_snapshot("basic-site", &baseline);
    write_snapshot("orphan-pages", &candidate);

    // A diff with no findings passes even when warnings gate the build.
    Command::cargo_bin("crawlcontract")
        .unwrap()
        .args([
            "diff",
            baseline.to_str().unwrap(),
            baseline.to_str().unwrap(),
            "--fail-on",
            "warning",
        ])
        .assert()
        .success();

    // A real regression fails the gate.
    Command::cargo_bin("crawlcontract")
        .unwrap()
        .args([
            "diff",
            baseline.to_str().unwrap(),
            candidate.to_str().unwrap(),
            "--fail-on",
            "warning",
        ])
        .assert()
        .code(1);
}

#[test]
fn scan_rejects_malformed_headers() {
    Command::cargo_bin("crawlcontract")
        .unwrap()
        .args([
            "scan",
            &fixture_path("basic-site"),
            "--header",
            "not-a-header",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "header must use the NAME: VALUE form",
        ));
}

#[test]
fn scan_rejects_invalid_live_tuning_flags() {
    Command::cargo_bin("crawlcontract")
        .unwrap()
        .args(["scan", &fixture_path("basic-site"), "--timeout", "0"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "--timeout must be greater than zero",
        ));

    Command::cargo_bin("crawlcontract")
        .unwrap()
        .args(["scan", &fixture_path("basic-site"), "--crawl-delay=-1"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("invalid --crawl-delay value"));
}

#[test]
fn markdown_report_includes_the_baseline_comparison() {
    let temp = tempfile::tempdir().unwrap();
    let baseline = temp.path().join("baseline.json");
    write_snapshot("basic-site", &baseline);

    // The report is still produced on the way to the failing exit code.
    let assert = Command::cargo_bin("crawlcontract")
        .unwrap()
        .args([
            "scan",
            &fixture_path("orphan-pages"),
            "--public-origin",
            "https://example.test",
            "--baseline",
            baseline.to_str().unwrap(),
            "--format",
            "markdown",
        ])
        .assert()
        .code(1);

    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert!(stdout.contains("## Baseline Comparison"));
    assert!(stdout.contains("### Lost URLs"));
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
