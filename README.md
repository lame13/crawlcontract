# crawlcontract

A deterministic CI gate that proves a site's crawl, canonical and indexability
signals agree — and identifies the exact artifact that disagrees.

## What it does

crawlcontract builds a single **URL-state graph** from a site's:

- HTML pages (`<link rel="canonical">`, `<meta name="robots">`, hreflang, internal links)
- HTTP headers (`X-Robots-Tag`, `Link` header canonical)
- `robots.txt` (allow/disallow rules, sitemap references)
- XML sitemaps
- Redirect chains

It then enforces **cross-artifact invariants** — hard rules that check whether
these signals agree with each other. Every failure includes deterministic
evidence: the URL, what was declared, what was observed, and what the canonical
target is.

## Install

```bash
cargo install crawlcontract
```

Or build from source:

```bash
git clone https://github.com/lame13/crawlcontract
cd crawlcontract
cargo build --release
```

## Usage

### Scan a static dist directory

```bash
crawlcontract scan ./dist --public-origin https://example.com
```

### Save a snapshot for diffing

```bash
crawlcontract scan ./dist \
  --public-origin https://example.com \
  --snapshot candidate.json
```

### Diff two snapshots

```bash
crawlcontract diff baseline.json candidate.json --policy crawlcontract.toml
```

### Output formats

```bash
# JSON
crawlcontract scan ./dist --format json

# Markdown
crawlcontract scan ./dist --format markdown

# SARIF (for CI/CD integration)
crawlcontract scan ./dist --format sarif

# Multiple formats
crawlcontract scan ./dist --format terminal,json --output report.json
```

### CI integration

```bash
# Exit 1 on any error-level finding (default)
crawlcontract scan ./dist --public-origin https://example.com

# Exit 1 on warnings too
crawlcontract scan ./dist --fail-on error,warning

# Use a policy file for exclusions
crawlcontract scan ./dist --policy crawlcontract.toml
```

## Rules

| Rule ID | Severity | Description |
|---|---|---|
| `CC-SITEMAP-INDEXABILITY-001` | ERROR | URL in sitemap but effective directive is noindex |
| `CC-SITEMAP-INDEXABILITY-002` | ERROR | URL in sitemap but robots.txt blocks crawling |
| `CC-SITEMAP-INDEXABILITY-003` | WARN | URL in sitemap but canonicalizes elsewhere |
| `CC-CANONICAL-CONSISTENCY-001` | ERROR | HTML canonical conflicts with HTTP Link header canonical |
| `CC-CANONICAL-CONSISTENCY-002` | WARN | Canonical conflicts with sitemap presence |
| `CC-CANONICAL-CONSISTENCY-003` | ERROR | Canonical target does not resolve |
| `CC-CANONICAL-RESOLUTION-001` | ERROR | Canonical chain exceeds depth 1 |
| `CC-CANONICAL-RESOLUTION-002` | ERROR | Canonical cycle detected |
| `CC-ROBOTS-EFFECTIVE-001` | WARN | Meta robots and X-Robots-Tag conflict |
| `CC-HREFLANG-RECIPROCAL-001` | ERROR | Hreflang entry not reciprocated by target |
| `CC-HREFLANG-RECIPROCAL-002` | ERROR | Hreflang cluster missing self-reference |
| `CC-HREFLANG-CANONICAL-001` | ERROR | Hreflang URL differs from target's canonical |
| `CC-LINK-TARGET-001` | ERROR | Internal link targets a broken (non-200) URL |
| `CC-LINK-TARGET-002` | WARN | Internal link chains through redirect |
| `CC-ORPHAN-001` | WARN | Indexable sitemap URL unreachable via internal links |
| `CC-DIFF-LOSS-INDEXABLE` | ERROR | Indexable URL count decreased beyond threshold |
| `CC-DIFF-LOSS-CONTENT` | WARN | Page lost significant content |
| `CC-DIFF-LOSS-LINKS` | WARN | Internal link count decreased beyond threshold |

## Example finding

```
CC-SITEMAP-INDEXABILITY-001  ERROR

URL:       /property/example
Declared:  sitemap.xml
Observed:  X-Robots-Tag: noindex
Canonical: /property/other

The sitemap declares this URL for indexing, but the effective
HTTP indexing directive excludes it.
```

## Policy file

Create a `crawlcontract.toml` for exclusions and diff thresholds:

```toml
[general]
fail_on = ["error"]

[[exclusions]]
rule_id = "CC-ORPHAN-001"
url_pattern = "/admin/**"
reason = "Admin pages not linked from public navigation"

[diff]
max_indexable_url_loss_percent = 5
max_link_loss_percent = 10
max_heading_loss_percent = 15
```

See [`crawlcontract.toml.example`](crawlcontract.toml.example) for a complete
example.

## Design principles

- **No scores.** crawlcontract proves agreement, not quality.
- **Deterministic evidence.** Every finding includes what was declared, what
  was observed, and what the canonical target is.
- **Cross-artifact invariants.** The rules check whether different signals
  (sitemap, canonicals, robots directives) agree — not whether individual
  pages have good titles.
- **CI-native.** Exit codes, SARIF output, snapshot diffing, and policy files
  for deterministic gates.

## License

MIT OR Apache-2.0
