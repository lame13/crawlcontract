use crate::model::snapshot::Snapshot;
use crate::rules::registry::{Evidence, Finding, Rule, Severity};

/// CC-ORPHAN-001: A verified indexable page is unreachable via internal links.
pub struct OrphanRule;

impl Rule for OrphanRule {
    fn id(&self) -> &str {
        "CC-ORPHAN"
    }

    fn evaluate(&self, snapshot: &Snapshot) -> Vec<Finding> {
        let mut findings = Vec::new();

        for state in snapshot.urls.values() {
            if state.is_indexable && state.http_status == Some(200) && !state.is_reachable {
                let declared = if state.found_in_sitemap {
                    "sitemap.xml"
                } else {
                    "scanned page"
                };
                findings.push(Finding {
                    rule_id: "CC-ORPHAN-001".to_string(),
                    severity: Severity::Warning,
                    url: state.url.to_string(),
                    message: "This URL is indexable, but it is not reachable via internal \
                              links from the scan entry point."
                        .to_string(),
                    evidence: Evidence {
                        declared: Some(declared.to_string()),
                        observed: Some("not reachable via internal links".to_string()),
                        canonical: state.effective_canonical.as_ref().map(|u| u.to_string()),
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
    use crate::model::snapshot::{Snapshot, Statistics};
    use crate::model::url_state::{UrlSource, UrlState};
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
    fn orphan_detected() {
        let mut urls = BTreeMap::new();

        let url = Url::parse("https://example.test/hidden-page").unwrap();
        let mut state = UrlState::new(url.clone());
        state.found_in_sitemap = true;
        state.add_source(UrlSource::Sitemap);
        state.http_status = Some(200);
        state.is_indexable = true;
        state.is_reachable = false; // Not reachable
        urls.insert(url.to_string(), state);

        let snapshot = make_snapshot(urls);
        let findings = OrphanRule.evaluate(&snapshot);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule_id, "CC-ORPHAN-001");
    }

    #[test]
    fn reachable_not_orphan() {
        let mut urls = BTreeMap::new();

        let url = Url::parse("https://example.test/linked-page").unwrap();
        let mut state = UrlState::new(url.clone());
        state.found_in_sitemap = true;
        state.add_source(UrlSource::Sitemap);
        state.http_status = Some(200);
        state.is_indexable = true;
        state.is_reachable = true;
        urls.insert(url.to_string(), state);

        let snapshot = make_snapshot(urls);
        let findings = OrphanRule.evaluate(&snapshot);
        assert!(findings.is_empty());
    }

    #[test]
    fn non_indexable_not_flagged() {
        let mut urls = BTreeMap::new();

        let url = Url::parse("https://example.test/admin").unwrap();
        let mut state = UrlState::new(url.clone());
        state.found_in_sitemap = true;
        state.add_source(UrlSource::Sitemap);
        state.is_indexable = false;
        state.is_reachable = false;
        urls.insert(url.to_string(), state);

        let snapshot = make_snapshot(urls);
        let findings = OrphanRule.evaluate(&snapshot);
        assert!(findings.is_empty());
    }

    #[test]
    fn indexable_page_outside_sitemap_is_still_an_orphan() {
        let mut urls = BTreeMap::new();
        let url = Url::parse("https://example.test/unlinked").unwrap();
        let mut state = UrlState::new(url.clone());
        state.http_status = Some(200);
        state.is_indexable = true;
        urls.insert(url.to_string(), state);

        let findings = OrphanRule.evaluate(&make_snapshot(urls));
        assert_eq!(findings.len(), 1);
    }
}
