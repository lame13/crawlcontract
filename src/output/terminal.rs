use crate::model::snapshot::Snapshot;
use crate::rules::registry::{Finding, Severity};

/// Print findings to the terminal with colors and grouping.
pub fn print_findings(findings: &[Finding]) {
    use colored::Colorize;

    if findings.is_empty() {
        println!("{}", "✓ No issues found.".green().bold());
        return;
    }

    // Group by rule ID
    let mut grouped: Vec<(&str, Vec<&Finding>)> = Vec::new();
    let mut current_rule = "";
    let mut current_group = Vec::new();

    for finding in findings {
        if finding.rule_id != current_rule {
            if !current_group.is_empty() {
                grouped.push((current_rule, current_group));
            }
            current_rule = &finding.rule_id;
            current_group = Vec::new();
        }
        current_group.push(finding);
    }
    if !current_group.is_empty() {
        grouped.push((current_rule, current_group));
    }

    for (rule_id, group) in &grouped {
        let severity = group[0].severity;
        let severity_str = match severity {
            Severity::Error => "ERROR".red().bold(),
            Severity::Warning => "WARNING".yellow().bold(),
            Severity::Info => "INFO".blue().bold(),
        };
        let label = if group.len() == 1 {
            "finding"
        } else {
            "findings"
        };

        println!(
            "\n{}  {}  ({} {label})",
            rule_id.cyan(),
            severity_str,
            group.len()
        );
        println!("{}", "─".repeat(60));

        for finding in group {
            println!("  URL:       {}", finding.url);
            if let Some(ref declared) = finding.evidence.declared {
                println!("  Declared:  {declared}");
            }
            if let Some(ref observed) = finding.evidence.observed {
                println!("  Observed:  {observed}");
            }
            if let Some(ref canonical) = finding.evidence.canonical {
                println!("  Canonical: {canonical}");
            }
            println!("  {}", finding.message);
            println!();
        }
    }

    // Summary
    let errors = findings
        .iter()
        .filter(|f| f.severity == Severity::Error)
        .count();
    let warnings = findings
        .iter()
        .filter(|f| f.severity == Severity::Warning)
        .count();
    let infos = findings
        .iter()
        .filter(|f| f.severity == Severity::Info)
        .count();

    println!("{}", "═".repeat(60));
    let label = if findings.len() == 1 {
        "finding"
    } else {
        "findings"
    };
    print!("{} {label}: ", findings.len());
    if errors > 0 {
        print!("{} ", format!("{errors} error(s)").red().bold());
    }
    if warnings > 0 {
        print!("{} ", format!("{warnings} warning(s)").yellow().bold());
    }
    if infos > 0 {
        print!("{} ", format!("{infos} info(s)").blue().bold());
    }
    println!();
}

/// Print snapshot statistics to the terminal.
pub fn print_statistics(snapshot: &Snapshot) {
    use colored::Colorize;

    println!("{}", "Scan Statistics".bold());
    println!("{}", "─".repeat(40));
    println!("  Base URL:        {}", snapshot.base_url);
    println!("  Total URLs:      {}", snapshot.statistics.total_urls);
    println!(
        "  Indexable:       {}",
        snapshot.statistics.indexable_urls.to_string().green()
    );
    println!(
        "  Non-indexable:   {}",
        snapshot.statistics.non_indexable_urls.to_string().red()
    );
    println!(
        "  Orphaned:        {}",
        snapshot.statistics.orphan_urls.to_string().yellow()
    );
    println!("  In sitemap:      {}", snapshot.statistics.sitemap_urls);
    println!(
        "  From links:      {}",
        snapshot.statistics.internal_link_urls
    );
    println!("  Redirects:       {}", snapshot.statistics.redirect_urls);
    println!("  Broken (≥400):   {}", snapshot.statistics.broken_urls);
    println!();
}

/// Print how many findings are new relative to a baseline snapshot.
pub fn print_baseline_summary(pre_existing: usize, new: usize, total: usize) {
    use colored::Colorize;

    println!(
        "Baseline: {} of {} finding(s) already existed; {} new.",
        pre_existing,
        total,
        new.to_string().bold()
    );
}
