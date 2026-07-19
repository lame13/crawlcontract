use crate::model::snapshot::Snapshot;
use crate::model::url_state::RobotsTxtStatus;
use crate::rules::registry::{Evidence, Finding, Rule, Severity};

/// CC-SITEMAP-INDEXABILITY-001: URL in sitemap but effective directive is noindex.
/// CC-SITEMAP-INDEXABILITY-002: URL in sitemap but robots.txt blocks crawling.
/// CC-SITEMAP-INDEXABILITY-003: URL in sitemap but canonicalizes elsewhere.
/// CC-SITEMAP-INDEXABILITY-004: URL in sitemap but does not resolve successfully.
pub struct SitemapIndexabilityRule;

impl Rule for SitemapIndexabilityRule {
    fn id(&self) -> &str {
        "CC-SITEMAP-INDEXABILITY"
    }

    fn evaluate(&self, snapshot: &Snapshot) -> Vec<Finding> {
        let mut findings = Vec::new();

        for state in snapshot.urls.values() {
            if !state.found_in_sitemap {
                continue;
            }

            // 001: noindex
            if state.effective_robots.is_noindex() {
                findings.push(Finding {
                    rule_id: "CC-SITEMAP-INDEXABILITY-001".to_string(),
                    severity: Severity::Error,
                    url: state.url.to_string(),
                    message: "The sitemap declares this URL for indexing, but the effective \
                              indexing directive excludes it."
                        .to_string(),
                    evidence: Evidence {
                        declared: Some("sitemap.xml".to_string()),
                        observed: Some("noindex".to_string()),
                        canonical: state.effective_canonical.as_ref().map(|u| u.to_string()),
                        detail: None,
                    },
                });
            }

            // 002: robots.txt blocked
            if state.robots_txt_status == RobotsTxtStatus::Blocked {
                findings.push(Finding {
                    rule_id: "CC-SITEMAP-INDEXABILITY-002".to_string(),
                    severity: Severity::Error,
                    url: state.url.to_string(),
                    message: "The sitemap declares this URL for indexing, but robots.txt \
                              disallows crawling it."
                        .to_string(),
                    evidence: Evidence {
                        declared: Some("sitemap.xml".to_string()),
                        observed: Some("robots.txt: disallow".to_string()),
                        canonical: None,
                        detail: None,
                    },
                });
            }

            // 003: canonicalizes elsewhere
            if let Some(ref canonical) = state.effective_canonical {
                if canonical != &state.url {
                    findings.push(Finding {
                        rule_id: "CC-SITEMAP-INDEXABILITY-003".to_string(),
                        severity: Severity::Warning,
                        url: state.url.to_string(),
                        message: "The sitemap declares this URL, but it canonicalizes to a \
                                  different URL. The sitemap should contain the canonical URL."
                            .to_string(),
                        evidence: Evidence {
                            declared: Some("sitemap.xml".to_string()),
                            observed: None,
                            canonical: Some(canonical.to_string()),
                            detail: None,
                        },
                    });
                }
            }

            // 004: sitemap target must be verified as a successful page response
            if state.robots_txt_status != RobotsTxtStatus::Blocked
                && !state
                    .http_status
                    .map(|status| status == 200)
                    .unwrap_or(false)
            {
                let observed = state
                    .http_status
                    .map(|status| format!("HTTP {status}"))
                    .unwrap_or_else(|| "not fetched or absent from static output".to_string());
                findings.push(Finding {
                    rule_id: "CC-SITEMAP-INDEXABILITY-004".to_string(),
                    severity: Severity::Error,
                    url: state.url.to_string(),
                    message: "The sitemap declares this URL, but it did not resolve to a \
                              successful page response."
                        .to_string(),
                    evidence: Evidence {
                        declared: Some("sitemap.xml".to_string()),
                        observed: Some(observed),
                        canonical: state
                            .effective_canonical
                            .as_ref()
                            .map(|url| url.to_string()),
                        detail: None,
                    },
                });
            }
        }

        findings
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::robots::RobotsDirective;
    use crate::model::snapshot::{Snapshot, Statistics};
    use crate::model::url_state::{RobotsTxtStatus, UrlSource, UrlState};
    use std::collections::BTreeMap;
    use url::Url;

    fn make_snapshot(urls: BTreeMap<String, UrlState>) -> Snapshot {
        Snapshot {
            version: "1.0".to_string(),
            tool: "crawlcontract".to_string(),
            base_url: Url::parse("https://example.test").unwrap(),
            public_origin: None,
            scanned_at: crate::model::snapshot::utc_now(),
            statistics: Statistics::from_url_states(&urls),
            urls,
        }
    }

    #[test]
    fn sitemap_url_with_noindex_fails_001() {
        let mut urls = BTreeMap::new();
        let url = Url::parse("https://example.test/page").unwrap();
        let mut state = UrlState::new(url.clone());
        state.found_in_sitemap = true;
        state.add_source(UrlSource::Sitemap);
        state.effective_robots = RobotsDirective {
            noindex: Some(true),
            ..Default::default()
        };
        state.http_status = Some(200);
        state.is_indexable = false;
        urls.insert(url.to_string(), state);

        let snapshot = make_snapshot(urls);
        let findings = SitemapIndexabilityRule.evaluate(&snapshot);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule_id, "CC-SITEMAP-INDEXABILITY-001");
    }

