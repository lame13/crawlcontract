use std::collections::HashMap;

use crate::model::snapshot::Snapshot;
use crate::rules::registry::{Evidence, Finding, Rule, Severity};

/// CC-HREFLANG-RECIPROCAL-001: Hreflang entry not reciprocated by target page.
/// CC-HREFLANG-RECIPROCAL-002: Hreflang cluster missing self-reference.
pub struct HreflangReciprocalRule;

impl Rule for HreflangReciprocalRule {
    fn id(&self) -> &str {
        "CC-HREFLANG-RECIPROCAL"
    }

    fn evaluate(&self, snapshot: &Snapshot) -> Vec<Finding> {
        let mut findings = Vec::new();

        // Build a map: url -> list of (lang, url) hreflang entries
        let hreflang_map: HashMap<&str, Vec<(&str, &str)>> = snapshot
            .urls
            .values()
            .map(|state| {
                let entries: Vec<(&str, &str)> = state
                    .hreflang
                    .iter()
                    .map(|e| (e.lang.as_str(), e.url.as_str()))
                    .collect();
                (state.url.as_str(), entries)
            })
            .collect();

        for (url, entries) in &hreflang_map {
            let own_lang = entries.iter().find(|(_, target)| *target == *url);

            // 002: Missing self-reference
            if !entries.is_empty() && own_lang.is_none() {
                findings.push(Finding {
                    rule_id: "CC-HREFLANG-RECIPROCAL-002".to_string(),
                    severity: Severity::Error,
                    url: url.to_string(),
                    message: "This page declares hreflang entries for other pages but does \
                              not include a self-referencing hreflang entry."
                        .to_string(),
                    evidence: Evidence {
                        declared: None,
                        observed: Some(format!(
                            "hreflang entries: {}",
                            entries
                                .iter()
                                .map(|(lang, target)| format!("{lang} → {target}"))
                                .collect::<Vec<_>>()
                                .join(", ")
                        )),
                        canonical: None,
                        detail: None,
                    },
                });
            }

            // 001: Reciprocity check
            for (lang, target_url) in entries {
                if *target_url == *url {
                    continue; // Self-reference, skip
                }
                match hreflang_map.get(target_url) {
                    Some(target_entries) => {
                        let has_back = target_entries.iter().any(|(_, back_url)| *back_url == *url);
                        if !has_back {
                            findings.push(Finding {
                                rule_id: "CC-HREFLANG-RECIPROCAL-001".to_string(),
                                severity: Severity::Error,
                                url: url.to_string(),
                                message: format!(
                                    "This page declares hreflang {lang} → {target_url}, \
                                     but {target_url} does not link back with a reciprocal \
                                     hreflang entry."
                                ),
                                evidence: Evidence {
                                    declared: Some(format!("hreflang: {lang} → {target_url}")),
                                    observed: Some(format!(
                                        "target hreflang: [{}]",
                                        target_entries
                                            .iter()
                                            .map(|(l, u)| format!("{l} → {u}"))
                                            .collect::<Vec<_>>()
                                            .join(", ")
                                    )),
                                    canonical: None,
                                    detail: None,
                                },
                            });
                        }
                    }
                    None => {
                        findings.push(Finding {
                            rule_id: "CC-HREFLANG-RECIPROCAL-001".to_string(),
                            severity: Severity::Error,
                            url: url.to_string(),
                            message: format!(
                                "This page declares hreflang {lang} → {target_url}, \
                                 but {target_url} was not found in the scan and cannot \
                                 be verified for reciprocity."
                            ),
                            evidence: Evidence {
                                declared: Some(format!("hreflang: {lang} → {target_url}")),
                                observed: Some("target not found in scan".to_string()),
                                canonical: None,
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
    use crate::model::hreflang::HreflangEntry;
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
    fn reciprocal_hreflang_passes() {
        let mut urls = BTreeMap::new();

        let en_url = Url::parse("https://example.test/en/page").unwrap();
        let es_url = Url::parse("https://example.test/es/page").unwrap();

        let mut en_state = UrlState::new(en_url.clone());
        en_state.hreflang = vec![
            HreflangEntry {
                lang: "en".to_string(),
                url: en_url.clone(),
            },
            HreflangEntry {
                lang: "es".to_string(),
                url: es_url.clone(),
            },
        ];

        let mut es_state = UrlState::new(es_url.clone());
        es_state.hreflang = vec![
            HreflangEntry {
                lang: "en".to_string(),
                url: en_url.clone(),
            },
            HreflangEntry {
                lang: "es".to_string(),
                url: es_url.clone(),
            },
        ];

        urls.insert(en_url.to_string(), en_state);
        urls.insert(es_url.to_string(), es_state);

        let snapshot = make_snapshot(urls);
        let findings = HreflangReciprocalRule.evaluate(&snapshot);
        assert!(findings.is_empty());
    }

    #[test]
    fn missing_reciprocity_detected() {
        let mut urls = BTreeMap::new();

        let en_url = Url::parse("https://example.test/en/page").unwrap();
        let es_url = Url::parse("https://example.test/es/page").unwrap();

        let mut en_state = UrlState::new(en_url.clone());
        en_state.hreflang = vec![
            HreflangEntry {
                lang: "en".to_string(),
                url: en_url.clone(),
            },
            HreflangEntry {
                lang: "es".to_string(),
                url: es_url.clone(),
            },
        ];

        // es page has no hreflang at all
        let es_state = UrlState::new(es_url.clone());

        urls.insert(en_url.to_string(), en_state);
        urls.insert(es_url.to_string(), es_state);

        let snapshot = make_snapshot(urls);
        let findings = HreflangReciprocalRule.evaluate(&snapshot);
        assert!(findings
            .iter()
            .any(|f| f.rule_id == "CC-HREFLANG-RECIPROCAL-001"));
    }

    #[test]
    fn missing_self_reference_detected() {
        let mut urls = BTreeMap::new();

        let en_url = Url::parse("https://example.test/en/page").unwrap();
        let es_url = Url::parse("https://example.test/es/page").unwrap();

        let mut en_state = UrlState::new(en_url.clone());
        // Only references es, missing self-reference
        en_state.hreflang = vec![HreflangEntry {
            lang: "es".to_string(),
            url: es_url.clone(),
        }];

        urls.insert(en_url.to_string(), en_state);

        let snapshot = make_snapshot(urls);
        let findings = HreflangReciprocalRule.evaluate(&snapshot);
        assert!(findings
            .iter()
            .any(|f| f.rule_id == "CC-HREFLANG-RECIPROCAL-002"));
    }
}
