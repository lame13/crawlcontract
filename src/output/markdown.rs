use crate::diff::engine::{DiffSummary, UrlChange};
use crate::model::snapshot::Snapshot;
use crate::rules::registry::{findings_summary, Finding, Severity};

/// Output findings as Markdown.
pub fn findings_to_markdown(
    findings: &[Finding],
    snapshot: &Snapshot,
    diff: Option<&DiffSummary>,
) -> String {
    let mut md = String::new();

    md.push_str("# CrawlContract Report\n\n");
    md.push_str(&format!("**Base URL:** {}\n", snapshot.base_url));
    md.push_str(&format!(
        "**Scanned:** {}\n\n",
        snapshot.scanned_at.format("%Y-%m-%d %H:%M:%S UTC")
    ));

    // Statistics
    md.push_str("## Statistics\n\n");
    md.push_str("| Metric | Count |\n");
    md.push_str("|--------|-------|\n");
    md.push_str(&format!(
        "| Total URLs | {} |\n",
        snapshot.statistics.total_urls
    ));
    md.push_str(&format!(
        "| Indexable | {} |\n",
        snapshot.statistics.indexable_urls
    ));
    md.push_str(&format!(
        "| Non-indexable | {} |\n",
        snapshot.statistics.non_indexable_urls
    ));
    md.push_str(&format!(
        "| Orphaned | {} |\n",
        snapshot.statistics.orphan_urls
    ));
    md.push_str(&format!(
        "| In sitemap | {} |\n",
        snapshot.statistics.sitemap_urls
    ));
    md.push_str(&format!(
        "| Broken (≥400) | {} |\n",
        snapshot.statistics.broken_urls
    ));
    md.push('\n');

    if let Some(diff) = diff {
        md.push_str(&diff_section(diff));
    }

    if findings.is_empty() {
        md.push_str("## Findings\n\n");
        md.push_str("✓ No issues found.\n");
        return md;
    }

    // Findings summary
    let summary = findings_summary(findings);
    let errors = findings
        .iter()
        .filter(|f| f.severity == Severity::Error)
        .count();
    let warnings = findings
        .iter()
        .filter(|f| f.severity == Severity::Warning)
        .count();

    md.push_str("## Findings Summary\n\n");
    md.push_str(&format!(
        "**{} errors, {} warnings** across {} rules\n\n",
        errors,
        warnings,
        summary.len()
    ));

    md.push_str("| Rule | Severity | Count |\n");
    md.push_str("|------|----------|-------|\n");
    for (rule_id, group) in &summary {
        let sev = match group[0].severity {
            Severity::Error => "🔴 Error",
            Severity::Warning => "🟡 Warning",
            Severity::Info => "🔵 Info",
        };
        md.push_str(&format!("| `{rule_id}` | {sev} | {} |\n", group.len()));
    }
    md.push('\n');

    // Detailed findings
    md.push_str("## Detailed Findings\n\n");
    for (rule_id, group) in &summary {
        md.push_str(&format!("### {rule_id}\n\n"));
        for finding in group.iter() {
            md.push_str(&format!(
                "**{}** — `{}`\n\n",
                finding.url,
                match finding.severity {
                    Severity::Error => "ERROR",
                    Severity::Warning => "WARNING",
                    Severity::Info => "INFO",
                }
            ));
            md.push_str(&format!("{}\n\n", finding.message));
            if let Some(ref declared) = finding.evidence.declared {
                md.push_str(&format!("- **Declared:** {declared}\n"));
            }
            if let Some(ref observed) = finding.evidence.observed {
                md.push_str(&format!("- **Observed:** {observed}\n"));
            }
            if let Some(ref canonical) = finding.evidence.canonical {
                md.push_str(&format!("- **Canonical:** {canonical}\n"));
            }
            md.push('\n');
        }
    }

    md
}

/// Render the baseline comparison as a Markdown section.
fn diff_section(diff: &DiffSummary) -> String {
    let mut md = String::new();

    let delta = diff.indexable_candidate as i64 - diff.indexable_baseline as i64;
    md.push_str("## Baseline Comparison\n\n");
    md.push_str(&format!(
        "Indexable URLs: **{} → {}** ({delta:+})\n\n",
        diff.indexable_baseline, diff.indexable_candidate
    ));

    if !diff.lost_urls.is_empty() {
        md.push_str(&format!("### Lost URLs ({})\n\n", diff.lost_urls.len()));
        for url in &diff.lost_urls {
            md.push_str(&format!("- `{url}`\n"));
        }
        md.push('\n');
    }

    if !diff.gained_urls.is_empty() {
        md.push_str(&format!("### Gained URLs ({})\n\n", diff.gained_urls.len()));
        for url in &diff.gained_urls {
            md.push_str(&format!("- `{url}`\n"));
        }
        md.push('\n');
    }

    if !diff.changed_urls.is_empty() {
        md.push_str(&format!(
            "### Changed URLs ({})\n\n",
            diff.changed_urls.len()
        ));
        for (url, changes) in &diff.changed_urls {
            md.push_str(&format!("- `{url}`\n"));
            for change in changes {
                md.push_str(&format!("  - {}\n", describe_change(change)));
            }
        }
        md.push('\n');
    }

    md
}

fn describe_change(change: &UrlChange) -> String {
    match change {
        UrlChange::CanonicalChanged { from, to } => {
            format!("canonical: {from:?} → {to:?}")
        }
        UrlChange::IndexChanged { from, to } => format!("indexable: {from} → {to}"),
        UrlChange::RobotsChanged => "robots directive changed".to_string(),
        UrlChange::WordCountChanged { from, to } => format!("words: {from:?} → {to:?}"),
        UrlChange::LinkCountChanged { from, to } => format!("links: {from} → {to}"),
    }
}
