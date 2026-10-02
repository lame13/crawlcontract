# Changelog

All notable changes to this project will be documented in this file.

The project uses semantic versioning.

## [0.4.0] - 2026-10-02

This release adds baseline comparisons to scans, detailed JSON diff reports,
and new canonical, sitemap coverage, and hreflang indexability checks.

### Added

- Baseline-native scans. `scan --baseline SNAPSHOT` runs the `CC-DIFF-LOSS-*`
  rules alongside the normal rule set and reports lost, gained, and changed
  URLs in terminal, JSON, and Markdown reports.
- `scan --fail-on-new`, which exits `1` only for findings the baseline scan does
  not already contain. Findings are computed by running the same rule set
  against the baseline, so "new" means new since that baseline rather than new
  since the installed tool version.
- JSON diff payloads. `json` output now includes a `diff` object with
  `lost_urls`, `gained_urls`, `changed_urls`, per-URL change details,
  `counts`, `indexable_urls` (baseline, candidate, delta), and baseline
  provenance. Previously `diff --format json` emitted only findings, so the URL
  lists never reached a machine-readable consumer.
- Findings are marked `"new": true` or `"new": false` in JSON output from
  `scan --baseline`.
- `CC-SITEMAP-COVERAGE-001`: an indexable, verified HTML page is missing from
  every sitemap. This is the inverse of the sitemap indexability family, which
  only checked declared URLs. Scans with no sitemap data are out of scope.
- `CC-CANONICAL-PRESENCE-001`: an indexable page declares no canonical in HTML
  or in the HTTP `Link` header. The existing canonical rules only compared
  canonicals that existed. Both this rule and `CC-SITEMAP-COVERAGE-001` only
  consider responses that were parsed as HTML, so a `200` response for an
  image, stylesheet, or font is never reported as a page without a canonical.
- `CC-HREFLANG-INDEXABILITY-001`: an hreflang target returns a non-`200` status,
  is blocked by `robots.txt`, or carries `noindex`. Robots-blocked targets are
  reported even when they cannot be fetched. Canonical mismatches stay with
  `CC-HREFLANG-CANONICAL-001`.
- Live scan flags: `--user-agent`, `--header NAME: VALUE` (repeatable),
  `--timeout`, and `--crawl-delay`. Custom headers are restricted to the crawl
  origin; use `--user-agent` to set the crawler identity consistently.
- A rule catalog in `rules::registry` as the single source of truth for policy
  validation and SARIF rule metadata.
- `diff --fail-on` for parity with `scan`.
- A baseline comparison section in Markdown reports.
- crates.io metadata plus a `crates-io` release job that publishes on version
  tags when `CARGO_REGISTRY_TOKEN` is configured.
- CI now runs `cargo audit` on pushes and on a weekly schedule, verifies that
  `VERSION` matches `Cargo.toml`, runs `cargo package --locked`, and tests on
  macOS in addition to Linux and Windows.

### Changed

- `robots.txt` groups are selected by the configured user-agent: the longest
  `User-agent` token contained in the crawler's user-agent string wins, with
  `*` as the fallback. `Crawl-delay` is read from the matched group.
- Static scans evaluate `robots.txt` for the configured user agent instead of
  the wildcard group only.
- SARIF results carry catalog descriptions, a `helpUri`, a `detail` property,
  and `partialFingerprints` so consumers can track one finding across runs.
- Library API: `RobotsTxt::parse` and `static_dir::build_snapshot` take a
  user-agent argument; `findings_to_markdown` takes an optional diff summary;
  `LiveScanConfig` includes a `headers` field.

### Fixed

- Baseline comparisons and SARIF fingerprints distinguish separate declared
  targets on the same page, so a pre-existing finding cannot mask a new target
  failure with the same rule ID.
- Named `User-agent` groups were parsed and then discarded, so a site that
  restricted a specific crawler by name or listed `crawlcontract` explicitly
  was evaluated against the wrong rules.
