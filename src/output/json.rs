use serde_json;

use crate::model::snapshot::Snapshot;
use crate::rules::registry::Finding;

/// Output findings as JSON.
pub fn findings_to_json(findings: &[Finding]) -> anyhow::Result<String> {
    let output = serde_json::json!({
        "version": "1.0",
        "tool": "crawlcontract",
        "findings": findings,
        "summary": {
            "total": findings.len(),
            "errors": findings.iter().filter(|f| f.severity == crate::rules::registry::Severity::Error).count(),
            "warnings": findings.iter().filter(|f| f.severity == crate::rules::registry::Severity::Warning).count(),
            "infos": findings.iter().filter(|f| f.severity == crate::rules::registry::Severity::Info).count(),
        }
    });
    Ok(serde_json::to_string_pretty(&output)?)
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
}
