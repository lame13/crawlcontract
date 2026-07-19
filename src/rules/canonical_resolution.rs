use std::collections::{HashMap, HashSet};

use crate::model::snapshot::Snapshot;
use crate::rules::registry::{Evidence, Finding, Rule, Severity};
use crate::scanner::html_signals::normalize_url_key;

/// CC-CANONICAL-RESOLUTION-001: Canonical chain exceeds depth 1.
/// CC-CANONICAL-RESOLUTION-002: Canonical cycle detected.
pub struct CanonicalResolutionRule;

impl Rule for CanonicalResolutionRule {
    fn id(&self) -> &str {
        "CC-CANONICAL-RESOLUTION"
    }

    fn evaluate(&self, snapshot: &Snapshot) -> Vec<Finding> {
        let mut findings = Vec::new();

        // Build canonical graph: url -> canonical target
        let canonical_graph: HashMap<String, Option<String>> = snapshot
            .urls
            .iter()
            .map(|(key, state)| {
                (
                    key.clone(),
                    state.effective_canonical.as_ref().map(normalize_url_key),
                )
            })
            .collect();

        for url in snapshot.urls.keys() {
            let Some(target) = canonical_graph.get(url) else {
                continue;
            };
            let Some(target) = target else {
                continue;
            };
            if target == url {
                continue;
            }

            // Walk the canonical chain
            let mut chain = vec![url.clone()];
            let mut current = target.clone();
            let mut visited: HashSet<String> = HashSet::new();
            visited.insert(url.clone());

            loop {
                if visited.contains(&current) {
                    // Cycle detected
                    chain.push(current.clone());
                    findings.push(Finding {
                        rule_id: "CC-CANONICAL-RESOLUTION-002".to_string(),
                        severity: Severity::Error,
                        url: url.clone(),
                        message: format!("Canonical cycle detected: {}", chain.join(" → ")),
                        evidence: Evidence {
                            declared: None,
                            observed: None,
                            canonical: Some(chain.join(" → ")),
                            detail: None,
                        },
                    });
                    break;
                }

                chain.push(current.clone());
                visited.insert(current.clone());

                // Check if this target has its own canonical
                match canonical_graph.get(&current) {
                    Some(Some(next)) if next != &current => {
                        if visited.contains(next) {
                            current = next.clone();
                            continue;
                        }
                        if chain.len() >= 2 {
                            let mut reported_chain = chain.clone();
                            reported_chain.push(next.clone());
                            findings.push(Finding {
                                rule_id: "CC-CANONICAL-RESOLUTION-001".to_string(),
                                severity: Severity::Error,
                                url: url.clone(),
                                message: format!(
                                    "Canonical chain exceeds depth 1: {}",
                                    reported_chain.join(" → ")
                                ),
                                evidence: Evidence {
                                    declared: None,
                                    observed: None,
                                    canonical: Some(reported_chain.join(" → ")),
                                    detail: Some(format!(
                                        "Chain length: {}",
                                        reported_chain.len() - 1
                                    )),
                                },
                            });
                            break;
                        }
                        current = next.clone();
                    }
                    _ => break,
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
    fn canonical_cycle_detected() {
        let mut urls = BTreeMap::new();

        let a = Url::parse("https://example.test/a").unwrap();
        let b = Url::parse("https://example.test/b").unwrap();

        let mut state_a = UrlState::new(a.clone());
        state_a.effective_canonical = Some(b.clone());

        let mut state_b = UrlState::new(b.clone());
        state_b.effective_canonical = Some(a.clone());

        urls.insert(a.to_string(), state_a);
        urls.insert(b.to_string(), state_b);

        let snapshot = make_snapshot(urls);
        let findings = CanonicalResolutionRule.evaluate(&snapshot);
        assert!(findings
            .iter()
            .any(|f| f.rule_id == "CC-CANONICAL-RESOLUTION-002"));
    }

    #[test]
    fn canonical_chain_too_long() {
        let mut urls = BTreeMap::new();

        let a = Url::parse("https://example.test/a").unwrap();
        let b = Url::parse("https://example.test/b").unwrap();
        let c = Url::parse("https://example.test/c").unwrap();
        let d = Url::parse("https://example.test/d").unwrap();

        let mut state_a = UrlState::new(a.clone());
        state_a.effective_canonical = Some(b.clone());

        let mut state_b = UrlState::new(b.clone());
        state_b.effective_canonical = Some(c.clone());

        let mut state_c = UrlState::new(c.clone());
        state_c.effective_canonical = Some(d.clone());

        let state_d = UrlState::new(d.clone());

        urls.insert(a.to_string(), state_a);
        urls.insert(b.to_string(), state_b);
        urls.insert(c.to_string(), state_c);
        urls.insert(d.to_string(), state_d);

        let snapshot = make_snapshot(urls);
        let findings = CanonicalResolutionRule.evaluate(&snapshot);
        assert!(findings
            .iter()
            .any(|f| f.rule_id == "CC-CANONICAL-RESOLUTION-001"));
    }

    #[test]
    fn self_canonical_passes() {
        let mut urls = BTreeMap::new();
        let url = Url::parse("https://example.test/page").unwrap();
        let mut state = UrlState::new(url.clone());
        state.effective_canonical = Some(url.clone());
        urls.insert(url.to_string(), state);

        let snapshot = make_snapshot(urls);
        let findings = CanonicalResolutionRule.evaluate(&snapshot);
        assert!(findings.is_empty());
    }
}
