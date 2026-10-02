use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::Context;
use url::Url;

use crate::graph::reachability::compute_reachability;
use crate::graph::state::compute_derived_state;
use crate::model::snapshot::{utc_now, Snapshot, Statistics};
use crate::model::url_state::{UrlSource, UrlState};
use crate::scanner::html_signals::{normalize_url_key, process_html_files};
use crate::signals::sitemap::{collect_sitemap_urls, parse_sitemap};

/// Result of scanning a static directory.
pub struct StaticScanResult {
    pub html_files: Vec<(Url, String)>,
    pub robots_txt_body: Option<String>,
    pub sitemap_files: Vec<(PathBuf, String)>,
    pub base_url: Url,
}

/// Scan a static directory, discover HTML files, robots.txt, and sitemap.xml.
pub fn discover_files(dist_path: &Path, base_url: &Url) -> anyhow::Result<StaticScanResult> {
    let mut html_files = Vec::new();
    let mut robots_txt_body = None;
    let mut sitemap_files = Vec::new();

    let entries = walk_dir(dist_path)?;

    for entry_path in entries {
        let relative = entry_path.strip_prefix(dist_path).unwrap_or(&entry_path);

        // Check for robots.txt
        if relative == Path::new("robots.txt") {
            robots_txt_body = Some(
                std::fs::read_to_string(&entry_path)
                    .with_context(|| format!("reading {}", entry_path.display()))?,
            );
            continue;
        }

        // Check for sitemap.xml (and variants)
        if is_sitemap_file(relative) {
            sitemap_files.push((
                relative.to_path_buf(),
                std::fs::read_to_string(&entry_path)
                    .with_context(|| format!("reading {}", entry_path.display()))?,
            ));
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
        sitemap_files,
        base_url: base_url.clone(),
    })
}

/// Build a complete scan result into a Snapshot.
pub fn build_snapshot(
    result: StaticScanResult,
    public_origin: Option<Url>,
    user_agent: &str,
) -> anyhow::Result<Snapshot> {
    let base_url = result.base_url.clone();
    let mut states: BTreeMap<String, UrlState> = BTreeMap::new();

    // 1. Process robots.txt
    let robots_txt = result
        .robots_txt_body
        .as_deref()
        .map(|body| crate::signals::robots_txt::RobotsTxt::parse(body, &base_url, user_agent));

    // 2. Process sitemap
    let mut sitemap_urls = BTreeSet::new();
    for (path, body) in &result.sitemap_files {
        let sitemap =
            parse_sitemap(body).with_context(|| format!("parsing sitemap {}", path.display()))?;
        sitemap_urls.extend(collect_sitemap_urls(&sitemap));
    }

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
            state.robots_txt_status = rt.is_allowed(path_and_query(&state.url));
        }
    }

    // 6. In static mode, discovered page targets without a matching HTML file do not resolve.
    for state in states.values_mut() {
        if state.http_status.is_none()
            && state.url.origin() == base_url.origin()
            && (state.found_in_sitemap
                || state.sources.contains(&UrlSource::InternalLink)
                || state.sources.contains(&UrlSource::Canonical))
        {
            state.http_status = Some(404);
        }
    }

    // 7. Compute derived state (effective directives, indexability)
    compute_derived_state(&mut states);

    // 8. Compute reachability
    compute_reachability(&mut states, &base_url);

    // 9. Build snapshot
    let statistics = Statistics::from_url_states(&states);
    Ok(Snapshot {
        version: "1.0".to_string(),
        tool: "crawlcontract".to_string(),
        base_url,
        public_origin,
        scanned_at: utc_now(),
        urls: states,
        statistics,
    })
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
    let mut entries = std::fs::read_dir(dir)
        .with_context(|| format!("reading directory {}", dir.display()))?
        .collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            walk_dir_inner(&path, result)?;
        } else if file_type.is_file() {
            result.push(path);
        }
    }
    Ok(())
}

fn path_and_query(url: &Url) -> &str {
    &url[url::Position::BeforePath..url::Position::AfterQuery]
}

