use serde_json::json;

use crate::rules::registry::{Finding, Severity};

/// Serialize findings as SARIF 2.1.0 format.
pub fn findings_to_sarif(findings: &[Finding]) -> anyhow::Result<String> {
    let rules: Vec<serde_json::Value> = {
        let mut seen = std::collections::BTreeSet::new();
        findings
            .iter()
            .filter(|f| seen.insert(&f.rule_id))
            .map(|f| {
                json!({
                    "id": f.rule_id,
                    "name": f.rule_id,
                    "shortDescription": {
                        "text": f.message
                    },
                    "defaultConfiguration": {
                        "level": severity_to_sarif_level(f.severity)
                    }
                })
            })
            .collect()
    };

    let results: Vec<serde_json::Value> = findings
        .iter()
        .map(|f| {
            let mut result = json!({
                "ruleId": f.rule_id,
                "level": severity_to_sarif_level(f.severity),
                "message": {
                    "text": f.message
                },
                "locations": [{
                    "physicalLocation": {
                        "artifactLocation": {
                            "uri": f.url
                        }
                    }
                }]
            });

            // Add properties for evidence
            let mut props = serde_json::Map::new();
            if let Some(ref declared) = f.evidence.declared {
                props.insert("declared".to_string(), json!(declared));
            }
            if let Some(ref observed) = f.evidence.observed {
                props.insert("observed".to_string(), json!(observed));
            }
            if let Some(ref canonical) = f.evidence.canonical {
                props.insert("canonical".to_string(), json!(canonical));
            }
            if !props.is_empty() {
                result["properties"] = json!(props);
            }

            result
        })
        .collect();

    let sarif = json!({
        "$schema": "https://raw.githubusercontent.com/oasis-tcs/sarif-spec/master/Schemata/sarif-schema-2.1.0.json",
        "version": "2.1.0",
        "runs": [{
            "tool": {
                "driver": {
                    "name": "crawlcontract",
                    "version": env!("CARGO_PKG_VERSION"),
                    "informationUri": "https://github.com/lame13/crawlcontract",
                    "rules": rules
                }
            },
            "results": results
        }]
    });

    Ok(serde_json::to_string_pretty(&sarif)?)
}

fn severity_to_sarif_level(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Info => "note",
    }
}
