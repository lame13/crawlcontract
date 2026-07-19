# crawlcontract

A deterministic CI gate that proves a site's crawl, canonical, and indexability
signals agree—and identifies the artifact that disagrees.

`crawlcontract` builds one URL-state graph from HTML, HTTP responses, redirects,
`robots.txt`, XML sitemaps, canonicals, internal links, and hreflang. It then
checks cross-artifact invariants instead of assigning an SEO score.

The project is in early development. Snapshots currently use schema `1.0` and
findings use deterministic rule IDs; compatibility is not promised before the
first stable release.

## Why this exists

Most site checkers validate one artifact at a time. `crawlcontract` focuses on
contradictions between artifacts:

- A sitemap URL must return `200`, remain crawlable, avoid `noindex`, and point
  to itself canonically.
- HTML and HTTP canonical signals must agree.
- Canonical targets must return `200`, remain indexable, and avoid chains or
  cycles.
- Meta robots and `X-Robots-Tag` are combined additively; a permissive
  directive cannot cancel a restrictive one.
- Internal links must resolve directly instead of failing, remaining
  unverified, or passing through redirects.
- Hreflang alternates must be reciprocal, self-inclusive, and canonically
  aligned.
- Every verified indexable page must be reachable from the scan entry point.
- Release snapshots can gate unexpected losses in indexable URLs, words,
  headings, and internal links.

## Install

