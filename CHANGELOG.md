# Changelog

All notable changes to this project will be documented in this file.

The project uses semantic versioning.

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
