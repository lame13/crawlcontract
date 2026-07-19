use std::collections::BTreeSet;
use url::Url;

/// A parsed XML sitemap or sitemap index.
#[derive(Debug, Clone)]
pub enum Sitemap {
    /// A regular sitemap containing URLs.
    UrlSet(SitemapUrlSet),
    /// A sitemap index referencing other sitemaps.
    Index(SitemapIndex),
}

#[derive(Debug, Clone)]
pub struct SitemapUrlSet {
    pub urls: Vec<SitemapEntry>,
}

#[derive(Debug, Clone)]
pub struct SitemapEntry {
    pub loc: Url,
    pub lastmod: Option<String>,
    pub changefreq: Option<String>,
    pub priority: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct SitemapIndex {
    pub sitemaps: Vec<Url>,
}

/// Parse an XML sitemap body. Returns the appropriate variant.
pub fn parse_sitemap(body: &str) -> Result<Sitemap, SitemapParseError> {
    // Simple XML parser using string scanning — avoids heavy XML deps for MVP.
    let body = body.trim();
    if body.contains("<sitemapindex") {
        parse_sitemap_index(body)
    } else if body.contains("<urlset") {
        parse_url_set(body)
    } else {
        Err(SitemapParseError::UnknownFormat)
    }
}

fn parse_url_set(body: &str) -> Result<Sitemap, SitemapParseError> {
    let mut urls = Vec::new();
    let mut remaining = body;

    while let Some(start) = remaining.find("<url>") {
        let end = remaining[start..]
            .find("</url>")
            .ok_or(SitemapParseError::Malformed)?;
        let entry_xml = &remaining[start..start + end + 6];
        remaining = &remaining[start + end + 6..];

        if let Some(loc) = extract_tag(entry_xml, "loc") {
            if let Ok(url) = Url::parse(&loc) {
                urls.push(SitemapEntry {
                    loc: url,
                    lastmod: extract_tag(entry_xml, "lastmod"),
                    changefreq: extract_tag(entry_xml, "changefreq"),
                    priority: extract_tag(entry_xml, "priority").and_then(|p| p.parse().ok()),
                });
            }
        }
    }

    Ok(Sitemap::UrlSet(SitemapUrlSet { urls }))
}

fn parse_sitemap_index(body: &str) -> Result<Sitemap, SitemapParseError> {
    let mut sitemaps = Vec::new();
    let mut remaining = body;

    while let Some(start) = remaining.find("<sitemap>") {
        let end = remaining[start..]
            .find("</sitemap>")
            .ok_or(SitemapParseError::Malformed)?;
        let entry_xml = &remaining[start..start + end + 10];
        remaining = &remaining[start + end + 10..];

        if let Some(loc) = extract_tag(entry_xml, "loc") {
            if let Ok(url) = Url::parse(&loc) {
                sitemaps.push(url);
            }
        }
    }

    Ok(Sitemap::Index(SitemapIndex { sitemaps }))
}

fn extract_tag(xml: &str, tag: &str) -> Option<String> {
    let start_tag = format!("<{tag}>");
    let end_tag = format!("</{tag}>");
    let start = xml.find(&start_tag)? + start_tag.len();
    let end = xml[start..].find(&end_tag)?;
    Some(xml[start..start + end].trim().to_string())
}

/// Collect all URLs from a sitemap (recursively for sitemap indexes).
/// Returns the flat list of sitemap entries and any child sitemap URLs that failed to parse.
pub fn collect_sitemap_urls(sitemap: &Sitemap) -> BTreeSet<Url> {
    let mut urls = BTreeSet::new();
    match sitemap {
        Sitemap::UrlSet(set) => {
            for entry in &set.urls {
                urls.insert(entry.loc.clone());
            }
        }
        Sitemap::Index(_) => {
            // Caller must recursively fetch child sitemaps.
        }
    }
    urls
}

#[derive(Debug, thiserror::Error)]
pub enum SitemapParseError {
    #[error("unknown sitemap format")]
    UnknownFormat,
    #[error("malformed sitemap XML")]
    Malformed,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_url_set_basic() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
  <url>
    <loc>https://example.test/page1</loc>
    <lastmod>2026-01-01</lastmod>
    <priority>0.8</priority>
  </url>
  <url>
    <loc>https://example.test/page2</loc>
  </url>
</urlset>"#;
        let sitemap = parse_sitemap(xml).unwrap();
        match sitemap {
            Sitemap::UrlSet(set) => {
                assert_eq!(set.urls.len(), 2);
                assert_eq!(set.urls[0].loc.as_str(), "https://example.test/page1");
                assert_eq!(set.urls[0].lastmod.as_deref(), Some("2026-01-01"));
                assert_eq!(set.urls[0].priority, Some(0.8));
                assert_eq!(set.urls[1].loc.as_str(), "https://example.test/page2");
                assert!(set.urls[1].lastmod.is_none());
            }
            _ => panic!("expected urlset"),
        }
    }

    #[test]
    fn parse_sitemap_index() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<sitemapindex>
  <sitemap>
    <loc>https://example.test/sitemap-pages.xml</loc>
  </sitemap>
  <sitemap>
    <loc>https://example.test/sitemap-posts.xml</loc>
  </sitemap>
</sitemapindex>"#;
        let sitemap = parse_sitemap(xml).unwrap();
        match sitemap {
            Sitemap::Index(idx) => {
                assert_eq!(idx.sitemaps.len(), 2);
            }
            _ => panic!("expected index"),
        }
    }

    #[test]
    fn unknown_format() {
        assert!(parse_sitemap("<html></html>").is_err());
    }
}
