use crate::model::snapshot::Snapshot;
use crate::model::url_state::RobotsTxtStatus;
use crate::rules::registry::{Evidence, Finding, Rule, Severity};
use crate::scanner::html_signals::normalize_url_key;

/// CC-HREFLANG-INDEXABILITY-001: An hreflang target exists but cannot be indexed.
///
/// Hreflang only works between indexable pages: an alternate that returns a
/// non-200 status, is blocked by robots.txt, or carries `noindex` is discarded.
/// Targets whose only problem is canonicalizing elsewhere are covered by
/// `CC-HREFLANG-CANONICAL-001` and are not reported here.
pub struct HreflangIndexabilityRule;

impl Rule for HreflangIndexabilityRule {
    fn id(&self) -> &str {
        "CC-HREFLANG-INDEXABILITY"
    }

    fn evaluate(&self, snapshot: &Snapshot) -> Vec<Finding> {
        let mut findings = Vec::new();

        for state in snapshot.urls.values() {
            for entry in &state.hreflang {
                let target_key = normalize_url_key(&entry.url);
                let Some(target) = snapshot.urls.get(&target_key) else {
                    continue;
                };

                // Live scans deliberately do not fetch robots-blocked targets.
                let reason = if target.robots_txt_status == RobotsTxtStatus::Blocked {
                    "robots.txt disallows crawling it".to_string()
                } else {
                    match target.http_status {
                        Some(status) if status != 200 => format!("it returns HTTP {status}"),
                        Some(200) if target.effective_robots.is_noindex() => {
                            "it is marked noindex".to_string()
                        }
                        _ => continue,
                    }
                };

                findings.push(Finding {
                    rule_id: "CC-HREFLANG-INDEXABILITY-001".to_string(),
                    severity: Severity::Error,
                    url: state.url.to_string(),
                    message: format!(
                        "This page declares hreflang {} → {}, but that URL cannot be indexed \
                         because {reason}. Hreflang annotations are ignored when the alternate \
                         is not indexable.",
                        entry.lang, entry.url
                    ),
                    evidence: Evidence {
                        declared: Some(format!("hreflang: {} → {}", entry.lang, entry.url)),
                        observed: Some(reason),
                        canonical: target
                            .effective_canonical
                            .as_ref()
                            .map(|url| url.to_string()),
                        detail: target
                            .http_status
                            .map(|status| format!("hreflang target returned HTTP {status}")),
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
    use crate::model::hreflang::HreflangEntry;
    use crate::model::robots::RobotsDirective;
    use crate::model::snapshot::Statistics;
    use crate::model::url_state::UrlState;
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

    fn source_page(target: &Url) -> (String, UrlState) {
        let url = Url::parse("https://example.test/en/page").unwrap();
        let mut state = UrlState::new(url.clone());
        state.hreflang = vec![HreflangEntry {
            lang: "es".to_string(),
            url: target.clone(),
        }];
        (url.to_string(), state)
    }

    #[test]
    fn noindex_alternate_is_reported() {
        let target = Url::parse("https://example.test/es/page").unwrap();
        let mut target_state = UrlState::new(target.clone());
        target_state.http_status = Some(200);
        target_state.effective_robots = RobotsDirective {
            noindex: Some(true),
            ..Default::default()
        };

        let urls = BTreeMap::from([source_page(&target), (target.to_string(), target_state)]);

        let findings = HreflangIndexabilityRule.evaluate(&make_snapshot(urls));

        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule_id, "CC-HREFLANG-INDEXABILITY-001");
    }

    #[test]
    fn blocked_alternate_is_reported() {
        let target = Url::parse("https://example.test/es/page").unwrap();
        let mut target_state = UrlState::new(target.clone());
        // A live scan cannot fetch this target, so there is no HTTP status.
        target_state.robots_txt_status = RobotsTxtStatus::Blocked;

        let urls = BTreeMap::from([source_page(&target), (target.to_string(), target_state)]);

        let findings = HreflangIndexabilityRule.evaluate(&make_snapshot(urls));

        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn broken_alternate_is_reported() {
        let target = Url::parse("https://example.test/es/page").unwrap();
        let mut target_state = UrlState::new(target.clone());
        target_state.http_status = Some(404);

        let urls = BTreeMap::from([source_page(&target), (target.to_string(), target_state)]);

        let findings = HreflangIndexabilityRule.evaluate(&make_snapshot(urls));

        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn healthy_alternate_passes() {
        let target = Url::parse("https://example.test/es/page").unwrap();
        let mut target_state = UrlState::new(target.clone());
        target_state.http_status = Some(200);
        target_state.effective_canonical = Some(target.clone());
        target_state.is_indexable = true;

        let urls = BTreeMap::from([source_page(&target), (target.to_string(), target_state)]);

        let findings = HreflangIndexabilityRule.evaluate(&make_snapshot(urls));

        assert!(findings.is_empty());
    }

    #[test]
    fn unverified_external_alternate_is_skipped() {
        let target = Url::parse("https://other.example/es/page").unwrap();
        let urls = BTreeMap::from([
            source_page(&target),
            (target.to_string(), UrlState::new(target.clone())),
        ]);

        let findings = HreflangIndexabilityRule.evaluate(&make_snapshot(urls));

        assert!(findings.is_empty());
    }

    #[test]
    fn canonical_mismatch_is_left_to_the_canonical_rule() {
        let target = Url::parse("https://example.test/es/page").unwrap();
        let mut target_state = UrlState::new(target.clone());
        target_state.http_status = Some(200);
        target_state.effective_canonical =
            Some(Url::parse("https://example.test/es/other").unwrap());
        target_state.is_indexable = false;

        let urls = BTreeMap::from([source_page(&target), (target.to_string(), target_state)]);

        let findings = HreflangIndexabilityRule.evaluate(&make_snapshot(urls));

        assert!(findings.is_empty());
    }
}
