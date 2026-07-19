use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::Context;
use url::Url;

use crate::model::snapshot::{utc_now, Snapshot, Statistics};
use crate::model::url_state::{RobotsTxtStatus, UrlSource, UrlState};
use crate::scanner::html_signals::{normalize_url_key, process_html_files};
use crate::signals::sitemap::{collect_sitemap_urls, parse_sitemap, Sitemap};

/// Result of scanning a static directory.
pub struct StaticScanResult {
    pub html_files: Vec<(Url, String)>,
    pub robots_txt_body: Option<String>,
    pub sitemap_body: Option<String>,
    pub base_url: Url,
}

/// Scan a static directory, discover HTML files, robots.txt, and sitemap.xml.
pub fn discover_files(dist_path: &Path, base_url: &Url) -> anyhow::Result<StaticScanResult> {
    let mut html_files = Vec::new();
    let mut robots_txt_body = None;
    let mut sitemap_body = None;

    let entries = walk_dir(dist_path)?;

    for entry_path in entries {
        let relative = entry_path.strip_prefix(dist_path).unwrap_or(&entry_path);

        let relative_str = relative.to_string_lossy();

        // Check for robots.txt
        if relative_str == "robots.txt" {
            robots_txt_body = Some(
                std::fs::read_to_string(&entry_path)
                    .with_context(|| format!("reading {}", entry_path.display()))?,
            );
            continue;
        }

        // Check for sitemap.xml (and variants)
        if relative_str == "sitemap.xml"
            || relative_str.ends_with("/sitemap.xml")
            || relative_str.starts_with("sitemap-")
            || relative_str.ends_with("-sitemap.xml")
        {
            sitemap_body = Some(
                std::fs::read_to_string(&entry_path)
                    .with_context(|| format!("reading {}", entry_path.display()))?,
            );
            continue;
        }

        // Process HTML files
        if is_html_file(&entry_path) {
            let url = file_path_to_url(&entry_path, dist_path, base_url)?;
            let content = std::fs::read_to_string(&entry_path)
                .with_context(|| format!("reading {}", entry_path.display()))?;
            html_files.push((url, content));
        }
    }

    Ok(StaticScanResult {
        html_files,
        robots_txt_body,
        sitemap_body,
        base_url: base_url.clone(),
    })
}

/// Build a complete scan result into a Snapshot.
pub fn build_snapshot(result: StaticScanResult, public_origin: Option<Url>) -> Snapshot {
    let base_url = result.base_url.clone();
    let mut states: BTreeMap<String, UrlState> = BTreeMap::new();

    // 1. Process robots.txt
    let robots_txt = result
        .robots_txt_body
        .as_deref()
        .map(|body| crate::signals::robots_txt::RobotsTxt::parse(body, &base_url));

    // 2. Process sitemap
    let sitemap_urls = if let Some(body) = &result.sitemap_body {
        match parse_sitemap(body) {
            Ok(sitemap) => {
                let urls = collect_sitemap_urls(&sitemap);
                // If it's an index, we can't resolve child sitemaps in static mode,
                // so we record what we have.
                if let Sitemap::Index(idx) = &sitemap {
                    for child_url in &idx.sitemaps {
                        let key = normalize_url_key(child_url);
                        // Mark child sitemaps as discovered but note they're indexes
                        states.entry(key).or_insert_with(|| {
                            let mut s = UrlState::new(child_url.clone());
                            s.add_source(UrlSource::Sitemap);
                            s
                        });
                    }
                }
                urls
            }
            Err(_) => BTreeSet::new(),
        }
    } else {
        BTreeSet::new()
    };

    // 3. Mark sitemap URLs in states
    for url in &sitemap_urls {
        let key = normalize_url_key(url);
        let state = states.entry(key).or_insert_with(|| {
            let mut s = UrlState::new(url.clone());
            s.add_source(UrlSource::Sitemap);
            s
        });
        state.found_in_sitemap = true;
    }

    // 4. Process HTML files and extract signals
    process_html_files(&result.html_files, &mut states);

    // 5. Apply robots.txt status
    for state in states.values_mut() {
        if let Some(rt) = &robots_txt {
            state.robots_txt_status = rt.is_allowed(state.url.path());
        }
    }

    // 6. Apply robots.txt sitemap URLs
    if let Some(rt) = &robots_txt {
        for sitemap_url in &rt.sitemaps {
            let key = normalize_url_key(sitemap_url);
            states.entry(key).or_insert_with(|| {
                let mut s = UrlState::new(sitemap_url.clone());
                s.add_source(UrlSource::RobotsTxt);
                s
            });
        }
    }

    // 7. Compute derived state (effective directives, indexability)
    compute_derived_state(&mut states);

    // 8. Compute reachability
    compute_reachability(&mut states, &base_url);

    // 9. Build snapshot
    let statistics = Statistics::from_url_states(&states);
    Snapshot {
        version: "1.0".to_string(),
        tool: "crawlcontract".to_string(),
        base_url,
        public_origin,
        scanned_at: utc_now(),
        urls: states,
        statistics,
    }
}