- `Crawl-delay` was ignored unless it appeared in the `User-agent: *` group.
- Policy exclusions now validate against the rule catalog rather than a
  hand-maintained list of rule IDs, so a newly added rule cannot silently
  become unexcludable.

## [0.3.1] - 2026-07-19

### Fixed

- Fail live scans instead of reporting success when the entry response is not
  successful HTML, a request fails, robots.txt blocks the entry point, or the
  page budget is exhausted.
- Map preview-origin sitemap entries, HTML and HTTP canonicals, hreflang, and
  internal links to one consistent public identity.
- Build static routes and recognize sitemap files from native path components
  on Windows as well as Unix.
- Reject unknown policy fields, empty failure gates, invalid exclusions, and
  unsupported rule IDs instead of silently accepting ineffective policy.
- Reject crawl delays above 60 seconds without panicking, and wait only between
  page requests rather than before the first page.

## [0.3.0] - 2026-07-19

### Added

- Explicit failures for unresolved sitemap, canonical, and internal-link
  targets and redirect cycles.
- Standards-aware XML sitemap parsing, multiple static sitemap files, preview
  origin mapping, and bounded concurrent live fetching.
- Policy validation and a dedicated word-loss threshold.
- Tag-triggered Linux, macOS, and Windows release archives with SHA-256
  checksums.

### Fixed

- Preserve all URL discovery sources and stop treating sitemap files as page
  URLs.
- Compute reachability from the real entry point so orphan pages are not hidden.
- Combine robots directives restrictively and support wildcard/end-anchored
  `robots.txt` rules with allow-wins ties.
- Record real scan timestamps and report the package version consistently.
- Reject invalid output selections, snapshot schemas, stale snapshot statistics,
  and incompatible snapshot base URLs.

### Changed

- Parse output formats and failure severities as validated CLI values.
- Retire duplicate `CC-CANONICAL-CONSISTENCY-002`; sitemap/canonical mismatch is
  reported once as `CC-SITEMAP-INDEXABILITY-003`.

## [0.2.2] - 2026-07-19

### Fixed

- Applied `cargo fmt` formatting to source files that were inconsistent with
  the project's default rustfmt style.

## [0.2.1] - 2026-07-19

### Added

- GitHub Actions CI workflow for check, test, clippy, and format.
- CLI smoke tests covering `scan`, `diff`, output formats, snapshot file
  writing, policy application, and error handling.

## [0.2.0] - 2026-07-19

### Added

- Live site scanning via HTTP with `crawlcontract scan https://example.com`.
  Supports bounded concurrency, configurable `--max-pages`, redirect chain
  tracking, `robots.txt` crawl-delay respect, and sitemap discovery from
  `robots.txt` sitemap directives.
- `--max-pages` and `--concurrency` CLI flags for the `scan` subcommand.
- Async runtime (tokio) for both static and live scanning paths.

## [0.1.0] - 2026-07-19

### Added

- Initial release of crawlcontract.
- Static directory scanner that walks `./dist` and extracts HTML signals.
- Signal extractors for `<link rel="canonical">`, `<meta name="robots">`,
  `<link rel="hreflang">`, internal links, and redirect targets.
- `robots.txt` parser with allow/disallow evaluation.
- XML sitemap and sitemap-index parser.
- URL-state graph builder that composes per-URL effective directives.
- Reachability analysis from entry points via internal link graph.
- Cross-artifact invariant rules engine with 18 deterministic rule IDs
  covering sitemap indexability, canonical consistency and resolution,
  effective robots directives, hreflang reciprocal alignment, internal link
  targets, and orphan detection.
- Snapshot diff engine that compares baseline and candidate scans,
  detecting unexpected losses in indexable URLs, links, and content.
- Output formatters for terminal (colored), JSON, Markdown, and SARIF 2.1.0.
- TOML policy file for exclusions and diff thresholds.
- CLI with `scan` and `diff` subcommands and CI-friendly exit codes.
- Neutral `example.test` fixtures covering all rule scenarios.
