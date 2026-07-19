use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::config::policy::Policy;
use crate::model::snapshot::Snapshot;

/// Severity of a rule finding.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Warning,
    Error,
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Severity::Info => write!(f, "info"),
            Severity::Warning => write!(f, "warning"),
            Severity::Error => write!(f, "error"),
        }
    }
}

impl Severity {
    pub fn from_str_lossy(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "error" => Severity::Error,
            "warning" => Severity::Warning,
            "info" => Severity::Info,
            _ => Severity::Error,
        }
    }
}

/// Structured evidence attached to a finding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    pub declared: Option<String>,
    pub observed: Option<String>,
    pub canonical: Option<String>,
    pub detail: Option<String>,
}

/// A single rule finding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub rule_id: String,
    pub severity: Severity,
    pub url: String,
    pub message: String,
    pub evidence: Evidence,
}

impl std::fmt::Display for Finding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "{}  {}",
            self.rule_id,
            self.severity.to_string().to_uppercase()
        )?;
        writeln!(f)?;
        writeln!(f, "URL:       {}", self.url)?;
        if let Some(ref declared) = self.evidence.declared {
            writeln!(f, "Declared:  {declared}")?;
        }
        if let Some(ref observed) = self.evidence.observed {
            writeln!(f, "Observed:  {observed}")?;
        }
        if let Some(ref canonical) = self.evidence.canonical {
            writeln!(f, "Canonical: {canonical}")?;
        }
        writeln!(f)?;
        write!(f, "{}", self.message)
    }
}

/// A rule that can evaluate a snapshot and produce findings.
pub trait Rule: Send + Sync {
    fn id(&self) -> &str;
    fn evaluate(&self, snapshot: &Snapshot) -> Vec<Finding>;
}

/// Run all registered rules against a snapshot, applying policy exclusions.
pub fn run_all_rules(
    snapshot: &Snapshot,
    rules: &[Box<dyn Rule>],
    policy: Option<&Policy>,
) -> Vec<Finding> {
    let mut all_findings = Vec::new();

    for rule in rules {
        let findings = rule.evaluate(snapshot);
        for finding in findings {
            // Apply exclusions
            if let Some(policy) = policy {
                if policy.is_excluded(&finding.rule_id, &finding.url) {
                    continue;
                }
            }
            all_findings.push(finding);
        }
    }

    // Sort by severity (error first), then rule_id, then url
    all_findings.sort_by(|a, b| {
        b.severity
            .cmp(&a.severity)
            .then_with(|| a.rule_id.cmp(&b.rule_id))
            .then_with(|| a.url.cmp(&b.url))
    });

    all_findings
}

/// Summary of findings grouped by rule.
pub fn findings_summary(findings: &[Finding]) -> BTreeMap<&str, Vec<&Finding>> {
    let mut summary: BTreeMap<&str, Vec<&Finding>> = BTreeMap::new();
    for finding in findings {
        summary.entry(&finding.rule_id).or_default().push(finding);
    }
    summary
}

/// Returns true if findings include any severity that should cause failure.
pub fn should_fail(findings: &[Finding], policy: Option<&Policy>) -> bool {
    let fail_severities = match policy {
        Some(p) => &p.general.fail_on,
        None => return findings.iter().any(|f| f.severity == Severity::Error),
    };

    findings.iter().any(|f| {
        fail_severities
            .iter()
            .any(|s| s.eq_ignore_ascii_case(&f.severity.to_string()))
    })
}
