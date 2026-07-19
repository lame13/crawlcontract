use std::path::PathBuf;
use std::process;

use anyhow::Context;
use clap::{Parser, Subcommand};
use url::Url;

use crawlcontract::config::policy::Policy;
use crawlcontract::diff::engine::diff_snapshots;
use crawlcontract::output::json::{findings_to_json, snapshot_from_json, snapshot_to_json};
use crawlcontract::output::markdown::findings_to_markdown;
use crawlcontract::output::sarif::findings_to_sarif;
use crawlcontract::output::terminal::{print_findings, print_statistics};
use crawlcontract::rules::canonical_consistency::CanonicalConsistencyRule;
use crawlcontract::rules::canonical_resolution::CanonicalResolutionRule;
use crawlcontract::rules::diff_rules::DiffLossRule;
use crawlcontract::rules::hreflang_canonical::HreflangCanonicalRule;
use crawlcontract::rules::hreflang_reciprocal::HreflangReciprocalRule;
use crawlcontract::rules::internal_link_target::InternalLinkTargetRule;
use crawlcontract::rules::orphan::OrphanRule;
use crawlcontract::rules::registry::{self, run_all_rules, should_fail, Rule};
use crawlcontract::rules::robots_effective::RobotsEffectiveRule;
use crawlcontract::rules::sitemap_indexability::SitemapIndexabilityRule;
use crawlcontract::scanner::live::{scan_live, LiveScanConfig};
use crawlcontract::scanner::static_dir::{build_snapshot, discover_files};

/// A deterministic CI gate that proves a site's crawl, canonical and
/// indexability signals agree.
#[derive(Parser)]
#[command(name = "crawlcontract", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Scan a static directory or live URL and produce a snapshot.
    Scan {
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
        format: Vec<String>,

        /// Write output to a file instead of stdout.
        #[arg(long)]
        output: Option<PathBuf>,

        /// Fail (exit 1) on findings at or above this severity.
        /// Values: error, warning, info. Default: error.
        #[arg(long)]
        fail_on: Option<String>,

        /// Path to a policy file (crawlcontract.toml) for exclusions.
        #[arg(long)]
        policy: Option<PathBuf>,

        /// Maximum number of pages to fetch in live mode.
        #[arg(long, default_value = "500")]
        max_pages: usize,

        /// Number of concurrent requests in live mode.
        #[arg(long, default_value = "8")]
        concurrency: usize,
    },

    /// Diff two snapshots and report changes.
    Diff {
        /// Path to the baseline snapshot JSON.
        baseline: PathBuf,

        /// Path to the candidate snapshot JSON.
        candidate: PathBuf,

        /// Path to a policy file (crawlcontract.toml) for diff thresholds.
        #[arg(long)]
        policy: Option<PathBuf>,

        /// Output format(s): terminal, json, markdown. Can be specified multiple times.
        #[arg(long, value_delimiter = ',')]
        format: Vec<String>,

        /// Write output to a file instead of stdout.
        #[arg(long)]
        output: Option<PathBuf>,
    },
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
            Commands::Scan {
                source,
                public_origin,
                snapshot,
                format,
                output,
                fail_on,
                policy,
                max_pages,
                concurrency,
            } => {
                cmd_scan(
                    source,
                    public_origin,
                    snapshot,
                    format,
                    output,
                    fail_on,
                    policy,
                    max_pages,
                    concurrency,
                )
                .await
            }
            Commands::Diff {
                baseline,
                candidate,
                policy,
                format,
                output,
            } => cmd_diff(baseline, candidate, policy, format, output),
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

