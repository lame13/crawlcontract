use crate::config::policy::DiffPolicy;
use crate::model::snapshot::Snapshot;
use crate::rules::registry::{Evidence, Finding, Rule, Severity};

/// CC-DIFF-LOSS-INDEXABLE: Indexable URL count decreased beyond threshold.
/// CC-DIFF-LOSS-CONTENT: Page lost significant word count or heading structure.
/// CC-DIFF-LOSS-LINKS: Internal link count decreased beyond threshold.
pub struct DiffLossRule {
    pub baseline: Snapshot,
    pub policy: DiffPolicy,
}

impl Rule for DiffLossRule {
    fn id(&self) -> &str {
        "CC-DIFF-LOSS"
    }

    fn evaluate(&self, snapshot: &Snapshot) -> Vec<Finding> {
        let mut findings = Vec::new();
        let baseline = &self.baseline;

        // --- Indexable URL loss ---
        let baseline_indexable = baseline.statistics.indexable_urls as f64;
        let candidate_indexable = snapshot.statistics.indexable_urls as f64;
        if baseline_indexable > 0.0 {
            let loss_pct =
                ((baseline_indexable - candidate_indexable) / baseline_indexable) * 100.0;
            if loss_pct > self.policy.max_indexable_url_loss_percent {
                findings.push(Finding {
                    rule_id: "CC-DIFF-LOSS-INDEXABLE".to_string(),
                    severity: Severity::Error,
                    url: "(site-wide)".to_string(),
                    message: format!(
                        "Indexable URL count decreased by {loss_pct:.1}% \
                         (baseline: {}, candidate: {}, threshold: {}%). \
                         This may indicate a deployment error.",
                        baseline.statistics.indexable_urls,
                        snapshot.statistics.indexable_urls,
                        self.policy.max_indexable_url_loss_percent
                    ),
                    evidence: Evidence {
                        declared: Some(format!(
                            "baseline indexable: {}",
                            baseline.statistics.indexable_urls
                        )),
                        observed: Some(format!(
                            "candidate indexable: {}",
                            snapshot.statistics.indexable_urls
                        )),
                        canonical: None,
                        detail: Some(format!("{loss_pct:.1}% loss")),
                    },
                });
            }
        }

        // --- Per-page content and link losses ---
        for (key, baseline_state) in &baseline.urls {
            let candidate_state = match snapshot.urls.get(key) {
                Some(s) => s,
                None => {
                    // URL completely lost
                    if baseline_state.is_indexable {
                        findings.push(Finding {
                            rule_id: "CC-DIFF-LOSS-INDEXABLE".to_string(),
                            severity: Severity::Error,
                            url: key.clone(),
                            message: "An indexable URL from the baseline is missing \
                                      in the candidate."
                                .to_string(),
                            evidence: Evidence {
                                declared: Some("present in baseline".to_string()),
                                observed: Some("missing in candidate".to_string()),
                                canonical: None,
                                detail: None,
                            },
                        });
                    }
                    continue;
                }
            };

            // Content loss
            if let (Some(bw), Some(cw)) = (baseline_state.word_count, candidate_state.word_count) {
                if bw > 0 {
                    let loss_pct = ((bw as f64 - cw as f64) / bw as f64) * 100.0;
                    if loss_pct > self.policy.max_heading_loss_percent && loss_pct > 50.0 {
                        findings.push(Finding {
                            rule_id: "CC-DIFF-LOSS-CONTENT".to_string(),
                            severity: Severity::Warning,
                            url: key.clone(),
                            message: format!(
                                "Word count decreased by {loss_pct:.1}% \
                                 (baseline: {bw}, candidate: {cw})."
                            ),
                            evidence: Evidence {
                                declared: Some(format!("baseline words: {bw}")),
                                observed: Some(format!("candidate words: {cw}")),
                                canonical: None,
                                detail: Some(format!("{loss_pct:.1}% loss")),
                            },
                        });
                    }
                }
            }

            // Heading loss
            if let (Some(bh), Some(ch)) =
                (baseline_state.heading_count, candidate_state.heading_count)
            {
                if bh > 0 {
                    let loss_pct = ((bh as f64 - ch as f64) / bh as f64) * 100.0;
                    if loss_pct > self.policy.max_heading_loss_percent {
                        findings.push(Finding {
                            rule_id: "CC-DIFF-LOSS-CONTENT".to_string(),
                            severity: Severity::Warning,
                            url: key.clone(),
                            message: format!(
                                "Heading count decreased by {loss_pct:.1}% \
                                 (baseline: {bh}, candidate: {ch})."
                            ),
                            evidence: Evidence {
                                declared: Some(format!("baseline headings: {bh}")),
                                observed: Some(format!("candidate headings: {ch}")),
                                canonical: None,
                                detail: Some(format!("{loss_pct:.1}% loss")),
                            },
                        });
                    }
                }
            }

            // Link loss
            let bl = baseline_state.internal_links_out.len();
            let cl = candidate_state.internal_links_out.len();
            if bl > 0 {
                let loss_pct = ((bl as f64 - cl as f64) / bl as f64) * 100.0;
                if loss_pct > self.policy.max_link_loss_percent {
                    findings.push(Finding {
                        rule_id: "CC-DIFF-LOSS-LINKS".to_string(),
                        severity: Severity::Warning,
                        url: key.clone(),
                        message: format!(
                            "Internal link count decreased by {loss_pct:.1}% \
                             (baseline: {bl}, candidate: {cl})."
                        ),
                        evidence: Evidence {
                            declared: Some(format!("baseline links: {bl}")),
                            observed: Some(format!("candidate links: {cl}")),
                            canonical: None,
                            detail: Some(format!("{loss_pct:.1}% loss")),
                        },
                    });
                }
            }
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

    fn make_snapshot_with_stats(urls: BTreeMap<String, UrlState>, indexable: usize) -> Snapshot {
        let mut stats = Statistics::from_url_states(&urls);
        stats.indexable_urls = indexable;
        Snapshot {
            version: "1.0".to_string(),
            tool: "crawlcontract".to_string(),
            base_url: Url::parse("https://example.test").unwrap(),
            public_origin: None,
            scanned_at: crate::model::snapshot::utc_now(),
            statistics: stats,
            urls,
        }
    }

    #[test]
    fn indexable_url_loss_detected() {
        let baseline_urls = BTreeMap::new();
        let baseline = make_snapshot_with_stats(baseline_urls, 100);

        let candidate_urls = BTreeMap::new();
        let candidate = make_snapshot_with_stats(candidate_urls, 50);

        let rule = DiffLossRule {
            baseline,
            policy: DiffPolicy::default(),
        };

        let findings = rule.evaluate(&candidate);
        assert!(findings
            .iter()
            .any(|f| f.rule_id == "CC-DIFF-LOSS-INDEXABLE"));
    }

    #[test]
    fn minor_loss_within_threshold() {
        let baseline_urls = BTreeMap::new();
        let baseline = make_snapshot_with_stats(baseline_urls, 100);

        let candidate_urls = BTreeMap::new();
        let candidate = make_snapshot_with_stats(candidate_urls, 98);

        let rule = DiffLossRule {
            baseline,
            policy: DiffPolicy::default(), // 5% threshold
        };

        let findings = rule.evaluate(&candidate);
        assert!(!findings
            .iter()
            .any(|f| f.rule_id == "CC-DIFF-LOSS-INDEXABLE"));
    }

    #[test]
    fn per_page_content_loss() {
        let url = Url::parse("https://example.test/page").unwrap();
        let key = url.to_string();

        let mut baseline_urls = BTreeMap::new();
        let mut baseline_state = UrlState::new(url.clone());
        baseline_state.word_count = Some(1000);
        baseline_state.heading_count = Some(10);
        baseline_state.internal_links_out = vec![
            Url::parse("https://example.test/a").unwrap(),
            Url::parse("https://example.test/b").unwrap(),
            Url::parse("https://example.test/c").unwrap(),
            Url::parse("https://example.test/d").unwrap(),
            Url::parse("https://example.test/e").unwrap(),
        ];
        baseline_urls.insert(key.clone(), baseline_state);
        let baseline = make_snapshot_with_stats(baseline_urls, 1);

        let mut candidate_urls = BTreeMap::new();
        let mut candidate_state = UrlState::new(url.clone());
        candidate_state.word_count = Some(100); // 90% loss
        candidate_state.heading_count = Some(2); // 80% loss
        candidate_state.internal_links_out = vec![Url::parse("https://example.test/a").unwrap()]; // 80% loss
        candidate_urls.insert(key.clone(), candidate_state);
        let candidate = make_snapshot_with_stats(candidate_urls, 1);

        let rule = DiffLossRule {
            baseline,
            policy: DiffPolicy::default(),
        };

        let findings = rule.evaluate(&candidate);
        assert!(findings.iter().any(|f| f.rule_id == "CC-DIFF-LOSS-CONTENT"));
        assert!(findings.iter().any(|f| f.rule_id == "CC-DIFF-LOSS-LINKS"));
    }
}
