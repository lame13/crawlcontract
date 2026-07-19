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
    Ok(serde_json::from_str(json)?)
}