/// Compute effective canonical, effective robots, and indexability for all URLs.
fn compute_derived_state(states: &mut BTreeMap<String, UrlState>) {
    // First pass: compute effective canonical
    let canonical_map: BTreeMap<String, Option<Url>> = states
        .iter()
        .map(|(key, state)| {
            let effective = resolve_effective_canonical(state, states);
            (key.clone(), effective)
        })
        .collect();

    for (key, canonical) in &canonical_map {
        if let Some(state) = states.get_mut(key) {
            state.effective_canonical = canonical.clone();
        }
    }

    // Second pass: compute effective robots and indexability
    let keys: Vec<String> = states.keys().cloned().collect();
    for key in &keys {
        let state = states.get(key).unwrap();

        let effective_robots = crate::signals::robots_directive::effective_robots_directive(
            state.html_meta_robots.as_ref(),
            state.http_x_robots_tag.as_ref(),
        );

        // A page is indexable when:
        // - No noindex directive
        // - Not blocked by robots.txt (blocked pages can still be indexed
        //   but without content, which is generally undesirable)
        // - HTTP status is 2xx (or unknown for static)
        // - Does not canonicalize elsewhere
        let no_noindex = !effective_robots.is_noindex();
        let not_blocked = state.robots_txt_status != RobotsTxtStatus::Blocked;
        let ok_status = state.http_status.map(|s| s < 400).unwrap_or(true); // static files assume 200
        let is_canonical = state
            .effective_canonical
            .as_ref()
            .map(|c| c == &state.url)
            .unwrap_or(true); // no canonical = self-canonical

        let is_indexable = no_noindex && not_blocked && ok_status && is_canonical;

        let state = states.get_mut(key).unwrap();
        state.effective_robots = effective_robots;
        state.is_indexable = is_indexable;
    }
}

/// Resolve the effective canonical URL for a state.
/// Follows the chain: html_canonical → http_canonical → self.
fn resolve_effective_canonical(
    state: &UrlState,
    _states: &BTreeMap<String, UrlState>,
) -> Option<Url> {
    // Prefer HTML canonical, then HTTP canonical
    let canonical = state
        .html_canonical
        .as_ref()
        .or(state.http_canonical.as_ref());

    canonical.cloned()
}

/// Compute reachability via BFS from entry points.
/// Entry points: homepage, any URL found in both sitemap and internal links.
fn compute_reachability(states: &mut BTreeMap<String, UrlState>, base_url: &Url) {
    use std::collections::{HashSet, VecDeque};

    // Build adjacency list from internal links
    let adjacency: BTreeMap<String, Vec<String>> = states
        .iter()
        .map(|(key, state)| {
            let targets: Vec<String> = state
                .internal_links_out
                .iter()
                .map(normalize_url_key)
                .collect();
            (key.clone(), targets)
        })
        .collect();

    // Entry points: homepage + any URL found via internal links
    let mut queue = VecDeque::new();
    let mut visited = HashSet::new();

    // Homepage is always an entry point
    let homepage_key = normalize_url_key(base_url);
    if states.contains_key(&homepage_key) {
        queue.push_back(homepage_key.clone());
        visited.insert(homepage_key);
    }

    // Also seed from any URL that was discovered via DirectScan (i.e., found as an HTML file)
    for (key, state) in states.iter() {
        if state.sources.contains(&UrlSource::DirectScan) && !visited.contains(key) {
            queue.push_back(key.clone());
            visited.insert(key.clone());
        }
    }

    // BFS
    while let Some(current) = queue.pop_front() {
        if let Some(neighbors) = adjacency.get(&current) {
            for neighbor in neighbors {
                if visited.insert(neighbor.clone()) {
                    queue.push_back(neighbor.clone());
                }
            }
        }
    }

    // Mark reachable URLs
    for key in &visited {
        if let Some(state) = states.get_mut(key) {
            state.is_reachable = true;
        }
    }
}

