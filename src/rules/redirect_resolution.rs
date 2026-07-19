use crate::model::snapshot::Snapshot;
use crate::rules::registry::{Evidence, Finding, Rule, Severity};
use crate::signals::redirect::has_redirect_cycle;

/// CC-REDIRECT-RESOLUTION-001: Redirect cycle detected.
pub struct RedirectResolutionRule;

impl Rule for RedirectResolutionRule {
    fn id(&self) -> &str {
        "CC-REDIRECT-RESOLUTION"
    }

    fn evaluate(&self, snapshot: &Snapshot) -> Vec<Finding> {
        snapshot
            .urls
            .values()
            .filter(|state| has_redirect_cycle(&state.redirect_chain))
            .map(|state| {
                let chain = state
                    .redirect_chain
                    .iter()
                    .map(|url| url.as_str())
                    .collect::<Vec<_>>()
                    .join(" → ");
                Finding {
                    rule_id: "CC-REDIRECT-RESOLUTION-001".to_string(),
                    severity: Severity::Error,
                    url: state.url.to_string(),
                    message: format!("Redirect cycle detected: {chain}"),
                    evidence: Evidence {
                        declared: None,
                        observed: Some(chain),
                        canonical: None,
                        detail: None,
                    },
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::snapshot::{utc_now, Statistics};
    use crate::model::url_state::UrlState;
    use std::collections::BTreeMap;
    use url::Url;

    #[test]
    fn reports_redirect_cycle() {
        let a = Url::parse("https://example.test/a").unwrap();
        let b = Url::parse("https://example.test/b").unwrap();
        let mut state = UrlState::new(a.clone());
        state.redirect_chain = vec![a.clone(), b, a.clone()];
        let urls = BTreeMap::from([(a.to_string(), state)]);
        let snapshot = Snapshot {
            version: "1.0".into(),
            tool: "crawlcontract".into(),
            base_url: Url::parse("https://example.test/").unwrap(),
            public_origin: None,
            scanned_at: utc_now(),
            statistics: Statistics::from_url_states(&urls),
            urls,
        };

        let findings = RedirectResolutionRule.evaluate(&snapshot);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule_id, "CC-REDIRECT-RESOLUTION-001");
    }
}
