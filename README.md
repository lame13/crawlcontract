# crawlcontract

A deterministic CI gate that proves a site's crawl, canonical, and indexability
signals agree—and identifies the artifact that disagrees.

`crawlcontract` builds one URL-state graph from HTML, HTTP responses, redirects,
`robots.txt`, XML sitemaps, canonicals, internal links, and hreflang. It then
checks cross-artifact invariants instead of assigning an SEO score.

The project is in early development. Snapshots and reports currently use schema
`1.0` and findings use deterministic rule IDs; compatibility is not promised
before the first stable release.

## Why this exists

Most site checkers validate one artifact at a time. `crawlcontract` focuses on
contradictions between artifacts:

- A sitemap URL must return `200`, remain crawlable, avoid `noindex`, and point
  to itself canonically.
- An indexable page must be declared by a sitemap and must carry a canonical,
  rather than only the sitemap entries being checked.
- HTML and HTTP canonical signals must agree.
- Canonical targets must return `200`, remain indexable, and avoid chains or
  cycles.
- Meta robots and `X-Robots-Tag` are combined additively; a permissive
  directive cannot cancel a restrictive one.
- Internal links must resolve directly instead of failing, remaining
  unverified, or passing through redirects.
- Hreflang alternates must be reciprocal, self-inclusive, canonically aligned,
  and indexable.
- Every verified indexable page must be reachable from the scan entry point.
- Release snapshots can gate unexpected losses in indexable URLs, words,
  headings, and internal links.
- A scan can be compared against a committed baseline in one command, so a pull
  request only fails for findings it introduces.

## Install

