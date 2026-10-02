use std::path::PathBuf;

use crawlcontract::config::policy::Policy;
use crawlcontract::model::snapshot::Snapshot;
use crawlcontract::rules::canonical_consistency::CanonicalConsistencyRule;
use crawlcontract::rules::canonical_presence::CanonicalPresenceRule;
use crawlcontract::rules::canonical_resolution::CanonicalResolutionRule;
use crawlcontract::rules::hreflang_indexability::HreflangIndexabilityRule;
use crawlcontract::rules::hreflang_reciprocal::HreflangReciprocalRule;
use crawlcontract::rules::orphan::OrphanRule;
use crawlcontract::rules::registry::{run_all_rules, Rule};
use crawlcontract::rules::sitemap_coverage::SitemapCoverageRule;
use crawlcontract::rules::sitemap_indexability::SitemapIndexabilityRule;
use crawlcontract::scanner::static_dir::{build_snapshot, discover_files};
use url::Url;

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn base_url() -> Url {
    Url::parse("https://example.test").unwrap()
}

fn scan_fixture(name: &str) -> Snapshot {
    let result = discover_files(&fixture_path(name), &base_url()).unwrap();
    build_snapshot(result, None, &crawlcontract::scanner::default_user_agent()).unwrap()
}

#[test]
fn basic_site_clean_scan() {
    let snapshot = scan_fixture("basic-site");

    assert_eq!(
        snapshot.statistics.total_urls,
        4,
        "URLs: {:?}",
        snapshot.urls.keys().collect::<Vec<_>>()
    );
    assert_eq!(snapshot.statistics.indexable_urls, 4);
    assert_eq!(snapshot.statistics.orphan_urls, 0);
    assert_eq!(snapshot.statistics.internal_link_urls, 4);

    let rules: Vec<Box<dyn Rule>> = vec![
        Box::new(SitemapIndexabilityRule),
        Box::new(SitemapCoverageRule),
        Box::new(CanonicalConsistencyRule),
        Box::new(CanonicalPresenceRule),
        Box::new(CanonicalResolutionRule),
        Box::new(HreflangReciprocalRule),
        Box::new(HreflangIndexabilityRule),
        Box::new(OrphanRule),
    ];
    let findings = run_all_rules(&snapshot, &rules, None);
    assert!(
        findings.is_empty(),
        "Expected no findings, got: {:?}",
        findings
    );
}

#[test]
fn sitemap_conflict_detects_noindex() {
    let snapshot = scan_fixture("sitemap-conflict");

    let rules: Vec<Box<dyn Rule>> = vec![Box::new(SitemapIndexabilityRule)];
    let findings = run_all_rules(&snapshot, &rules, None);

    assert!(
        findings
            .iter()
            .any(|f| f.rule_id == "CC-SITEMAP-INDEXABILITY-001"),
        "Expected CC-SITEMAP-INDEXABILITY-001, got: {:?}",
        findings
    );
}

#[test]
fn canonical_cycle_detected() {
    let snapshot = scan_fixture("canonical-cycle");

    let rules: Vec<Box<dyn Rule>> = vec![Box::new(CanonicalResolutionRule)];
    let findings = run_all_rules(&snapshot, &rules, None);

    assert!(
        findings
            .iter()
            .any(|f| f.rule_id == "CC-CANONICAL-RESOLUTION-002"),
        "Expected CC-CANONICAL-RESOLUTION-002, got: {:?}",
        findings
    );
}

#[test]
fn orphan_page_detected() {
    let snapshot = scan_fixture("orphan-pages");

    let rules: Vec<Box<dyn Rule>> = vec![Box::new(OrphanRule)];
    let findings = run_all_rules(&snapshot, &rules, None);

    assert!(
        findings
            .iter()
            .any(|f| f.rule_id == "CC-ORPHAN-001" && f.url.contains("orphan")),
        "Expected CC-ORPHAN-001 for /orphan, got: {:?}",
        findings
    );
}

#[test]
fn hreflang_missing_reciprocity() {
    let snapshot = scan_fixture("hreflang-broken");

    let rules: Vec<Box<dyn Rule>> = vec![Box::new(HreflangReciprocalRule)];
    let findings = run_all_rules(&snapshot, &rules, None);

    // The es page doesn't exist in the scan, so it should flag as unreciprocated
    assert!(
        findings
            .iter()
            .any(|f| f.rule_id == "CC-HREFLANG-RECIPROCAL-001"),
        "Expected CC-HREFLANG-RECIPROCAL-001, got: {:?}",
        findings
    );
}

#[test]
fn policy_exclusion_works() {
    let snapshot = scan_fixture("orphan-pages");

    let policy_str = r#"
[[exclusions]]
rule_id = "CC-ORPHAN-001"
url_pattern = "https://example.test/orphan"
reason = "Known orphan, excluded"
"#;
    let policy: Policy = toml::from_str(policy_str).unwrap();

    let rules: Vec<Box<dyn Rule>> = vec![Box::new(OrphanRule)];
    let findings = run_all_rules(&snapshot, &rules, Some(&policy));

    assert!(
        findings.is_empty(),
        "Expected no findings after exclusion, got: {:?}",
        findings
    );
}

#[test]
fn snapshot_serialization_roundtrip() {
    let snapshot = scan_fixture("basic-site");

    let json = crawlcontract::output::json::snapshot_to_json(&snapshot).unwrap();
    let restored = crawlcontract::output::json::snapshot_from_json(&json).unwrap();

    assert_eq!(
        snapshot.statistics.total_urls,
        restored.statistics.total_urls
    );
    assert_eq!(snapshot.urls.len(), restored.urls.len());
}

#[test]
fn indexable_page_missing_from_the_sitemap_is_reported() {
    let snapshot = scan_fixture("coverage-gap");

    let findings = run_all_rules(&snapshot, &[Box::new(SitemapCoverageRule)], None);

    assert!(
        findings
            .iter()
            .any(|f| f.rule_id == "CC-SITEMAP-COVERAGE-001" && f.url.ends_with("/extra")),
        "Expected CC-SITEMAP-COVERAGE-001 for /extra, got: {findings:?}"
    );
    assert!(
        !findings.iter().any(|f| f.url == "https://example.test/"),
        "URLs declared in the sitemap must not be reported: {findings:?}"
    );
}

#[test]
fn canonical_presence_rule_uses_html_and_http_signals() {
    let snapshot = scan_fixture("canonical-missing");

    let findings = run_all_rules(&snapshot, &[Box::new(CanonicalPresenceRule)], None);

    assert_eq!(
        findings.len(),
        1,
        "Expected exactly one missing canonical, got: {findings:?}"
    );
    assert_eq!(findings[0].rule_id, "CC-CANONICAL-PRESENCE-001");
    assert!(findings[0].url.ends_with("/no-canonical"));
}

#[test]
fn hreflang_targeting_a_noindex_page_is_reported() {
    let snapshot = scan_fixture("hreflang-noindex");

    let findings = run_all_rules(&snapshot, &[Box::new(HreflangIndexabilityRule)], None);

    assert!(
        findings
            .iter()
            .any(|f| f.rule_id == "CC-HREFLANG-INDEXABILITY-001" && f.url.contains("/en/")),
        "Expected CC-HREFLANG-INDEXABILITY-001, got: {findings:?}"
    );
}
