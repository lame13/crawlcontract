use crate::model::snapshot::Snapshot;
use crate::rules::registry::{Evidence, Finding, Rule, Severity};

/// CC-SITEMAP-COVERAGE-001: An indexable, verified page is missing from every sitemap.
///
/// This is the inverse of [`crate::rules::sitemap_indexability`]: that rule
/// proves every sitemap entry is indexable, this one proves every indexable
/// page is declared. A site with no sitemap data in the snapshot is out of
/// scope, because there is nothing to compare against.
pub struct SitemapCoverageRule;

impl Rule for SitemapCoverageRule {
    fn id(&self) -> &str {
        "CC-SITEMAP-COVERAGE"
    }

    fn evaluate(&self, snapshot: &Snapshot) -> Vec<Finding> {
        let mut findings = Vec::new();

        if snapshot.statistics.sitemap_urls == 0 {
            return findings;
        }

        for state in snapshot.urls.values() {
            if state.found_in_sitemap || !state.is_indexable {
                continue;
            }

            // Only HTML documents belong in a sitemap. A 200 response that was
            // never parsed as HTML (for example `/sitemap.xml` itself, or a
            // linked PDF) has no word count and is not a page.
            if state.word_count.is_none() {
                continue;
            }

            let sources = state
                .sources
                .iter()
                .map(|source| source.to_string())
                .collect::<Vec<_>>()
                .join(", ");

            findings.push(Finding {
                rule_id: "CC-SITEMAP-COVERAGE-001".to_string(),
                severity: Severity::Warning,
                url: state.url.to_string(),
                message: "This URL is indexable and returns HTTP 200, but no sitemap declares \
                          it. Search engines can still find it, but the sitemap no longer \
                          describes the site."
                    .to_string(),
                evidence: Evidence {
                    declared: Some("no sitemap entry".to_string()),
                    observed: Some("indexable HTML page".to_string()),
                    canonical: state
                        .effective_canonical
                        .as_ref()
                        .map(|url| url.to_string()),
                    detail: Some(format!("discovered via: {sources}")),
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

    fn indexable_page(url: &str, in_sitemap: bool) -> (String, UrlState) {
        let parsed = Url::parse(url).unwrap();
        let mut state = UrlState::new(parsed.clone());
        state.http_status = Some(200);
        state.word_count = Some(120);
        state.is_indexable = true;
        state.is_reachable = true;
        state.effective_canonical = Some(parsed.clone());
        state.found_in_sitemap = in_sitemap;
        state.add_source(if in_sitemap {
            UrlSource::Sitemap
        } else {
            UrlSource::InternalLink
        });
        (parsed.to_string(), state)
    }

    #[test]
    fn indexable_page_missing_from_sitemap_is_reported() {
        let urls = BTreeMap::from([
            indexable_page("https://example.test/listed", true),
            indexable_page("https://example.test/unlisted", false),
        ]);

        let findings = SitemapCoverageRule.evaluate(&make_snapshot(urls));

        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule_id, "CC-SITEMAP-COVERAGE-001");
        assert!(findings[0].url.ends_with("/unlisted"));
    }

    #[test]
    fn site_without_any_sitemap_is_out_of_scope() {
        let urls = BTreeMap::from([indexable_page("https://example.test/unlisted", false)]);

        let findings = SitemapCoverageRule.evaluate(&make_snapshot(urls));

        assert!(findings.is_empty());
    }

    #[test]
    fn non_html_responses_are_not_flagged() {
        let mut urls = BTreeMap::from([indexable_page("https://example.test/listed", true)]);
        // /sitemap.xml returns 200 but is never parsed as an HTML document.
        let sitemap_url = Url::parse("https://example.test/sitemap.xml").unwrap();
        let mut sitemap_state = UrlState::new(sitemap_url.clone());
        sitemap_state.http_status = Some(200);
        sitemap_state.is_indexable = true;
        urls.insert(sitemap_url.to_string(), sitemap_state);

        let findings = SitemapCoverageRule.evaluate(&make_snapshot(urls));

        assert!(findings.is_empty(), "unexpected findings: {findings:?}");
    }

    #[test]
    fn non_indexable_page_is_not_flagged() {
        let mut urls = BTreeMap::from([indexable_page("https://example.test/listed", true)]);
        let url = Url::parse("https://example.test/noindexed").unwrap();
        let mut state = UrlState::new(url.clone());
        state.http_status = Some(200);
        state.word_count = Some(50);
        state.is_indexable = false;
        urls.insert(url.to_string(), state);

        let findings = SitemapCoverageRule.evaluate(&make_snapshot(urls));

        assert!(findings.is_empty());
    }
}
