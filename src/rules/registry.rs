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

pub type FindingKey = (String, String, Severity, Option<String>);

impl Finding {
    /// Identity shared by baseline comparisons and report fingerprints.
    /// The subject distinguishes multiple failing targets on one page, and a
    /// severity escalation must not be hidden by an existing warning.
    pub fn key(&self) -> FindingKey {
        (
            self.rule_id.clone(),
            self.url.clone(),
            self.severity,
            self.evidence
                .declared
                .as_ref()
                .or(self.evidence.canonical.as_ref())
                .or(self.evidence.detail.as_ref())
                .cloned(),
        )
    }
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

/// Static metadata for a finding this tool can emit.
pub struct RuleMetadata {
    pub id: &'static str,
    pub severity: Severity,
    pub description: &'static str,
}

/// Every finding rule, in report order.
///
/// This catalog is the single source of truth for policy validation and SARIF
/// rule metadata, so a new rule cannot silently become unavailable to
/// exclusions or undocumented in machine-readable output.
pub const RULE_CATALOG: &[RuleMetadata] = &[
    RuleMetadata {
        id: "CC-SITEMAP-INDEXABILITY-001",
        severity: Severity::Error,
        description: "A URL declared in a sitemap carries an effective noindex directive.",
    },
    RuleMetadata {
        id: "CC-SITEMAP-INDEXABILITY-002",
        severity: Severity::Error,
        description: "A URL declared in a sitemap is blocked by robots.txt.",
    },
    RuleMetadata {
        id: "CC-SITEMAP-INDEXABILITY-003",
        severity: Severity::Warning,
        description: "A URL declared in a sitemap canonicalizes to a different URL.",
    },
    RuleMetadata {
        id: "CC-SITEMAP-INDEXABILITY-004",
        severity: Severity::Error,
        description: "A URL declared in a sitemap does not return HTTP 200 or was not verified.",
    },
    RuleMetadata {
        id: "CC-SITEMAP-COVERAGE-001",
        severity: Severity::Warning,
        description: "An indexable page is missing from every sitemap.",
    },
    RuleMetadata {
        id: "CC-CANONICAL-CONSISTENCY-001",
        severity: Severity::Error,
        description: "The HTML canonical and the HTTP Link canonical disagree.",
    },
    RuleMetadata {
        id: "CC-CANONICAL-CONSISTENCY-003",
        severity: Severity::Error,
        description: "A canonical target does not return HTTP 200 or was not verified.",
    },
    RuleMetadata {
        id: "CC-CANONICAL-CONSISTENCY-004",
        severity: Severity::Error,
        description: "A canonical target returns HTTP 200 but is not indexable.",
    },
    RuleMetadata {
        id: "CC-CANONICAL-PRESENCE-001",
        severity: Severity::Warning,
        description: "An indexable page declares no canonical URL.",
    },
    RuleMetadata {
        id: "CC-CANONICAL-RESOLUTION-001",
        severity: Severity::Error,
        description: "A canonical chain exceeds one hop.",
    },
    RuleMetadata {
        id: "CC-CANONICAL-RESOLUTION-002",
        severity: Severity::Error,
        description: "A canonical cycle was detected.",
    },
    RuleMetadata {
        id: "CC-ROBOTS-EFFECTIVE-001",
        severity: Severity::Warning,
        description: "Meta robots and X-Robots-Tag directives conflict.",
    },
    RuleMetadata {
        id: "CC-HREFLANG-RECIPROCAL-001",
        severity: Severity::Error,
        description: "An hreflang entry is not reciprocated by its target.",
    },
    RuleMetadata {
        id: "CC-HREFLANG-RECIPROCAL-002",
        severity: Severity::Error,
        description: "An hreflang cluster lacks a self-reference.",
    },
    RuleMetadata {
        id: "CC-HREFLANG-CANONICAL-001",
        severity: Severity::Error,
        description: "An hreflang target canonicalizes to a different URL.",
    },
    RuleMetadata {
        id: "CC-HREFLANG-INDEXABILITY-001",
        severity: Severity::Error,
        description: "An hreflang target exists but cannot be indexed.",
    },
    RuleMetadata {
        id: "CC-LINK-TARGET-001",
        severity: Severity::Error,
        description: "An internal link returns a non-200 status without a redirect target.",
    },
    RuleMetadata {
        id: "CC-LINK-TARGET-002",
        severity: Severity::Warning,
        description: "An internal link passes through a redirect.",
    },
    RuleMetadata {
        id: "CC-LINK-TARGET-003",
        severity: Severity::Error,
        description: "An internal link target was not verified.",
    },
    RuleMetadata {
        id: "CC-LINK-TARGET-004",
        severity: Severity::Warning,
        description: "An internal link target canonicalizes elsewhere.",
    },
    RuleMetadata {
        id: "CC-REDIRECT-RESOLUTION-001",
        severity: Severity::Error,
        description: "A redirect chain contains a cycle.",
    },
    RuleMetadata {
        id: "CC-ORPHAN-001",
        severity: Severity::Warning,
        description: "A verified indexable page is unreachable internally.",
    },
    RuleMetadata {
        id: "CC-DIFF-LOSS-INDEXABLE",
        severity: Severity::Error,
        description: "Indexable URL loss exceeds the configured policy threshold.",
    },
    RuleMetadata {
        id: "CC-DIFF-LOSS-CONTENT",
        severity: Severity::Warning,
        description: "Word or heading loss exceeds the configured policy threshold.",
    },
    RuleMetadata {
        id: "CC-DIFF-LOSS-LINKS",
        severity: Severity::Warning,
        description: "Internal link loss exceeds the configured policy threshold.",
    },
];

/// Look up the catalog entry for a finding rule ID.
pub fn rule_metadata(id: &str) -> Option<&'static RuleMetadata> {
    RULE_CATALOG.iter().find(|rule| rule.id == id)
}

/// Returns true when `id` is a finding rule this tool can emit.
pub fn is_finding_rule(id: &str) -> bool {
    rule_metadata(id).is_some()
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
