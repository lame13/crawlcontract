use std::path::PathBuf;

use crawlcontract::config::policy::Policy;
use crawlcontract::rules::canonical_consistency::CanonicalConsistencyRule;
use crawlcontract::rules::canonical_resolution::CanonicalResolutionRule;
use crawlcontract::rules::hreflang_reciprocal::HreflangReciprocalRule;
use crawlcontract::rules::orphan::OrphanRule;
use crawlcontract::rules::registry::{run_all_rules, Rule};
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

#[test]
fn basic_site_clean_scan() {
    let result = discover_files(&fixture_path("basic-site"), &base_url()).unwrap();
    let snapshot = build_snapshot(result, None);

    assert_eq!(
        snapshot.statistics.total_urls,
        5,
        "URLs: {:?}",
        snapshot.urls.keys().collect::<Vec<_>>()
    );
    assert_eq!(snapshot.statistics.indexable_urls, 5);
    assert_eq!(snapshot.statistics.orphan_urls, 1); // sitemap.xml is orphaned

    let rules: Vec<Box<dyn Rule>> = vec![
        Box::new(SitemapIndexabilityRule),
        Box::new(CanonicalConsistencyRule),
        Box::new(CanonicalResolutionRule),
        Box::new(HreflangReciprocalRule),
        Box::new(OrphanRule),
    ];
    let findings = run_all_rules(&snapshot, &rules, None);
    // sitemap.xml is orphaned (linked from robots.txt but not from any HTML page)
    let non_orphan_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.rule_id != "CC-ORPHAN-001" || !f.url.contains("sitemap"))
        .collect();
    assert!(
        non_orphan_findings.is_empty(),
        "Expected only sitemap orphan finding, got: {:?}",
        findings
    );
}

#[test]
fn sitemap_conflict_detects_noindex() {
    let result = discover_files(&fixture_path("sitemap-conflict"), &base_url()).unwrap();
    let snapshot = build_snapshot(result, None);

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
    let result = discover_files(&fixture_path("canonical-cycle"), &base_url()).unwrap();
    let snapshot = build_snapshot(result, None);

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
    let result = discover_files(&fixture_path("orphan-pages"), &base_url()).unwrap();
    let snapshot = build_snapshot(result, None);

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
    let result = discover_files(&fixture_path("hreflang-broken"), &base_url()).unwrap();
    let snapshot = build_snapshot(result, None);

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
    let result = discover_files(&fixture_path("orphan-pages"), &base_url()).unwrap();
    let snapshot = build_snapshot(result, None);

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
    let result = discover_files(&fixture_path("basic-site"), &base_url()).unwrap();
    let snapshot = build_snapshot(result, None);

    let json = crawlcontract::output::json::snapshot_to_json(&snapshot).unwrap();
    let restored = crawlcontract::output::json::snapshot_from_json(&json).unwrap();

    assert_eq!(
        snapshot.statistics.total_urls,
        restored.statistics.total_urls
    );
    assert_eq!(snapshot.urls.len(), restored.urls.len());
}
