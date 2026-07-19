use crate::model::snapshot::Snapshot;
use crate::rules::registry::{Evidence, Finding, Rule, Severity};
use crate::scanner::html_signals::normalize_url_key;

/// CC-LINK-TARGET-001: Internal link resolves to a non-200 without a redirect target.
/// CC-LINK-TARGET-002: Internal link chains through redirect instead of targeting canonical.
/// CC-LINK-TARGET-003: Internal link target could not be verified.
/// CC-LINK-TARGET-004: Internal link target canonicalizes elsewhere.
pub struct InternalLinkTargetRule;

impl Rule for InternalLinkTargetRule {
    fn id(&self) -> &str {
        "CC-LINK-TARGET"
    }

    fn evaluate(&self, snapshot: &Snapshot) -> Vec<Finding> {
        let mut findings = Vec::new();

        for state in snapshot.urls.values() {
            for link_url in &state.internal_links_out {
                let link_key = normalize_url_key(link_url);

                match snapshot.urls.get(&link_key) {
                    Some(target_state) => {
                        // 001: Broken link
                        if let Some(status) = target_state.http_status {
                            if status != 200 && target_state.redirect_target.is_none() {
                                findings.push(Finding {
                                    rule_id: "CC-LINK-TARGET-001".to_string(),
                                    severity: Severity::Error,
                                    url: state.url.to_string(),
                                    message: format!(
                                        "Internal link target {link_url} returned HTTP {status} \
                                         without a usable redirect target."
                                    ),
                                    evidence: Evidence {
                                        declared: None,
                                        observed: Some(format!("HTTP {status}")),
                                        canonical: None,
                                        detail: Some(format!("link target: {link_url}")),
                                    },
                                });
                            }
                        } else {
                            findings.push(Finding {
                                rule_id: "CC-LINK-TARGET-003".to_string(),
                                severity: Severity::Error,
                                url: state.url.to_string(),
                                message: format!(
                                    "Internal link target {link_url} was not fetched, so its \
                                     resolution could not be verified."
                                ),
                                evidence: Evidence {
                                    declared: Some(link_url.to_string()),
                                    observed: Some("no response recorded".to_string()),
                                    canonical: None,
                                    detail: None,
                                },
                            });
                        }

                        // 002: Link goes through redirect
                        if let Some(ref redirect_target) = target_state.redirect_target {
                            findings.push(Finding {
                                rule_id: "CC-LINK-TARGET-002".to_string(),
                                severity: Severity::Warning,
                                url: state.url.to_string(),
                                message: format!(
                                    "Internal link targets {link_url} which redirects to \
                                     {redirect_target}. Link directly to the canonical URL."
                                ),
                                evidence: Evidence {
                                    declared: Some(link_url.to_string()),
                                    observed: Some(format!("redirect → {redirect_target}")),
                                    canonical: target_state
                                        .effective_canonical
                                        .as_ref()
                                        .map(|u| u.to_string()),
                                    detail: None,
                                },
                            });
                        }

                        if let Some(canonical) = &target_state.effective_canonical {
                            if canonical != link_url {
                                findings.push(Finding {
                                    rule_id: "CC-LINK-TARGET-004".to_string(),
                                    severity: Severity::Warning,
                                    url: state.url.to_string(),
                                    message: format!(
                                        "Internal link targets {link_url}, but that page \
                                         canonicalizes to {canonical}."
                                    ),
                                    evidence: Evidence {
                                        declared: Some(link_url.to_string()),
                                        observed: Some(
                                            "link target canonicalizes elsewhere".into(),
                                        ),
                                        canonical: Some(canonical.to_string()),
                                        detail: None,
                                    },
                                });
                            }
                        }
                    }
                    None => {
                        // URL not in scan — only flag if it's clearly broken
                        // In static mode we can't verify external targets
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
    fn broken_link_detected() {
        let mut urls = BTreeMap::new();

        let page = Url::parse("https://example.test/page").unwrap();
        let broken = Url::parse("https://example.test/missing").unwrap();

        let mut page_state = UrlState::new(page.clone());
        page_state.internal_links_out = vec![broken.clone()];

        let mut broken_state = UrlState::new(broken.clone());
        broken_state.http_status = Some(404);

        urls.insert(page.to_string(), page_state);
        urls.insert(broken.to_string(), broken_state);

        let snapshot = make_snapshot(urls);
        let findings = InternalLinkTargetRule.evaluate(&snapshot);
        assert!(findings.iter().any(|f| f.rule_id == "CC-LINK-TARGET-001"));
    }

    #[test]
    fn redirect_link_detected() {
        let mut urls = BTreeMap::new();

        let page = Url::parse("https://example.test/page").unwrap();
        let redirect = Url::parse("https://example.test/old-url").unwrap();
        let target = Url::parse("https://example.test/new-url").unwrap();

        let mut page_state = UrlState::new(page.clone());
        page_state.internal_links_out = vec![redirect.clone()];

        let mut redirect_state = UrlState::new(redirect.clone());
        redirect_state.redirect_target = Some(target.clone());

        urls.insert(page.to_string(), page_state);
        urls.insert(redirect.to_string(), redirect_state);

        let snapshot = make_snapshot(urls);
        let findings = InternalLinkTargetRule.evaluate(&snapshot);
        assert!(findings.iter().any(|f| f.rule_id == "CC-LINK-TARGET-002"));
    }

    #[test]
    fn unverified_link_detected() {
        let mut urls = BTreeMap::new();
        let page = Url::parse("https://example.test/page").unwrap();
        let target = Url::parse("https://example.test/unverified").unwrap();
        let mut page_state = UrlState::new(page.clone());
        page_state.internal_links_out = vec![target.clone()];
        urls.insert(page.to_string(), page_state);
        urls.insert(target.to_string(), UrlState::new(target));

        let findings = InternalLinkTargetRule.evaluate(&make_snapshot(urls));
        assert!(findings
            .iter()
            .any(|finding| finding.rule_id == "CC-LINK-TARGET-003"));
    }

    #[test]
    fn noncanonical_link_target_detected() {
        let mut urls = BTreeMap::new();
        let page = Url::parse("https://example.test/page").unwrap();
        let target = Url::parse("https://example.test/duplicate").unwrap();
        let canonical = Url::parse("https://example.test/canonical").unwrap();
        let mut page_state = UrlState::new(page.clone());
        page_state.internal_links_out = vec![target.clone()];
        let mut target_state = UrlState::new(target.clone());
        target_state.http_status = Some(200);
        target_state.effective_canonical = Some(canonical);
        urls.insert(page.to_string(), page_state);
        urls.insert(target.to_string(), target_state);

        let findings = InternalLinkTargetRule.evaluate(&make_snapshot(urls));
        assert!(findings
            .iter()
            .any(|finding| finding.rule_id == "CC-LINK-TARGET-004"));
    }
}
