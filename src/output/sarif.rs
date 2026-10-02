use serde_json::json;

use crate::rules::registry::{rule_metadata, Finding, Severity};

const RULES_URI: &str = "https://github.com/lame13/crawlcontract#rules";

/// Serialize findings as SARIF 2.1.0 format.
pub fn findings_to_sarif(findings: &[Finding]) -> anyhow::Result<String> {
    let rules: Vec<serde_json::Value> = {
        let mut seen = std::collections::BTreeSet::new();
        findings
            .iter()
            .filter(|f| seen.insert(&f.rule_id))
            .map(|f| {
                let metadata = rule_metadata(&f.rule_id);
                json!({
                    "id": f.rule_id,
                    "name": f.rule_id,
                    "shortDescription": {
                        "text": metadata
                            .map(|rule| rule.description)
                            .unwrap_or(f.message.as_str())
                    },
                    "helpUri": RULES_URI,
                    "defaultConfiguration": {
                        "level": severity_to_sarif_level(
                            metadata.map(|rule| rule.severity).unwrap_or(f.severity)
                        )
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
                }],
                // Stable per-run identity so consumers such as GitHub code
                // scanning can track one finding across runs.
                "partialFingerprints": {
                    "crawlcontractFindingKey": json!(f.key()).to_string()
                },
                "properties": {
                    "ruleId": f.rule_id,
                    "severity": f.severity.to_string()
                }
            });

            if let Some(ref declared) = f.evidence.declared {
                result["properties"]["declared"] = json!(declared);
            }
            if let Some(ref observed) = f.evidence.observed {
                result["properties"]["observed"] = json!(observed);
            }
            if let Some(ref canonical) = f.evidence.canonical {
                result["properties"]["canonical"] = json!(canonical);
            }
            if let Some(ref detail) = f.evidence.detail {
                result["properties"]["detail"] = json!(detail);
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