/// Check if a file path is an HTML file.
fn is_html_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("html") | Some("htm")
    )
}

fn is_sitemap_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            name == "sitemap.xml" || name.starts_with("sitemap-") || name.ends_with("-sitemap.xml")
        })
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

    let mut parts = relative
        .components()
        .map(|component| {
            component
                .as_os_str()
                .to_str()
                .map(str::to_owned)
                .with_context(|| format!("path is not valid UTF-8: {}", relative.display()))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let file_name = parts
        .pop()
        .with_context(|| format!("path has no file name: {}", relative.display()))?;

    let is_index = matches!(file_name.as_str(), "index.html" | "index.htm");
    if !is_index {
        let route_name = file_name
            .strip_suffix(".html")
            .or_else(|| file_name.strip_suffix(".htm"))
            .unwrap_or(&file_name);
        parts.push(route_name.to_string());
    }

    let mut url = base_url.clone();
    {
        let mut segments = url
            .path_segments_mut()
            .map_err(|_| anyhow::anyhow!("base URL cannot contain path segments: {base_url}"))?;
        segments.clear();
        for part in &parts {
            segments.push(part);
        }
        if is_index && !parts.is_empty() {
            segments.push("");
        }
    }
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
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
    fn recognizes_sitemap_names_from_native_path_components() {
        assert!(is_sitemap_file(Path::new("sitemap.xml")));
        assert!(is_sitemap_file(
            &Path::new("nested").join("post-sitemap.xml")
        ));
        assert!(is_sitemap_file(
            &Path::new("nested").join("sitemap-pages.xml")
        ));
        assert!(!is_sitemap_file(Path::new("sitemap.xml.bak")));
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
    fn file_path_to_url_encodes_url_delimiters_in_filenames() {
        let url = file_path_to_url(
            Path::new("/dist/question?#.html"),
            Path::new("/dist"),
            &base(),
        )
        .unwrap();
        assert_eq!(url.path(), "/question%3F%23");
        assert!(url.query().is_none());
        assert!(url.fragment().is_none());
    }

    #[test]
    fn file_path_to_url_uses_native_components_and_encodes_each_segment() {
        let root = Path::new("dist");
        let file = root.join("news archive").join("question?#.html");
        let url = file_path_to_url(&file, root, &base()).unwrap();
        assert_eq!(url.path(), "/news%20archive/question%3F%23");
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
        assert!(result.sitemap_files.is_empty());
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
        let snapshot = build_snapshot(result, None, &crate::scanner::default_user_agent()).unwrap();

        assert_eq!(snapshot.statistics.total_urls, 2);
        assert_eq!(snapshot.statistics.indexable_urls, 2);
        assert_eq!(snapshot.statistics.orphan_urls, 0);
    }

    #[test]
    fn missing_static_link_target_is_broken_and_not_indexable() {
        let tmp = tempfile::tempdir().unwrap();
        let dist = tmp.path().join("dist");
        fs::create_dir_all(&dist).unwrap();
        fs::write(
            dist.join("index.html"),
            r#"<html><body><a href="/missing">Missing</a></body></html>"#,
        )
        .unwrap();

        let result = discover_files(&dist, &base()).unwrap();
        let snapshot = build_snapshot(result, None, &crate::scanner::default_user_agent()).unwrap();
        let missing = snapshot.urls.get("https://example.test/missing").unwrap();
        assert_eq!(missing.http_status, Some(404));
        assert!(!missing.is_indexable);
    }

    #[test]
    fn invalid_sitemap_fails_the_scan() {
        let tmp = tempfile::tempdir().unwrap();
        let dist = tmp.path().join("dist");
        fs::create_dir_all(&dist).unwrap();
        fs::write(dist.join("index.html"), "<html></html>").unwrap();
        fs::write(dist.join("sitemap.xml"), "<urlset><url>").unwrap();

        let result = discover_files(&dist, &base()).unwrap();
        let error =
            build_snapshot(result, None, &crate::scanner::default_user_agent()).unwrap_err();
        assert!(error.to_string().contains("parsing sitemap sitemap.xml"));
    }
}
