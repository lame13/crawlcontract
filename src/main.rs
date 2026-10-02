use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process;
use std::time::Duration;

use anyhow::Context;
use clap::{Args, Parser, Subcommand, ValueEnum};
use url::Url;

use crawlcontract::config::policy::{parse_public_origin, Policy};
use crawlcontract::diff::engine::diff_snapshots;
use crawlcontract::model::snapshot::Snapshot;
use crawlcontract::output::json::{
    report_to_json, snapshot_from_json, snapshot_to_json, ReportInput,
};
use crawlcontract::output::markdown::findings_to_markdown;
use crawlcontract::output::sarif::findings_to_sarif;
use crawlcontract::output::terminal::{print_baseline_summary, print_findings, print_statistics};
use crawlcontract::rules::canonical_consistency::CanonicalConsistencyRule;
use crawlcontract::rules::canonical_presence::CanonicalPresenceRule;
use crawlcontract::rules::canonical_resolution::CanonicalResolutionRule;
use crawlcontract::rules::diff_rules::DiffLossRule;
use crawlcontract::rules::hreflang_canonical::HreflangCanonicalRule;
use crawlcontract::rules::hreflang_indexability::HreflangIndexabilityRule;
use crawlcontract::rules::hreflang_reciprocal::HreflangReciprocalRule;
use crawlcontract::rules::internal_link_target::InternalLinkTargetRule;
use crawlcontract::rules::orphan::OrphanRule;
use crawlcontract::rules::redirect_resolution::RedirectResolutionRule;
use crawlcontract::rules::registry::{self, run_all_rules, should_fail, Finding, Rule};
use crawlcontract::rules::robots_effective::RobotsEffectiveRule;
use crawlcontract::rules::sitemap_coverage::SitemapCoverageRule;
use crawlcontract::rules::sitemap_indexability::SitemapIndexabilityRule;
use crawlcontract::scanner::default_user_agent;
use crawlcontract::scanner::live::{parse_header_argument, scan_live, LiveScanConfig};
use crawlcontract::scanner::static_dir::{build_snapshot, discover_files};

/// A deterministic CI gate that proves a site's crawl, canonical and
/// indexability signals agree.
#[derive(Parser)]
#[command(name = "crawlcontract", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum OutputFormat {
    Terminal,
    Json,
    Markdown,
    Sarif,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum FailSeverity {
    Error,
    Warning,
    Info,
}

#[derive(Subcommand)]
enum Commands {
    /// Scan a static directory or live URL and produce a snapshot.
    Scan(ScanArgs),

    /// Diff two snapshots and report changes.
    Diff(DiffArgs),
}

#[derive(Args)]
struct ScanArgs {
    /// Path to the static dist directory, or a URL for live scanning.
    source: String,

    /// Public origin to use for URL resolution (e.g., https://example.com).
    #[arg(long)]
    public_origin: Option<String>,

    /// Write the snapshot JSON to this file.
    #[arg(long)]
    snapshot: Option<PathBuf>,

    /// Output format(s): terminal, json, markdown, sarif. Can be specified multiple times.
    #[arg(long, value_delimiter = ',')]
    format: Vec<OutputFormat>,

    /// Write output to a file instead of stdout.
    #[arg(long)]
    output: Option<PathBuf>,

    /// Comma-separated finding severities that cause exit 1. Default: error.
    #[arg(long, value_delimiter = ',')]
    fail_on: Option<Vec<FailSeverity>>,

    /// Path to a policy file (crawlcontract.toml) for exclusions.
    #[arg(long)]
    policy: Option<PathBuf>,

    /// Maximum number of pages to fetch in live mode.
    #[arg(long, default_value = "500")]
    max_pages: usize,

    /// Number of concurrent requests in live mode.
    #[arg(long, default_value = "8")]
    concurrency: usize,

    /// Compare this snapshot against the scan and report regressions.
    ///
    /// Adds the CC-DIFF-LOSS-* rules to the scan and prints the URL changes
    /// between the baseline and the candidate in terminal, JSON, and Markdown.
    #[arg(long, value_name = "SNAPSHOT")]
    baseline: Option<PathBuf>,

    /// Exit 1 only for findings the baseline scan does not already contain.
    ///
    /// Requires --baseline. Existing findings are still reported and still
    /// appear in machine-readable output, marked as `"new": false`.
    #[arg(long, requires = "baseline")]
    fail_on_new: bool,

    /// User-agent used for live requests and robots.txt evaluation.
    #[arg(long, default_value_t = default_user_agent())]
    user_agent: String,

