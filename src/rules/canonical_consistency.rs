use crate::model::snapshot::Snapshot;
use crate::rules::registry::{Evidence, Finding, Rule, Severity};
use crate::scanner::html_signals::normalize_url_key;

/// CC-CANONICAL-CONSISTENCY-001: HTML canonical conflicts with HTTP Link header canonical.
/// CC-CANONICAL-CONSISTENCY-003: Canonical target does not resolve (non-200 or missing).
/// CC-CANONICAL-CONSISTENCY-004: Canonical target resolves but is not indexable.
pub struct CanonicalConsistencyRule;

impl Rule for CanonicalConsistencyRule {
    fn id(&self) -> &str {
        "CC-CANONICAL-CONSISTENCY"
    }

    fn evaluate(&self, snapshot: &Snapshot) -> Vec<Finding> {
        let mut findings = Vec::new();

        for state in snapshot.urls.values() {
            // 001: HTML canonical != HTTP canonical
            if let (Some(html), Some(http)) = (&state.html_canonical, &state.http_canonical) {
                if html != http {
                    findings.push(Finding {
                        rule_id: "CC-CANONICAL-CONSISTENCY-001".to_string(),
                        severity: Severity::Error,
                        url: state.url.to_string(),
                        message: "The HTML canonical and HTTP Link header canonical disagree. \
                                  Use a single, consistent canonical method."
                            .to_string(),
                        evidence: Evidence {
                            declared: Some(format!("HTTP Link: {http}")),
                            observed: Some(format!("HTML canonical: {html}")),
                            canonical: None,
                            detail: None,
                        },
                    });
                }
            }

            // 003: Canonical target does not resolve
            if let Some(ref canonical) = state.effective_canonical {
                if canonical != &state.url {
                    // Check if the canonical target exists in the snapshot
                    let target_key = normalize_url_key(canonical);
                    if let Some(target_state) = snapshot.urls.get(&target_key) {
                        match target_state.http_status {
                            Some(status) if status != 200 => {
                                findings.push(Finding {
                                    rule_id: "CC-CANONICAL-CONSISTENCY-003".to_string(),
                                    severity: Severity::Error,
                                    url: state.url.to_string(),
                                    message: format!(
                                        "The canonical target returned HTTP {status}. \
                                         Canonical targets must resolve successfully."
                                    ),
                                    evidence: Evidence {
                                        declared: None,
                                        observed: Some(format!("HTTP {status}")),
                                        canonical: Some(canonical.to_string()),
                                        detail: None,
                                    },
                                });
                            }
                            None => {
                                findings.push(Finding {
                                    rule_id: "CC-CANONICAL-CONSISTENCY-003".to_string(),
                                    severity: Severity::Error,
                                    url: state.url.to_string(),
                                    message: "The canonical target was discovered but not \
                                              fetched, so it could not be verified."
                                        .to_string(),
                                    evidence: Evidence {
                                        declared: None,
                                        observed: Some("no response recorded".to_string()),
                                        canonical: Some(canonical.to_string()),
                                        detail: None,
                                    },
                                });
                            }
                            Some(_) if !target_state.is_indexable => {
                                findings.push(Finding {
                                    rule_id: "CC-CANONICAL-CONSISTENCY-004".to_string(),
                                    severity: Severity::Error,
                                    url: state.url.to_string(),
                                    message: "The canonical target resolves successfully but is \
                                              not indexable."
                                        .to_string(),
                                    evidence: Evidence {
                                        declared: None,
                                        observed: Some("canonical target is non-indexable".into()),
                                        canonical: Some(canonical.to_string()),
                                        detail: None,
                                    },
                                });
                            }
                            Some(_) => {}
                        }
                    } else {
                        // Canonical target not found in scan — warn
                        findings.push(Finding {
                            rule_id: "CC-CANONICAL-CONSISTENCY-003".to_string(),
                            severity: Severity::Warning,
                            url: state.url.to_string(),
                            message: "The canonical target was not found in the scanned site. \
                                      It may be unreachable or external."
                                .to_string(),
                            evidence: Evidence {
                                declared: None,
                                observed: Some("not found in scan".to_string()),
                                canonical: Some(canonical.to_string()),
                                detail: None,
                            },
                        });
                    }
                }
            }
        }

        findings
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::snapshot::{Snapshot, Statistics};
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

    #[test]
    fn html_and_http_canonical_conflict() {
        let mut urls = BTreeMap::new();
        let url = Url::parse("https://example.test/page").unwrap();
        let mut state = UrlState::new(url.clone());
        state.html_canonical = Some(Url::parse("https://example.test/page-a").unwrap());
        state.http_canonical = Some(Url::parse("https://example.test/page-b").unwrap());
        state.is_indexable = true;
        urls.insert(url.to_string(), state);

        let snapshot = make_snapshot(urls);
        let findings = CanonicalConsistencyRule.evaluate(&snapshot);
        assert!(findings
            .iter()
            .any(|f| f.rule_id == "CC-CANONICAL-CONSISTENCY-001"));
    }

    #[test]
    fn canonical_target_not_in_scan() {
        let mut urls = BTreeMap::new();
        let url = Url::parse("https://example.test/page").unwrap();
        let mut state = UrlState::new(url.clone());
        state.effective_canonical = Some(Url::parse("https://example.test/other-page").unwrap());
        urls.insert(url.to_string(), state);

        let snapshot = make_snapshot(urls);
        let findings = CanonicalConsistencyRule.evaluate(&snapshot);
        assert!(findings
            .iter()
            .any(|f| f.rule_id == "CC-CANONICAL-CONSISTENCY-003"));
    }
}