/// Returns Ok(true) if the process should exit with code 1.
#[allow(clippy::too_many_arguments)]
async fn cmd_scan(
    source: String,
    public_origin: Option<String>,
    snapshot_path: Option<PathBuf>,
    formats: Vec<String>,
    output_path: Option<PathBuf>,
    fail_on: Option<String>,
    policy_path: Option<PathBuf>,
    max_pages: usize,
    concurrency: usize,
) -> anyhow::Result<bool> {
    // Load policy
    let policy = match &policy_path {
        Some(path) => Some(
            Policy::load(path)
                .with_context(|| format!("loading policy from {}", path.display()))?,
        ),
        None => None,
    };

    // Determine if source is a URL or a path
    let is_url = source.starts_with("http://") || source.starts_with("https://");

    let snapshot = if is_url {
        let start_url = Url::parse(&source).with_context(|| format!("parsing URL: {source}"))?;
        let public_origin_url = public_origin
            .as_ref()
            .map(|o| Url::parse(o))
            .transpose()
            .with_context(|| "parsing public origin")?;

        let config = LiveScanConfig {
            start_url,
            public_origin: public_origin_url.clone(),
            max_pages,
            concurrency,
            ..LiveScanConfig::default()
        };

        eprintln!("Live scanning {source} (max {max_pages} pages) ...");
        scan_live(config).await.with_context(|| "live scanning")?
    } else {
        let dist_path = PathBuf::from(&source);
        if !dist_path.is_dir() {
            anyhow::bail!("{} is not a directory", dist_path.display());
        }

        // Determine base URL
        let base_url = if let Some(ref origin) = public_origin {
            Url::parse(origin).with_context(|| format!("parsing public origin: {origin}"))?
        } else {
            Url::parse("https://example.test").context("parsing default base URL")?
        };

        let public_origin_url = public_origin
            .as_ref()
            .map(|o| Url::parse(o))
            .transpose()
            .with_context(|| "parsing public origin")?;

        eprintln!("Scanning {} ...", dist_path.display());
        let result =
            discover_files(&dist_path, &base_url).with_context(|| "scanning static directory")?;
        eprintln!(
            "Found {} HTML file(s), robots.txt: {}, sitemap: {}",
            result.html_files.len(),
            result.robots_txt_body.is_some(),
            result.sitemap_body.is_some()
        );

        build_snapshot(result, public_origin_url)
    };

    // Save snapshot if requested
    if let Some(ref path) = snapshot_path {
        let json = snapshot_to_json(&snapshot)?;
        std::fs::write(path, &json)
            .with_context(|| format!("writing snapshot to {}", path.display()))?;
        eprintln!("Snapshot written to {}", path.display());
    }

    // Build rule set
    let rules: Vec<Box<dyn Rule>> = vec![
        Box::new(SitemapIndexabilityRule),
        Box::new(CanonicalConsistencyRule),
        Box::new(CanonicalResolutionRule),
        Box::new(RobotsEffectiveRule),
        Box::new(HreflangReciprocalRule),
        Box::new(HreflangCanonicalRule),
        Box::new(InternalLinkTargetRule),
        Box::new(OrphanRule),
    ];

    // Run rules
    let findings = run_all_rules(&snapshot, &rules, policy.as_ref());

    // Print statistics
    let formats = if formats.is_empty() {
        vec!["terminal".to_string()]
    } else {
        formats
    };

    if formats.contains(&"terminal".to_string()) {
        print_statistics(&snapshot);
        print_findings(&findings);
    }

    // Output other formats
    for format in &formats {
        match format.as_str() {
            "json" => {
                let json = findings_to_json(&findings)?;
                write_output(&json, output_path.as_deref(), "json")?;
            }
            "markdown" => {
                let md = findings_to_markdown(&findings, &snapshot);
                write_output(&md, output_path.as_deref(), "md")?;
            }
            "sarif" => {
                let sarif = findings_to_sarif(&findings)?;
                write_output(&sarif, output_path.as_deref(), "sarif")?;
            }
            "terminal" => {} // Already printed above
            other => {
                eprintln!("Warning: unknown format '{other}', skipping");
            }
        }
    }

    // Determine exit code
    let fail = if let Some(ref fail_on) = fail_on {
        let severities: Vec<String> = fail_on.split(',').map(|s| s.trim().to_string()).collect();
        findings.iter().any(|f| {
            severities
                .iter()
                .any(|s| s.eq_ignore_ascii_case(&f.severity.to_string()))
        })
    } else {
        should_fail(&findings, policy.as_ref())
    };

    Ok(fail)
}

/// Returns Ok(true) if the process should exit with code 1.
fn cmd_diff(
    baseline_path: PathBuf,
    candidate_path: PathBuf,
    policy_path: Option<PathBuf>,
    formats: Vec<String>,
    output_path: Option<PathBuf>,
) -> anyhow::Result<bool> {
    // Load policy
    let policy = match &policy_path {
        Some(path) => Some(
            Policy::load(path)
                .with_context(|| format!("loading policy from {}", path.display()))?,
        ),
        None => None,
    };

    // Load snapshots
    let baseline_json = std::fs::read_to_string(&baseline_path)
        .with_context(|| format!("reading baseline from {}", baseline_path.display()))?;
    let candidate_json = std::fs::read_to_string(&candidate_path)
        .with_context(|| format!("reading candidate from {}", candidate_path.display()))?;

    let baseline =
        snapshot_from_json(&baseline_json).with_context(|| "parsing baseline snapshot")?;
    let candidate =
        snapshot_from_json(&candidate_json).with_context(|| "parsing candidate snapshot")?;

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
        vec!["terminal".to_string()]
    } else {
        formats
    };

    if formats.contains(&"terminal".to_string()) {
        println!("{summary}");
        println!();
        print_findings(&findings);
    }

    // Output other formats
    for format in &formats {
        match format.as_str() {
            "json" => {
                let json = findings_to_json(&findings)?;
                write_output(&json, output_path.as_deref(), "json")?;
            }
            "markdown" => {
                let md = findings_to_markdown(&findings, &candidate);
                write_output(&md, output_path.as_deref(), "md")?;
            }
            "terminal" => {} // Already printed
            other => {
                eprintln!("Warning: unknown format '{other}', skipping");
            }
        }
    }

    let fail = should_fail(&findings, policy.as_ref());
    Ok(fail)
}

fn write_output(
    content: &str,
    path: Option<&std::path::Path>,
    _default_ext: &str,
) -> anyhow::Result<()> {
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
