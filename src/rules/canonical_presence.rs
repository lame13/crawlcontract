use crate::model::snapshot::Snapshot;
use crate::rules::registry::{Evidence, Finding, Rule, Severity};

/// CC-CANONICAL-PRESENCE-001: An indexable page declares no canonical at all.
///
/// The rest of the canonical rule family only compares canonicals that exist.
/// A template that emits no `rel="canonical"` and no `Link: rel="canonical"`
/// header is invisible to those rules while still being indexable.
pub struct CanonicalPresenceRule;

impl Rule for CanonicalPresenceRule {
    fn id(&self) -> &str {
        "CC-CANONICAL-PRESENCE"
    }

    fn evaluate(&self, snapshot: &Snapshot) -> Vec<Finding> {
        let mut findings = Vec::new();

        for state in snapshot.urls.values() {
            // `is_indexable` already requires a verified HTTP 200, no
            // restrictive directive, and a canonical that does not point
            // elsewhere.
            if !state.is_indexable {
                continue;
            }
            if state.html_canonical.is_some() || state.http_canonical.is_some() {
                continue;
            }
            // Only HTML documents are expected to carry a canonical. A 200
            // response that was never parsed as HTML - an image, a stylesheet,
            // a PDF - has no word count, and its absence of a canonical is not
            // a defect.
            if state.word_count.is_none() {
                continue;
            }

            findings.push(Finding {
                rule_id: "CC-CANONICAL-PRESENCE-001".to_string(),
                severity: Severity::Warning,
                url: state.url.to_string(),
                message: "This page is indexable but declares no canonical URL, so duplicate \
                          variants of the same content can compete with it."
                    .to_string(),
                evidence: Evidence {
                    declared: Some("no canonical".to_string()),
                    observed: Some(
                        "no rel=\"canonical\" link and no Link canonical header".to_string(),
                    ),
                    canonical: None,
                    detail: Some("indexable page without a canonical".to_string()),
                },
            });
        }

        findings
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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

    #[test]
    fn indexable_page_without_canonical_is_reported() {
        let url = Url::parse("https://example.test/page").unwrap();
        let mut state = UrlState::new(url.clone());
        state.http_status = Some(200);
        state.is_indexable = true;
        state.word_count = Some(120);

        let findings = CanonicalPresenceRule
            .evaluate(&make_snapshot(BTreeMap::from([(url.to_string(), state)])));

        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule_id, "CC-CANONICAL-PRESENCE-001");
    }

    #[test]
    fn html_canonical_satisfies_the_contract() {
        let url = Url::parse("https://example.test/page").unwrap();
        let mut state = UrlState::new(url.clone());
        state.http_status = Some(200);
        state.html_canonical = Some(url.clone());
        state.is_indexable = true;
        state.word_count = Some(120);

        let findings = CanonicalPresenceRule
            .evaluate(&make_snapshot(BTreeMap::from([(url.to_string(), state)])));

        assert!(findings.is_empty());
    }

    #[test]
    fn http_link_canonical_satisfies_the_contract() {
        let url = Url::parse("https://example.test/page").unwrap();
        let mut state = UrlState::new(url.clone());
        state.http_status = Some(200);
        state.http_canonical = Some(url.clone());
        state.is_indexable = true;
        state.word_count = Some(120);

        let findings = CanonicalPresenceRule
            .evaluate(&make_snapshot(BTreeMap::from([(url.to_string(), state)])));

        assert!(findings.is_empty());
    }

    #[test]
    fn non_indexable_page_without_canonical_is_not_reported() {
        let url = Url::parse("https://example.test/private").unwrap();
        let mut state = UrlState::new(url.clone());
        state.http_status = Some(200);
        state.is_indexable = false;

        let findings = CanonicalPresenceRule
            .evaluate(&make_snapshot(BTreeMap::from([(url.to_string(), state)])));

        assert!(findings.is_empty());
    }

    #[test]
    fn non_html_responses_are_not_reported() {
        let url = Url::parse("https://example.test/media/hero.webp").unwrap();
        let mut state = UrlState::new(url.clone());
        state.http_status = Some(200);
        // A 200 response that is not parsed as HTML has no word count.
        state.is_indexable = true;
        state.word_count = None;

        let findings = CanonicalPresenceRule
            .evaluate(&make_snapshot(BTreeMap::from([(url.to_string(), state)])));

        assert!(findings.is_empty(), "unexpected findings: {findings:?}");
    }
}