/// Recursively walk a directory, returning all file paths.
fn walk_dir(dir: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut result = Vec::new();
    walk_dir_inner(dir, &mut result)?;
    Ok(result)
}

fn walk_dir_inner(dir: &Path, result: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in
        std::fs::read_dir(dir).with_context(|| format!("reading directory {}", dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            walk_dir_inner(&path, result)?;
        } else {
            result.push(path);
        }
    }
    Ok(())
}

/// Check if a file path is an HTML file.
fn is_html_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("html") | Some("htm")
    )
}

/// Convert a file system path to a URL relative to the dist root.
fn file_path_to_url(file_path: &Path, dist_root: &Path, base_url: &Url) -> anyhow::Result<Url> {
    let relative = file_path.strip_prefix(dist_root).with_context(|| {
        format!(
            "{} is not under {}",
            file_path.display(),
            dist_root.display()
        )
    })?;

    let relative_str = relative.to_string_lossy();

    // Convert index.html to directory path, other .html files strip extension
    let url_path = if relative_str == "index.html" || relative_str == "index.htm" {
        "/".to_string()
    } else if relative_str.ends_with("/index.html") || relative_str.ends_with("/index.htm") {
        format!("/{}", &relative_str[..relative_str.len() - 10])
    } else if let Some(stripped) = relative_str.strip_suffix(".html") {
        format!("/{stripped}")
    } else if let Some(stripped) = relative_str.strip_suffix(".htm") {
        format!("/{stripped}")
    } else {
        format!("/{}", relative_str)
    };

    base_url
        .join(&url_path)
        .with_context(|| format!("joining {base_url} with {url_path}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn base() -> Url {
        Url::parse("https://example.test").unwrap()
    }

    #[test]
    fn is_html_file_recognizes_extensions() {
        assert!(is_html_file(Path::new("page.html")));
        assert!(is_html_file(Path::new("page.htm")));
        assert!(!is_html_file(Path::new("page.txt")));
        assert!(!is_html_file(Path::new("robots.txt")));
    }

    #[test]
    fn file_path_to_url_converts_index() {
        let url =
            file_path_to_url(Path::new("/dist/index.html"), Path::new("/dist"), &base()).unwrap();
        assert_eq!(url.path(), "/");
    }

    #[test]
    fn file_path_to_url_converts_page() {
        let url =
            file_path_to_url(Path::new("/dist/about.html"), Path::new("/dist"), &base()).unwrap();
        assert_eq!(url.path(), "/about");
    }

    #[test]
    fn file_path_to_url_nested_index() {
        let url = file_path_to_url(
            Path::new("/dist/blog/index.html"),
            Path::new("/dist"),
            &base(),
        )
        .unwrap();
        assert_eq!(url.path(), "/blog/");
    }

    #[test]
    fn discover_files_finds_html() {
        let tmp = tempfile::tempdir().unwrap();
        let dist = tmp.path().join("dist");
        fs::create_dir_all(&dist).unwrap();
        fs::write(dist.join("index.html"), "<html><body>Home</body></html>").unwrap();
        fs::write(dist.join("robots.txt"), "User-agent: *\nDisallow:").unwrap();

        let result = discover_files(&dist, &base()).unwrap();
        assert_eq!(result.html_files.len(), 1);
        assert!(result.robots_txt_body.is_some());
    }

    #[test]
    fn build_snapshot_basic() {
        let tmp = tempfile::tempdir().unwrap();
        let dist = tmp.path().join("dist");
        fs::create_dir_all(&dist).unwrap();
        fs::write(
            dist.join("index.html"),
            r#"<html><head><link rel="canonical" href="/"></head><body>
                <a href="/about">About</a>
                <h1>Home</h1>
                <p>Welcome to the site</p>
            </body></html>"#,
        )
        .unwrap();
        fs::write(
            dist.join("about.html"),
            r#"<html><head><link rel="canonical" href="/about"></head><body>
                <a href="/">Home</a>
                <h1>About</h1>
                <p>About this site</p>
            </body></html>"#,
        )
        .unwrap();

        let result = discover_files(&dist, &base()).unwrap();
        let snapshot = build_snapshot(result, None);

        assert_eq!(snapshot.statistics.total_urls, 2);
        assert_eq!(snapshot.statistics.indexable_urls, 2);
        assert_eq!(snapshot.statistics.orphan_urls, 0);
    }
}