Download a prebuilt archive for Linux x86-64, macOS Intel, macOS Apple Silicon,
or Windows x86-64 from the
[`latest` GitHub release](https://github.com/lame13/crawlcontract/releases/latest).
Each release includes a `SHA256SUMS` file.

Alternatively, install the current Git version:

```bash
cargo install --git https://github.com/lame13/crawlcontract
```

Or build a local checkout with Rust 1.85 or newer:

```bash
git clone https://github.com/lame13/crawlcontract
cd crawlcontract
cargo build --release
```

## Usage

Scan static output:

```bash
crawlcontract scan ./dist \
  --public-origin https://example.com \
  --snapshot candidate.json
```

Scan a live or preview site. `--public-origin` maps preview URLs onto their
published identity while requests still go to the preview origin:

```bash
crawlcontract scan https://preview.example.net \
  --public-origin https://example.com \
  --max-pages 500 \
  --concurrency 8 \
  --snapshot candidate.json
```

Compare release snapshots:

```bash
crawlcontract diff baseline.json candidate.json \
  --policy crawlcontract.toml
```

Write a machine-readable report:

```bash
crawlcontract scan ./dist \
  --public-origin https://example.com \
  --format sarif \
  --output crawlcontract.sarif
```

Supported report formats are `terminal`, `json`, `markdown`, and `sarif`.
Select at most one non-terminal format; terminal output may be combined with
it, for example `--format terminal,json`.

Exit codes are stable for CI:

| Code | Meaning |
|---:|---|
| `0` | Scan completed and no configured gate failed |
| `1` | One or more findings matched `fail_on` |
| `2` | Invalid input, configuration, snapshot, or scan failure |

By default only errors fail. To fail on warnings too:

```bash
crawlcontract scan ./dist \
  --public-origin https://example.com \
  --fail-on error,warning
```

## Scan modes

| Signal | Static directory | Live URL |
|---|:---:|:---:|
| HTML canonical, meta robots, links, hreflang | Yes | Yes |
| Word and heading counts | Yes | Yes |
| `robots.txt` wildcard rules and sitemap declarations | Yes | Yes |
| XML sitemap and sitemap-index entries | Yes | Yes |
| HTTP status and headers | Inferred from files | Observed |
| Redirect chains | No | Yes |
| `X-Robots-Tag` and HTTP `Link` canonical | No | Yes |

Static routes use common output conventions: `index.html` maps to `/`, nested
`index.html` maps to a trailing-slash route, and other `.html`/`.htm` files map
to extensionless routes. A same-origin target without a matching HTML file is
recorded as `404`.

## Rules

| Rule ID | Severity | Contract |
|---|---|---|
| `CC-SITEMAP-INDEXABILITY-001` | ERROR | Sitemap URL has an effective `noindex` |
| `CC-SITEMAP-INDEXABILITY-002` | ERROR | Sitemap URL is blocked by `robots.txt` |
| `CC-SITEMAP-INDEXABILITY-003` | WARN | Sitemap URL canonicalizes elsewhere |
| `CC-SITEMAP-INDEXABILITY-004` | ERROR | Sitemap URL does not return `200` or was not verified |
| `CC-CANONICAL-CONSISTENCY-001` | ERROR | HTML and HTTP canonicals disagree |
| `CC-CANONICAL-CONSISTENCY-003` | ERROR | Canonical target does not return `200` or was not verified |
| `CC-CANONICAL-CONSISTENCY-004` | ERROR | Canonical target returns `200` but is non-indexable |
| `CC-CANONICAL-RESOLUTION-001` | ERROR | Canonical chain exceeds one hop |
| `CC-CANONICAL-RESOLUTION-002` | ERROR | Canonical cycle detected |
| `CC-ROBOTS-EFFECTIVE-001` | WARN | Meta robots and `X-Robots-Tag` conflict |
| `CC-HREFLANG-RECIPROCAL-001` | ERROR | Hreflang entry is not reciprocated |
| `CC-HREFLANG-RECIPROCAL-002` | ERROR | Hreflang cluster lacks a self-reference |
| `CC-HREFLANG-CANONICAL-001` | ERROR | Hreflang target canonicalizes elsewhere |
| `CC-LINK-TARGET-001` | ERROR | Internal link returns non-`200` without a redirect target |
| `CC-LINK-TARGET-002` | WARN | Internal link passes through a redirect |
| `CC-LINK-TARGET-003` | ERROR | Internal link target was not verified |
| `CC-LINK-TARGET-004` | WARN | Internal link target canonicalizes elsewhere |
| `CC-REDIRECT-RESOLUTION-001` | ERROR | Redirect chain contains a cycle |
| `CC-ORPHAN-001` | WARN | Verified indexable page is unreachable internally |
| `CC-DIFF-LOSS-INDEXABLE` | ERROR | Indexable URL loss exceeds policy |
| `CC-DIFF-LOSS-CONTENT` | WARN | Word or heading loss exceeds policy |
| `CC-DIFF-LOSS-LINKS` | WARN | Internal-link loss exceeds policy |

Every finding contains a stable rule ID, URL, message, severity, and structured
evidence. Example:

```text
CC-SITEMAP-INDEXABILITY-001  ERROR

URL:       https://example.com/property/example
Declared:  sitemap.xml
Observed:  noindex
Canonical: https://example.com/property/other

The sitemap declares this URL for indexing, but the effective
indexing directive excludes it.
```

## Policy

Create `crawlcontract.toml` to configure gate severities, exclusions, public
origin, and diff thresholds:

```toml
[general]
fail_on = ["error"]
public_origin = "https://example.com"

[[exclusions]]
rule_id = "CC-ORPHAN-001"
url_pattern = "https://example.com/admin/**"
reason = "Admin pages are intentionally absent from public navigation"

[diff]
max_indexable_url_loss_percent = 5
max_link_loss_percent = 10
max_word_loss_percent = 50
max_heading_loss_percent = 15
```

Policy `public_origin` takes precedence over the CLI value. Thresholds must be
between `0` and `100`. Exclusions require both the rule ID and either the full
URL or path/query glob to match. See
[`crawlcontract.toml.example`](crawlcontract.toml.example).

## Current boundaries

- Live scans inspect response HTML; they do not execute JavaScript or compare a
  browser-rendered DOM.
- `robots.txt` evaluation currently models the `User-agent: *` group with
  `Allow`, `Disallow`, `*`, `$`, query matching, and allow-wins ties.
- Live sitemap discovery follows one sitemap-index level and does not yet read
  compressed `.xml.gz` files.
- Cross-origin hreflang and canonical targets are recorded, but an offline
  static scan cannot fetch them and therefore cannot prove they resolve.
- JSON-LD-to-visible-content parity is intentionally out of scope until it can
  be driven by explicit selectors or adapters.

## Roadmap

1. Harden the core contract: recursive and compressed sitemap support,
   configurable robots user agents, per-hop redirect evidence, and broader HTTP
   `Link`/`X-Robots-Tag` parsing.
2. Stabilize CI artifacts: rule metadata, SARIF source locations, snapshot
   migrations, severity overrides, and golden report fixtures.
3. Strengthen release comparison: explicit URL-loss allowlists, structural
   content fingerprints, and section-level reachability changes.
4. Add build provenance for release binaries, then publish the crate after
   package and MSRV verification.
5. Add opt-in semantic adapters for visible breadcrumb, list, and structured
   data parity without turning the project into a generic SEO scorecard.

## Development

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## License

Licensed under either Apache License 2.0 or the MIT license, at your option.
