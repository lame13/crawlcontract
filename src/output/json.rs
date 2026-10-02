use std::collections::BTreeSet;

use serde_json::{json, Value};

use crate::diff::engine::DiffSummary;
use crate::model::snapshot::Snapshot;
use crate::rules::registry::{Finding, FindingKey, Severity};

/// Everything the JSON report needs to describe one run.
pub struct ReportInput<'a> {
    /// The subcommand that produced the findings, for example `scan` or `diff`.
    pub command: &'a str,
    pub findings: &'a [Finding],
    /// The snapshot the findings were evaluated against, when there is one.
    pub snapshot: Option<&'a Snapshot>,
    /// Baseline comparison, when the run compared against another snapshot.
    pub diff: Option<&'a DiffSummary>,
    /// The baseline snapshot itself, for provenance.
    pub baseline: Option<&'a Snapshot>,
    /// Finding keys that already existed in the baseline.
    pub pre_existing: Option<&'a BTreeSet<FindingKey>>,
}

/// Output a machine-readable report.
///
/// The diff payload is the important part: consumers get the lost, gained, and
/// changed URL lists instead of having to parse terminal output.
pub fn report_to_json(input: &ReportInput<'_>) -> anyhow::Result<String> {
    let findings: Vec<Value> = input
        .findings
        .iter()
        .map(|finding| {
            let mut value = serde_json::to_value(finding)?;
            if let Some(pre_existing) = input.pre_existing {
                let key = finding.key();
                if let Some(object) = value.as_object_mut() {
                    object.insert("new".to_string(), json!(!pre_existing.contains(&key)));
                }
            }
            Ok(value)
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    let count = |severity: Severity| {
        input
            .findings
            .iter()
            .filter(|finding| finding.severity == severity)
            .count()
    };

    let new_count = input.pre_existing.map(|pre_existing| {
        input
            .findings
            .iter()
            .filter(|finding| !pre_existing.contains(&finding.key()))
            .count()
    });

    let mut output = json!({
        "version": "1.0",
        "tool": "crawlcontract",
        "tool_version": env!("CARGO_PKG_VERSION"),
        "command": input.command,
        "findings": findings,
        "summary": {
            "total": input.findings.len(),
            "errors": count(Severity::Error),
            "warnings": count(Severity::Warning),
            "infos": count(Severity::Info),
            "new": new_count,
        }
    });

    if let Some(snapshot) = input.snapshot {
        output["snapshot"] = json!({
            "base_url": snapshot.base_url,
            "public_origin": snapshot.public_origin,
            "scanned_at": snapshot.scanned_at,
            "schema_version": snapshot.version,
            "statistics": snapshot.statistics,
        });
    }

    if let Some(diff) = input.diff {
        let changed_urls: Vec<Value> = diff
            .changed_urls
            .iter()
            .map(|(url, changes)| {
                json!({
                    "url": url,
                    "changes": changes,
                })
            })
            .collect();

        output["diff"] = json!({
            "baseline": input.baseline.map(|baseline| json!({
                "base_url": baseline.base_url,
                "scanned_at": baseline.scanned_at,
                "schema_version": baseline.version,
                "tool": baseline.tool,
            })),
            "indexable_urls": {
                "baseline": diff.indexable_baseline,
                "candidate": diff.indexable_candidate,
                "delta": diff.indexable_candidate as i64 - diff.indexable_baseline as i64,
            },
            "lost_urls": diff.lost_urls,
            "gained_urls": diff.gained_urls,
            "changed_urls": changed_urls,
            "counts": {
                "lost": diff.lost_urls.len(),
                "gained": diff.gained_urls.len(),
                "changed": diff.changed_urls.len(),
            },
        });
    }

    Ok(serde_json::to_string_pretty(&output)?)
}

/// Output findings as JSON.
pub fn findings_to_json(findings: &[Finding]) -> anyhow::Result<String> {
    report_to_json(&ReportInput {
        command: "scan",
        findings,
        snapshot: None,
        diff: None,
        baseline: None,
        pre_existing: None,
    })
}

/// Serialize a snapshot to JSON (for --snapshot flag).
pub fn snapshot_to_json(snapshot: &Snapshot) -> anyhow::Result<String> {
    Ok(serde_json::to_string_pretty(snapshot)?)
}

/// Deserialize a snapshot from JSON.
pub fn snapshot_from_json(json: &str) -> anyhow::Result<Snapshot> {
    let snapshot: Snapshot = serde_json::from_str(json)?;
    if snapshot.version != "1.0" {
        anyhow::bail!("unsupported snapshot schema version: {}", snapshot.version);
    }
    if snapshot.tool != "crawlcontract" {
        anyhow::bail!(
            "snapshot was produced by an unsupported tool: {}",
            snapshot.tool
        );
    }

    let computed = crate::model::snapshot::Statistics::from_url_states(&snapshot.urls);
    if snapshot.statistics != computed {
        anyhow::bail!("snapshot statistics do not match its URL states");
    }

    Ok(snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::snapshot::{utc_now, Statistics};
    use crate::model::url_state::UrlState;
    use crate::rules::registry::Evidence;
    use std::collections::BTreeMap;
    use url::Url;

    fn empty_snapshot() -> Snapshot {
        Snapshot {
            version: "1.0".into(),
            tool: "crawlcontract".into(),
            base_url: Url::parse("https://example.test").unwrap(),
            public_origin: None,
            scanned_at: utc_now(),
            urls: BTreeMap::new(),
            statistics: Statistics::from_url_states(&BTreeMap::new()),
        }
    }

    #[test]
    fn rejects_stale_snapshot_statistics() {
        let mut snapshot = empty_snapshot();
        snapshot.statistics.total_urls = 1;
        let json = serde_json::to_string(&snapshot).unwrap();
        assert!(snapshot_from_json(&json).is_err());
    }

    #[test]
    fn rejects_unknown_snapshot_schema() {
        let mut snapshot = empty_snapshot();
        snapshot.version = "2.0".into();
        let json = serde_json::to_string(&snapshot).unwrap();
        assert!(snapshot_from_json(&json).is_err());
    }

    #[test]
    fn diff_payload_lists_url_changes() {
        let baseline = empty_snapshot();
        let mut candidate = empty_snapshot();
        let new_url = Url::parse("https://example.test/new").unwrap();
        candidate
            .urls
            .insert(new_url.to_string(), UrlState::new(new_url));
        candidate.statistics = Statistics::from_url_states(&candidate.urls);

        let summary = crate::diff::engine::diff_snapshots(&baseline, &candidate);
        let report = report_to_json(&ReportInput {
            command: "diff",
            findings: &[],
            snapshot: Some(&candidate),
            diff: Some(&summary),
            baseline: Some(&baseline),
            pre_existing: None,
        })
        .unwrap();

        let parsed: Value = serde_json::from_str(&report).unwrap();
        assert_eq!(
            parsed["diff"]["gained_urls"][0],
            json!("https://example.test/new")
        );
        assert_eq!(parsed["diff"]["counts"]["gained"], json!(1));
        assert_eq!(parsed["command"], json!("diff"));
    }

    #[test]
    fn findings_are_marked_new_against_a_baseline() {
        let finding = Finding {
            rule_id: "CC-ORPHAN-001".to_string(),
            severity: Severity::Warning,
            url: "https://example.test/orphan".to_string(),
            message: "orphan".to_string(),
            evidence: Evidence {
                declared: None,
                observed: None,
                canonical: None,
                detail: None,
            },
        };
        let pre_existing = BTreeSet::from([(
            "CC-CANONICAL-PRESENCE-001".to_string(),
            "https://example.test/other".to_string(),
            Severity::Warning,
            None,
        )]);

        let report = report_to_json(&ReportInput {
            command: "scan",
            findings: std::slice::from_ref(&finding),
            snapshot: None,
            diff: None,
            baseline: None,
            pre_existing: Some(&pre_existing),
        })
        .unwrap();

        let parsed: Value = serde_json::from_str(&report).unwrap();
        assert_eq!(parsed["findings"][0]["new"], json!(true));
        assert_eq!(parsed["summary"]["new"], json!(1));
    }
}