    /// Extra request header for live scans, repeatable. Format: "NAME: VALUE".
    #[arg(long = "header", value_name = "NAME: VALUE")]
    header: Vec<String>,

    /// Per-request timeout in seconds for live scans.
    #[arg(long, default_value = "30")]
    timeout: u64,

    /// Seconds to wait between live page requests; overrides robots.txt Crawl-delay.
    #[arg(long, value_name = "SECONDS")]
    crawl_delay: Option<f64>,
}

#[derive(Args)]
struct DiffArgs {
    /// Path to the baseline snapshot JSON.
    baseline: PathBuf,

    /// Path to the candidate snapshot JSON.
    candidate: PathBuf,

    /// Path to a policy file (crawlcontract.toml) for diff thresholds.
    #[arg(long)]
    policy: Option<PathBuf>,

    /// Output format(s): terminal, json, markdown, sarif. Can be specified multiple times.
    #[arg(long, value_delimiter = ',')]
    format: Vec<OutputFormat>,

    /// Write output to a file instead of stdout.
    #[arg(long)]
    output: Option<PathBuf>,

    /// Comma-separated finding severities that cause exit 1. Default: policy or error.
    #[arg(long, value_delimiter = ',')]
    fail_on: Option<Vec<FailSeverity>>,
}

fn main() {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .init();

    let cli = Cli::parse();

    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    let result = rt.block_on(async {
        match cli.command {
            Commands::Scan(args) => cmd_scan(args).await,
            Commands::Diff(args) => cmd_diff(args),
        }
    });

    match result {
        Ok(should_exit_error) => {
            if should_exit_error {
                process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("Error: {e}");
            for cause in e.chain().skip(1) {
                eprintln!("  Caused by: {cause}");
            }
            process::exit(2);
        }
    }
}

/// Every rule that evaluates a single snapshot.
fn evaluation_rules() -> Vec<Box<dyn Rule>> {
    vec![
        Box::new(SitemapIndexabilityRule),
        Box::new(SitemapCoverageRule),
        Box::new(CanonicalConsistencyRule),
        Box::new(CanonicalPresenceRule),
        Box::new(CanonicalResolutionRule),
        Box::new(RobotsEffectiveRule),
        Box::new(HreflangReciprocalRule),
        Box::new(HreflangCanonicalRule),
        Box::new(HreflangIndexabilityRule),
        Box::new(InternalLinkTargetRule),
        Box::new(RedirectResolutionRule),
        Box::new(OrphanRule),
    ]
}

/// Returns Ok(true) if the process should exit with code 1.
async fn cmd_scan(args: ScanArgs) -> anyhow::Result<bool> {
    let ScanArgs {
        source,
        public_origin,
        snapshot: snapshot_path,
        format: formats,
        output: output_path,
        fail_on,
        policy: policy_path,
        max_pages,
        concurrency,
        baseline: baseline_path,
        fail_on_new,
        user_agent,
        header,
        timeout,
        crawl_delay,
    } = args;

    if timeout == 0 {
        anyhow::bail!("--timeout must be greater than zero");
    }
    if concurrency == 0 {
        anyhow::bail!("--concurrency must be greater than zero");
    }
    let request_timeout = Duration::from_secs(timeout);
    let headers: Vec<(String, String)> = header
        .iter()
        .map(|raw| parse_header_argument(raw.as_str()))
        .collect::<anyhow::Result<_>>()?;
    let crawl_delay = crawl_delay
        .map(|seconds| {
            Duration::try_from_secs_f64(seconds)
                .with_context(|| format!("invalid --crawl-delay value: {seconds}"))
        })
        .transpose()?;

    // Load policy
    let policy = match &policy_path {
        Some(path) => Some(
            Policy::load(path)
                .with_context(|| format!("loading policy from {}", path.display()))?,
        ),
        None => None,
    };

    let public_origin = policy
        .as_ref()
        .and_then(|policy| policy.general.public_origin.clone())
        .or(public_origin);

    // Load the baseline before scanning so a broken baseline fails fast.
    let baseline = match &baseline_path {
        Some(path) => Some(load_snapshot(path)?),
        None => None,
    };

    let source_url = Url::parse(&source)
        .ok()
        .filter(|url| matches!(url.scheme(), "http" | "https"));

    let snapshot = if let Some(start_url) = source_url {
        let public_origin_url = public_origin
            .as_ref()
            .map(|origin| parse_public_origin(origin))
            .transpose()
            .with_context(|| "parsing public origin")?;

        let config = LiveScanConfig {
            start_url,
            public_origin: public_origin_url.clone(),
            max_pages,
            concurrency,
            request_timeout,
            user_agent: user_agent.clone(),
            crawl_delay,
            headers: headers.clone(),
        };

        eprintln!("Live scanning {source} (max {max_pages} pages) ...");
        eprintln!("User-agent: {user_agent}");
        scan_live(config).await.with_context(|| "live scanning")?
    } else {
        let dist_path = PathBuf::from(&source);
        if !dist_path.is_dir() {
            anyhow::bail!("{} is not a directory", dist_path.display());
        }

        // Determine base URL
        let base_url = if let Some(ref origin) = public_origin {
            parse_public_origin(origin)
                .with_context(|| format!("parsing public origin: {origin}"))?
        } else {
            Url::parse("https://example.test").context("parsing default base URL")?
        };

        let public_origin_url = public_origin
            .as_ref()
            .map(|origin| parse_public_origin(origin))
            .transpose()
            .with_context(|| "parsing public origin")?;

        eprintln!("Scanning {} ...", dist_path.display());
        let result =
            discover_files(&dist_path, &base_url).with_context(|| "scanning static directory")?;
        eprintln!(
            "Found {} HTML file(s), robots.txt: {}, sitemap: {}",
            result.html_files.len(),
            result.robots_txt_body.is_some(),
            !result.sitemap_files.is_empty()
        );

        build_snapshot(result, public_origin_url, &user_agent)?
    };

    if let Some(baseline) = &baseline {
        if baseline.base_url != snapshot.base_url {
            anyhow::bail!(
                "snapshot base URLs differ: baseline {}, candidate {}",
                baseline.base_url,
                snapshot.base_url
            );
        }
    }

    // Save snapshot if requested
    if let Some(ref path) = snapshot_path {
        let json = snapshot_to_json(&snapshot)?;
        std::fs::write(path, &json)
            .with_context(|| format!("writing snapshot to {}", path.display()))?;
        eprintln!("Snapshot written to {}", path.display());
    }

    // Findings that already existed in the baseline are computed with the same
    // rule set, minus the comparison rules, so `--fail-on-new` means "new
    // since this baseline" rather than "new since this tool version".
    let pre_existing: Option<BTreeSet<_>> = baseline.as_ref().map(|baseline| {
        run_all_rules(baseline, &evaluation_rules(), policy.as_ref())
            .iter()
            .map(Finding::key)
            .collect()
    });

    let mut rules = evaluation_rules();
    if let Some(baseline) = &baseline {
        rules.push(Box::new(DiffLossRule {
            baseline: baseline.clone(),
            policy: policy.as_ref().map(|p| p.diff.clone()).unwrap_or_default(),
        }));
    }

    let findings = run_all_rules(&snapshot, &rules, policy.as_ref());
    let new_findings: Vec<Finding> = match &pre_existing {
        Some(pre_existing) => findings
            .iter()
            .filter(|finding| !pre_existing.contains(&finding.key()))
            .cloned()
            .collect(),
        None => Vec::new(),
    };

    let formats = if formats.is_empty() {
        vec![OutputFormat::Terminal]
    } else {
        formats
    };

    validate_output_selection(&formats, output_path.as_deref())?;

    let diff_summary = baseline
        .as_ref()
        .map(|baseline| diff_snapshots(baseline, &snapshot));

    if formats.contains(&OutputFormat::Terminal) {
        print_statistics(&snapshot);
        if let Some(summary) = &diff_summary {
            println!("{summary}");
        }
        print_findings(&findings);
        if pre_existing.is_some() {
            print_baseline_summary(
                findings.len() - new_findings.len(),
                new_findings.len(),
                findings.len(),
            );
        }
    }

    write_reports(
        &formats,
        output_path.as_deref(),
        ReportInput {
            command: "scan",
            findings: &findings,
            snapshot: Some(&snapshot),
            diff: diff_summary.as_ref(),
            baseline: baseline.as_ref(),
            pre_existing: pre_existing.as_ref(),
        },
    )?;

    // Determine exit code
    let gating: &[Finding] = if fail_on_new {
        &new_findings
    } else {
        &findings
    };

    let fail = match &fail_on {
        Some(fail_on) => gating.iter().any(|finding| {
            fail_on.iter().any(|severity| match severity {
                FailSeverity::Error => finding.severity == registry::Severity::Error,
                FailSeverity::Warning => finding.severity == registry::Severity::Warning,
                FailSeverity::Info => finding.severity == registry::Severity::Info,
            })
        }),
        None => should_fail(gating, policy.as_ref()),
    };

    Ok(fail)
}

/// Returns Ok(true) if the process should exit with code 1.
fn cmd_diff(args: DiffArgs) -> anyhow::Result<bool> {
    let DiffArgs {
        baseline: baseline_path,
        candidate: candidate_path,
        policy: policy_path,
        format: formats,
        output: output_path,
        fail_on,
    } = args;

    // Load policy
    let policy = match &policy_path {
        Some(path) => Some(
            Policy::load(path)
                .with_context(|| format!("loading policy from {}", path.display()))?,
        ),
        None => None,
    };

    let baseline = load_snapshot(&baseline_path)?;
    let candidate = load_snapshot(&candidate_path)?;
    if baseline.base_url != candidate.base_url {
        anyhow::bail!(
            "snapshot base URLs differ: baseline {}, candidate {}",
            baseline.base_url,
            candidate.base_url
        );
    }

    // Run diff rules
    let diff_policy = policy.as_ref().map(|p| p.diff.clone()).unwrap_or_default();

    let diff_rule = DiffLossRule {
        baseline: baseline.clone(),
        policy: diff_policy,
    };
    let findings = registry::run_all_rules(&candidate, &[Box::new(diff_rule)], policy.as_ref());

    // Print diff summary
    let summary = diff_snapshots(&baseline, &candidate);

    let formats = if formats.is_empty() {
        vec![OutputFormat::Terminal]
    } else {
        formats
    };

    validate_output_selection(&formats, output_path.as_deref())?;

    if formats.contains(&OutputFormat::Terminal) {
        println!("{summary}");
        println!();
        print_findings(&findings);
    }

    write_reports(
        &formats,
        output_path.as_deref(),
        ReportInput {
            command: "diff",
            findings: &findings,
            snapshot: Some(&candidate),
            diff: Some(&summary),
            baseline: Some(&baseline),
            pre_existing: None,
        },
    )?;

    let fail = match &fail_on {
        Some(fail_on) => findings.iter().any(|finding| {
            fail_on.iter().any(|severity| match severity {
                FailSeverity::Error => finding.severity == registry::Severity::Error,
                FailSeverity::Warning => finding.severity == registry::Severity::Warning,
                FailSeverity::Info => finding.severity == registry::Severity::Info,
            })
        }),
        None => should_fail(&findings, policy.as_ref()),
    };

    Ok(fail)
}

fn load_snapshot(path: &Path) -> anyhow::Result<Snapshot> {
    let json = std::fs::read_to_string(path)
        .with_context(|| format!("reading snapshot from {}", path.display()))?;
    snapshot_from_json(&json).with_context(|| format!("parsing snapshot {}", path.display()))
}

/// Render every requested non-terminal format to a file or stdout.
fn write_reports(
    formats: &[OutputFormat],
    output_path: Option<&Path>,
    input: ReportInput<'_>,
) -> anyhow::Result<()> {
    for format in formats {
        match format {
            OutputFormat::Json => {
                let json = report_to_json(&input)?;
                write_output(&json, output_path)?;
            }
            OutputFormat::Markdown => {
                let snapshot = input
                    .snapshot
                    .context("markdown output requires a snapshot")?;
                let md = findings_to_markdown(input.findings, snapshot, input.diff);
                write_output(&md, output_path)?;
            }
            OutputFormat::Sarif => {
                let sarif = findings_to_sarif(input.findings)?;
                write_output(&sarif, output_path)?;
            }
            OutputFormat::Terminal => {}
        }
    }

    Ok(())
}

fn validate_output_selection(
    formats: &[OutputFormat],
    path: Option<&std::path::Path>,
) -> anyhow::Result<()> {
    let file_formats = formats
        .iter()
        .filter(|format| **format != OutputFormat::Terminal)
        .count();
    if file_formats > 1 {
        anyhow::bail!("select at most one of json, markdown, or sarif");
    }
    if path.is_some() && file_formats == 0 {
        anyhow::bail!("--output requires json, markdown, or sarif output");
    }
    Ok(())
}

fn write_output(content: &str, path: Option<&std::path::Path>) -> anyhow::Result<()> {
    match path {
        Some(p) => {
            std::fs::write(p, content)
                .with_context(|| format!("writing output to {}", p.display()))?;
            eprintln!("Output written to {}", p.display());
        }
        None => {
            // If format is non-terminal, write to stdout
            println!("{content}");
        }
    }
    Ok(())
}
