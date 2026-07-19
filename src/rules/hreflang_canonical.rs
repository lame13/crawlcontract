use crate::model::snapshot::Snapshot;
use crate::rules::registry::{Evidence, Finding, Rule, Severity};
use crate::scanner::html_signals::normalize_url_key;

/// CC-HREFLANG-CANONICAL-001: Hreflang URL differs from target's own canonical.
pub struct HreflangCanonicalRule;

impl Rule for HreflangCanonicalRule {
    fn id(&self) -> &str {
        "CC-HREFLANG-CANONICAL"
    }

    fn evaluate(&self, snapshot: &Snapshot) -> Vec<Finding> {
        let mut findings = Vec::new();

        for state in snapshot.urls.values() {
            if state.hreflang.is_empty() {
                continue;
            }

            for entry in &state.hreflang {
                // Check if the hreflang target's canonical matches the hreflang URL
                let target_key = normalize_url_key(&entry.url);
                if let Some(target_state) = snapshot.urls.get(&target_key) {
                    if let Some(ref target_canonical) = target_state.effective_canonical {
                        if target_canonical != &entry.url {
                            findings.push(Finding {
                                rule_id: "CC-HREFLANG-CANONICAL-001".to_string(),
                                severity: Severity::Error,
                                url: state.url.to_string(),
                                message: format!(
                                    "This page declares hreflang {} → {}, but {} canonicalizes \
                                     to {}. Hreflang URLs should point to the canonical version.",
                                    entry.lang, entry.url, entry.url, target_canonical
                                ),
                                evidence: Evidence {
                                    declared: Some(format!(
                                        "hreflang: {} → {}",
                                        entry.lang, entry.url
                                    )),
                                    observed: Some(format!(
                                        "target canonical: {}",
                                        target_canonical
                                    )),
                                    canonical: Some(target_canonical.to_string()),
                                    detail: None,
                                },
                            });
                        }
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
    fn hreflang_target_canonical_mismatch() {
        let mut urls = BTreeMap::new();

        let en_url = Url::parse("https://example.test/en/page").unwrap();
        let es_url = Url::parse("https://example.test/es/page").unwrap();
        let es_canonical = Url::parse("https://example.test/es/different-page").unwrap();

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
        es_state.effective_canonical = Some(es_canonical);

        urls.insert(en_url.to_string(), en_state);
        urls.insert(es_url.to_string(), es_state);

        let snapshot = make_snapshot(urls);
        let findings = HreflangCanonicalRule.evaluate(&snapshot);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule_id, "CC-HREFLANG-CANONICAL-001");
    }

    #[test]
    fn hreflang_target_is_canonical_passes() {
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
        es_state.effective_canonical = Some(es_url.clone());

        urls.insert(en_url.to_string(), en_state);
        urls.insert(es_url.to_string(), es_state);

        let snapshot = make_snapshot(urls);
        let findings = HreflangCanonicalRule.evaluate(&snapshot);
        assert!(findings.is_empty());
    }

    #[test]
    fn self_hreflang_must_match_the_pages_canonical() {
        let mut urls = BTreeMap::new();
        let page = Url::parse("https://example.test/en/page").unwrap();
        let canonical = Url::parse("https://example.test/en/other").unwrap();
        let mut state = UrlState::new(page.clone());
        state.hreflang = vec![HreflangEntry {
            lang: "en".to_string(),
            url: page.clone(),
        }];
        state.effective_canonical = Some(canonical);
        urls.insert(page.to_string(), state);

        let findings = HreflangCanonicalRule.evaluate(&make_snapshot(urls));
        assert_eq!(findings.len(), 1);
    }
}
