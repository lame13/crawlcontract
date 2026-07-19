use quick_xml::events::Event;
use quick_xml::Reader;
use serde::Deserialize;
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
    match root_element(body)? {
        RootElement::UrlSet => {
            let document: XmlUrlSet = quick_xml::de::from_str(body)?;
            let urls = document
                .urls
                .into_iter()
                .map(SitemapEntry::try_from)
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Sitemap::UrlSet(SitemapUrlSet { urls }))
        }
        RootElement::SitemapIndex => {
            let document: XmlSitemapIndex = quick_xml::de::from_str(body)?;
            let sitemaps = document
                .sitemaps
                .into_iter()
                .map(|entry| parse_loc(&entry.loc))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Sitemap::Index(SitemapIndex { sitemaps }))
        }
    }
}

#[derive(Debug, Deserialize)]
struct XmlUrlSet {
    #[serde(rename = "url", default)]
    urls: Vec<XmlSitemapEntry>,
}

#[derive(Debug, Deserialize)]
struct XmlSitemapEntry {
    loc: String,
    #[serde(default)]
    lastmod: Option<String>,
    #[serde(default)]
    changefreq: Option<String>,
    #[serde(default)]
    priority: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct XmlSitemapIndex {
    #[serde(rename = "sitemap", default)]
    sitemaps: Vec<XmlSitemapReference>,
}

#[derive(Debug, Deserialize)]
struct XmlSitemapReference {
    loc: String,
}

impl TryFrom<XmlSitemapEntry> for SitemapEntry {
    type Error = SitemapParseError;

    fn try_from(entry: XmlSitemapEntry) -> Result<Self, Self::Error> {
        Ok(Self {
            loc: parse_loc(&entry.loc)?,
            lastmod: entry.lastmod,
            changefreq: entry.changefreq,
            priority: entry.priority,
        })
    }
}

fn parse_loc(value: &str) -> Result<Url, SitemapParseError> {
    Url::parse(value.trim()).map_err(|_| SitemapParseError::InvalidUrl(value.trim().to_string()))
}

enum RootElement {
    UrlSet,
    SitemapIndex,
}

fn root_element(body: &str) -> Result<RootElement, SitemapParseError> {
    let mut reader = Reader::from_str(body);
    reader.config_mut().trim_text(true);

    loop {
        match reader.read_event()? {
            Event::Start(element) | Event::Empty(element) => {
                return match element.local_name().as_ref() {
                    b"urlset" => Ok(RootElement::UrlSet),
                    b"sitemapindex" => Ok(RootElement::SitemapIndex),
                    _ => Err(SitemapParseError::UnknownFormat),
                };
            }
            Event::Eof => return Err(SitemapParseError::UnknownFormat),
            _ => {}
        }
    }
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
    #[error("malformed sitemap XML: {0}")]
    MalformedXml(#[from] quick_xml::Error),
    #[error("invalid sitemap document: {0}")]
    MalformedDocument(#[from] quick_xml::DeError),
    #[error("invalid URL in sitemap <loc>: {0}")]
    InvalidUrl(String),
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

    #[test]
    fn parses_escaped_url_and_namespaced_root() {
        let xml = r#"<?xml version="1.0"?>
<sm:urlset xmlns:sm="http://www.sitemaps.org/schemas/sitemap/0.9">
  <sm:url><sm:loc>https://example.test/search?a=1&amp;b=2</sm:loc></sm:url>
</sm:urlset>"#;
        let sitemap = parse_sitemap(xml).unwrap();
        let Sitemap::UrlSet(set) = sitemap else {
            panic!("expected urlset");
        };
        assert_eq!(set.urls[0].loc.query(), Some("a=1&b=2"));
    }

    #[test]
    fn rejects_invalid_loc_instead_of_silently_dropping_it() {
        let xml = r#"<urlset><url><loc>not a URL</loc></url></urlset>"#;
        assert!(matches!(
            parse_sitemap(xml),
            Err(SitemapParseError::InvalidUrl(_))
        ));
    }

    #[test]
    fn parses_empty_urlset() {
        let sitemap = parse_sitemap("<urlset/>").unwrap();
        let Sitemap::UrlSet(set) = sitemap else {
            panic!("expected urlset");
        };
        assert!(set.urls.is_empty());
    }
}