    #[test]
    fn sitemap_url_with_robots_block_fails_002() {
        let mut urls = BTreeMap::new();
        let url = Url::parse("https://example.test/admin").unwrap();
        let mut state = UrlState::new(url.clone());
        state.found_in_sitemap = true;
        state.add_source(UrlSource::Sitemap);
        state.robots_txt_status = RobotsTxtStatus::Blocked;
        state.is_indexable = false;
        urls.insert(url.to_string(), state);

        let snapshot = make_snapshot(urls);
        let findings = SitemapIndexabilityRule.evaluate(&snapshot);
        assert!(findings
            .iter()
            .any(|f| f.rule_id == "CC-SITEMAP-INDEXABILITY-002"));
    }

    #[test]
    fn sitemap_url_canonicalizes_elsewhere_fails_003() {
        let mut urls = BTreeMap::new();
        let url = Url::parse("https://example.test/old-page").unwrap();
        let canonical = Url::parse("https://example.test/new-page").unwrap();
        let mut state = UrlState::new(url.clone());
        state.found_in_sitemap = true;
        state.add_source(UrlSource::Sitemap);
        state.effective_canonical = Some(canonical);
        state.is_indexable = false;
        urls.insert(url.to_string(), state);

        let snapshot = make_snapshot(urls);
        let findings = SitemapIndexabilityRule.evaluate(&snapshot);
        assert!(findings
            .iter()
            .any(|f| f.rule_id == "CC-SITEMAP-INDEXABILITY-003"));
    }

    #[test]
    fn sitemap_url_indexable_passes() {
        let mut urls = BTreeMap::new();
        let url = Url::parse("https://example.test/page").unwrap();
        let mut state = UrlState::new(url.clone());
        state.found_in_sitemap = true;
        state.add_source(UrlSource::Sitemap);
        state.is_indexable = true;
        state.http_status = Some(200);
        urls.insert(url.to_string(), state);

        let snapshot = make_snapshot(urls);
        let findings = SitemapIndexabilityRule.evaluate(&snapshot);
        assert!(findings.is_empty());
    }

    #[test]
    fn sitemap_url_that_does_not_resolve_fails_004() {
        let mut urls = BTreeMap::new();
        let url = Url::parse("https://example.test/missing").unwrap();
        let mut state = UrlState::new(url.clone());
        state.found_in_sitemap = true;
        state.add_source(UrlSource::Sitemap);
        state.http_status = Some(404);
        urls.insert(url.to_string(), state);

        let findings = SitemapIndexabilityRule.evaluate(&make_snapshot(urls));
        assert!(findings
            .iter()
            .any(|finding| finding.rule_id == "CC-SITEMAP-INDEXABILITY-004"));
    }
}
