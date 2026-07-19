# Changelog

All notable changes to this project will be documented in this file.

The project uses semantic versioning.

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