Download a prebuilt archive for Linux x86-64, macOS Intel, macOS Apple Silicon,
or Windows x86-64 from the
[`latest` GitHub release](https://github.com/lame13/crawlcontract/releases/latest).
Each release includes a `SHA256SUMS` file.

Alternatively, install from Git:

```bash
cargo install --git https://github.com/lame13/crawlcontract
```

The release workflow supports crates.io publishing when `CARGO_REGISTRY_TOKEN`
is configured. Until then, use the release binaries or Git installation above.

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
published identity while requests still go to the preview origin. The mapping
applies consistently to sitemap entries, redirects, HTML and HTTP canonicals,
hreflang, and internal links:

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

Gate a pull request on regressions only, by comparing the scan against the
committed baseline in a single command. This adds the `CC-DIFF-LOSS-*` rules to
the scan and marks every finding with `"new": true` or `"new": false` in JSON
output:

```bash
crawlcontract scan ./dist \
  --public-origin https://example.com \
  --baseline baseline.json \
  --fail-on-new \
  --format json \
  --output candidate-report.json
```

`--fail-on-new` gates only findings the baseline scan does not already contain,
so a site with pre-existing findings can still adopt the gate today. Refresh the
baseline deliberately by scanning into the same path:

```bash
crawlcontract scan ./dist --public-origin https://example.com --snapshot baseline.json
```

Live scans accept the request settings a real crawler needs:

```bash
crawlcontract scan https://preview.example.net \
  --user-agent "crawlcontract/0.4.0 (+https://example.com/bot)" \
  --header "Authorization: Bearer $PREVIEW_TOKEN" \
  --timeout 20 \
  --crawl-delay 1 \
  --snapshot preview.json
```

Custom headers are sent only to the scanned origin, including its robots.txt
and sitemap requests. Use `--user-agent` to set the User-Agent header and
robots.txt identity together.

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

`json` output describes the whole run: the snapshot statistics, the findings,
and—when a baseline or `diff` is involved—the `diff` object with `lost_urls`,
`gained_urls`, and `changed_urls`. Consumers do not have to parse terminal text
to build a pull-request comment.

Exit codes are stable for CI:

| Code | Meaning |
|---:|---|
| `0` | Scan completed and no configured gate failed |
| `1` | One or more findings matched `fail_on` |
| `2` | Invalid input, configuration, snapshot, or scan failure |

Live scans fail closed with exit code `2` when the entry URL is blocked, does
not end at an HTML `200` response, a page request fails, the page budget is
exhausted, or a declared crawl delay exceeds 60 seconds. A failed live scan
does not write a snapshot or print “No issues found,” because its evidence is
not complete enough to support that claim.

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
| `CC-SITEMAP-COVERAGE-001` | WARN | Indexable page is missing from every sitemap |
| `CC-CANONICAL-CONSISTENCY-001` | ERROR | HTML and HTTP canonicals disagree |
| `CC-CANONICAL-CONSISTENCY-003` | ERROR | Canonical target does not return `200` or was not verified |
| `CC-CANONICAL-CONSISTENCY-004` | ERROR | Canonical target returns `200` but is non-indexable |
| `CC-CANONICAL-PRESENCE-001` | WARN | Indexable HTML page declares no canonical at all |
| `CC-CANONICAL-RESOLUTION-001` | ERROR | Canonical chain exceeds one hop |
| `CC-CANONICAL-RESOLUTION-002` | ERROR | Canonical cycle detected |
| `CC-ROBOTS-EFFECTIVE-001` | WARN | Meta robots and `X-Robots-Tag` conflict |
| `CC-HREFLANG-RECIPROCAL-001` | ERROR | Hreflang entry is not reciprocated |
| `CC-HREFLANG-RECIPROCAL-002` | ERROR | Hreflang cluster lacks a self-reference |
| `CC-HREFLANG-CANONICAL-001` | ERROR | Hreflang target canonicalizes elsewhere |
| `CC-HREFLANG-INDEXABILITY-001` | ERROR | Hreflang target exists but cannot be indexed |
| `CC-LINK-TARGET-001` | ERROR | Internal link returns non-`200` without a redirect target |
| `CC-LINK-TARGET-002` | WARN | Internal link passes through a redirect |
| `CC-LINK-TARGET-003` | ERROR | Internal link target was not verified |
| `CC-LINK-TARGET-004` | WARN | Internal link target canonicalizes elsewhere |
| `CC-REDIRECT-RESOLUTION-001` | ERROR | Redirect chain contains a cycle |
| `CC-ORPHAN-001` | WARN | Verified indexable page is unreachable internally |
| `CC-DIFF-LOSS-INDEXABLE` | ERROR | Indexable URL loss exceeds policy |
| `CC-DIFF-LOSS-CONTENT` | WARN | Word or heading loss exceeds policy |
| `CC-DIFF-LOSS-LINKS` | WARN | Internal-link loss exceeds policy |

`CC-SITEMAP-COVERAGE-001` is the inverse of the sitemap indexability family: it
fires when an indexable HTML page is absent from every declared sitemap. A scan
that found no sitemap data at all is out of scope, so a site without a sitemap
is not flooded with warnings. Both it and `CC-CANONICAL-PRESENCE-001` only
consider responses that were parsed as HTML, so images, stylesheets, and other
non-HTML `200` responses are never reported as pages.

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

Policy `public_origin` takes precedence over the CLI value. Policy keys are
strict: unknown fields, empty `fail_on`, unknown exclusion rule IDs, blank
exclusion reasons, and malformed exclusion patterns are rejected. Thresholds
must be between `0` and `100`. Exclusions require both the rule ID and either
the full URL or path/query glob to match. See
[`crawlcontract.toml.example`](crawlcontract.toml.example).

## Current boundaries

- Live scans inspect response HTML; they do not execute JavaScript or compare a
  browser-rendered DOM.
- `robots.txt` evaluation selects the group that matches the configured
  `--user-agent` (longest matching token wins, `*` as fallback) and supports
  `Allow`, `Disallow`, `*`, `$`, query matching, allow-wins ties, and per-group
  `Crawl-delay`. Rules for other named crawlers are not merged in.
- Live sitemap discovery follows one sitemap-index level and does not yet read
  compressed `.xml.gz` files.
- Live scans do not retry transient failures. A page connection failure or an
  unsuccessful entry response aborts the scan; other `5xx` pages are recorded
  as broken targets.
- Cross-origin hreflang and canonical targets are recorded, but an offline
  static scan cannot fetch them and therefore cannot prove they resolve.
- JSON-LD-to-visible-content parity is intentionally out of scope until it can
  be driven by explicit selectors or adapters.

## Roadmap

1. Harden the core contract: recursive and compressed sitemap support, per-hop
   redirect evidence and redirect-chain rules, and broader HTTP
   `Link`/`X-Robots-Tag` parsing.
2. Stabilize CI artifacts: snapshot migrations, per-rule severity overrides,
   golden report fixtures, and a `crawlcontract rules --list` command driven by
   the rule catalog.
3. Strengthen release comparison: explicit URL-loss allowlists, structural
   content fingerprints, and section-level reachability changes.
4. Add retry and backoff for transient live-scan failures, plus build provenance
   for release binaries.
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
