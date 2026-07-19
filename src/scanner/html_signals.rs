use std::collections::BTreeMap;

use scraper::Html;
use url::Url;

use crate::model::hreflang::HreflangEntry;
use crate::model::robots::RobotsDirective;
use crate::model::url_state::{UrlSource, UrlState};
use crate::signals::canonical::extract_html_canonical;
use crate::signals::hreflang::extract_hreflang;
use crate::signals::internal_links::{
    count_headings, count_words, extract_internal_links, extract_internal_links_with_alias,
};
use crate::signals::robots_directive::extract_meta_robots;

/// All signals extracted from a single HTML document.
#[derive(Debug)]
pub struct HtmlSignals {
    pub canonical: Option<Url>,
    pub meta_robots: Option<RobotsDirective>,
    pub hreflang: Vec<HreflangEntry>,
    pub internal_links: Vec<Url>,
    pub word_count: usize,
    pub heading_count: usize,
}

/// Extract all HTML signals from a parsed document.
pub fn extract_html_signals(document: &Html, base_url: &Url) -> HtmlSignals {
    extract_html_signals_with_alias(document, base_url, None)
}

pub fn extract_html_signals_with_alias(
    document: &Html,
    base_url: &Url,
    crawl_origin: Option<&Url>,
) -> HtmlSignals {
    HtmlSignals {
        canonical: extract_html_canonical(document, base_url),
        meta_robots: extract_meta_robots(document),
        hreflang: extract_hreflang(document, base_url),
        internal_links: if crawl_origin.is_some() {
            extract_internal_links_with_alias(document, base_url, crawl_origin)
        } else {
            extract_internal_links(document, base_url)
        },
        word_count: count_words(document),
        heading_count: count_headings(document),
    }
}

/// Apply extracted HTML signals to a UrlState.
pub fn apply_html_signals(state: &mut UrlState, signals: &HtmlSignals) {
    state.html_canonical = signals.canonical.clone();
    state.html_meta_robots = signals.meta_robots.clone();
    state.hreflang = signals.hreflang.clone();
    state.internal_links_out = signals.internal_links.clone();
    state.word_count = Some(signals.word_count);
    state.heading_count = Some(signals.heading_count);
}

/// Build or update UrlStates from a map of URL → HTML content.
/// Returns the updated states.
pub fn process_html_files(html_files: &[(Url, String)], states: &mut BTreeMap<String, UrlState>) {
    for (url, html_content) in html_files {
        let document = Html::parse_document(html_content);
        let signals = extract_html_signals(&document, url);

        let key = normalize_url_key(url);
        let state = states.entry(key).or_insert_with(|| {
            let mut s = UrlState::new(url.clone());
            s.add_source(UrlSource::DirectScan);
            s
        });

        state.add_source(UrlSource::DirectScan);
        state.http_status = Some(200);
        apply_html_signals(state, &signals);

        if let Some(canonical) = &signals.canonical {
            let canonical_key = normalize_url_key(canonical);
            let canonical_state = states
                .entry(canonical_key)
                .or_insert_with(|| UrlState::new(canonical.clone()));
            canonical_state.add_source(UrlSource::Canonical);
        }

        // Register discovered hreflang URLs
        for entry in &signals.hreflang {
            let href_key = normalize_url_key(&entry.url);
            let state = states.entry(href_key).or_insert_with(|| {
                let mut s = UrlState::new(entry.url.clone());
                s.add_source(UrlSource::Hreflang);
                s
            });
            state.add_source(UrlSource::Hreflang);
        }

        // Register discovered internal link targets
        for link_url in &signals.internal_links {
            let link_key = normalize_url_key(link_url);
            let state = states.entry(link_key).or_insert_with(|| {
                let mut s = UrlState::new(link_url.clone());
                s.add_source(UrlSource::InternalLink);
                s
            });
            state.add_source(UrlSource::InternalLink);
        }
    }
}

/// Normalize a URL to a canonical string key for the BTreeMap.
pub fn normalize_url_key(url: &Url) -> String {
    let mut normalized = url.clone();
    normalized.set_fragment(None);
    // Ensure trailing slash consistency for directory-like paths
    normalized.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Url {
        Url::parse("https://example.test/").unwrap()
    }

    #[test]
    fn extract_all_signals() {
        let html = r#"
<html lang="en">
<head>
  <link rel="canonical" href="/page">
  <meta name="robots" content="noindex">
  <link rel="alternate" hreflang="en" href="/en/page">
</head>
<body>
  <h1>Title</h1>
  <p>Hello world</p>
  <a href="/other">Other page</a>
</body>
</html>"#;
        let doc = Html::parse_document(html);
        let signals = extract_html_signals(&doc, &base());
        assert!(signals.canonical.is_some());
        assert!(signals.meta_robots.as_ref().unwrap().is_noindex());
        assert_eq!(signals.hreflang.len(), 1);
        assert_eq!(signals.internal_links.len(), 1);
        assert_eq!(signals.word_count, 5);
        assert_eq!(signals.heading_count, 1);
    }
}
