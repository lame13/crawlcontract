use crate::model::snapshot::Snapshot;
use crate::rules::registry::{Evidence, Finding, Rule, Severity};

/// CC-ROBOTS-EFFECTIVE-001: Meta robots and X-Robots-Tag conflict on the same directive.
pub struct RobotsEffectiveRule;

impl Rule for RobotsEffectiveRule {
    fn id(&self) -> &str {
        "CC-ROBOTS-EFFECTIVE"
    }

    fn evaluate(&self, snapshot: &Snapshot) -> Vec<Finding> {
        let mut findings = Vec::new();

        for state in snapshot.urls.values() {
            let meta = match &state.html_meta_robots {
                Some(m) => m,
                None => continue,
            };
            let http = match &state.http_x_robots_tag {
                Some(h) => h,
                None => continue,
            };

            // Check for conflicts on key directives
            check_conflict(
                &mut findings,
                state.url.as_str(),
                "noindex",
                meta.noindex,
                http.noindex,
            );
            check_conflict(
                &mut findings,
                state.url.as_str(),
                "nofollow",
                meta.nofollow,
                http.nofollow,
            );
        }

        findings
    }
}

fn check_conflict(
    findings: &mut Vec<Finding>,
    url: &str,
    directive: &str,
    meta_val: Option<bool>,
    http_val: Option<bool>,
) {
    if let (Some(m), Some(h)) = (meta_val, http_val) {
        if m != h {
            findings.push(Finding {
                rule_id: "CC-ROBOTS-EFFECTIVE-001".to_string(),
                severity: Severity::Warning,
                url: url.to_string(),
                message: format!(
                    "Meta robots and X-Robots-Tag disagree on {directive}. \
                     Meta robots: {m}, X-Robots-Tag: {h}. \
                     Resolve the conflict to ensure predictable indexing behavior."
                ),
                evidence: Evidence {
                    declared: Some(format!("meta robots: {directive}={m}")),
                    observed: Some(format!("X-Robots-Tag: {directive}={h}")),
                    canonical: None,
                    detail: None,
                },
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::robots::RobotsDirective;
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
    fn conflicting_noindex_detected() {
        let mut urls = BTreeMap::new();
        let url = Url::parse("https://example.test/page").unwrap();
        let mut state = UrlState::new(url.clone());
        state.html_meta_robots = Some(RobotsDirective {
            noindex: Some(true),
            ..Default::default()
        });
        state.http_x_robots_tag = Some(RobotsDirective {
            noindex: Some(false),
            ..Default::default()
        });
        urls.insert(url.to_string(), state);

        let snapshot = make_snapshot(urls);
        let findings = RobotsEffectiveRule.evaluate(&snapshot);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule_id, "CC-ROBOTS-EFFECTIVE-001");
    }

    #[test]
    fn no_conflict_passes() {
        let mut urls = BTreeMap::new();
        let url = Url::parse("https://example.test/page").unwrap();
        let mut state = UrlState::new(url.clone());
        state.html_meta_robots = Some(RobotsDirective {
            noindex: Some(true),
            ..Default::default()
        });
        state.http_x_robots_tag = Some(RobotsDirective {
            noindex: Some(true),
            ..Default::default()
        });
        urls.insert(url.to_string(), state);

        let snapshot = make_snapshot(urls);
        let findings = RobotsEffectiveRule.evaluate(&snapshot);
        assert!(findings.is_empty());
    }
}
