use std::collections::BTreeSet;

use scraper::Html;
use url::Url;

/// Extract all internal links from an HTML document.
/// Returns normalized absolute URLs that share the same origin as `base_url`.
pub fn extract_internal_links(document: &Html, base_url: &Url) -> Vec<Url> {
    extract_internal_links_with_alias(document, base_url, None)
}

/// Extract internal links while treating an alternate crawl origin as the same site.
pub fn extract_internal_links_with_alias(
    document: &Html,
    base_url: &Url,
    crawl_origin: Option<&Url>,
) -> Vec<Url> {
    let selector = match scraper::Selector::parse("a[href]") {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    let base_origin = base_url.origin();

    document
        .select(&selector)
        .filter_map(|el| {
            let href = el.value().attr("href")?;
            // Skip anchors, javascript:, mailto:, tel:
            if href.starts_with('#')
                || href.starts_with("javascript:")
                || href.starts_with("mailto:")
                || href.starts_with("tel:")
            {
                return None;
            }
            let mut url = base_url.join(href).ok()?;
            if url.origin() != base_origin {
                crawl_origin.filter(|alias| url.origin() == alias.origin())?;
                url = replace_origin(&url, base_url);
            }
            // Strip fragment for normalization
            let mut normalized = url;
            normalized.set_fragment(None);
            Some(normalized)
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn replace_origin(url: &Url, origin: &Url) -> Url {
    let mut mapped = origin.clone();
    mapped.set_path(url.path());
    mapped.set_query(url.query());
    mapped.set_fragment(url.fragment());
    mapped
}

/// Count words in the visible text content of the document.
pub fn count_words(document: &Html) -> usize {
    let body = match scraper::Selector::parse("body").ok() {
        Some(sel) => match document.select(&sel).next() {
            Some(element) => element,
            None => return 0,
        },
        None => return 0,
    };
    let total = body.text().flat_map(str::split_whitespace).count();
    let excluded = scraper::Selector::parse("script, style, template, noscript")
        .ok()
        .map(|selector| {
            body.select(&selector)
                .flat_map(|element| element.text())
                .flat_map(str::split_whitespace)
                .count()
        })
        .unwrap_or(0);

    total.saturating_sub(excluded)
}

/// Count headings (h1..h6) in the document.
pub fn count_headings(document: &Html) -> usize {
    let selector = match scraper::Selector::parse("h1, h2, h3, h4, h5, h6") {
        Ok(s) => s,
        Err(_) => return 0,
    };
    document.select(&selector).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Url {
        Url::parse("https://example.test/").unwrap()
    }

    #[test]
    fn extract_internal_links_basic() {
        let html = concat!(
            "<html><body>",
            "<a href=\"/page1\">Page 1</a>",
            "<a href=\"https://example.test/page2\">Page 2</a>",
            "<a href=\"https://external.test/page\">External</a>",
            "<a href=\"#section\">Anchor</a>",
            "<a href=\"mailto:user@host.com\">Email</a>",
            "</body></html>"
        );
        let doc = Html::parse_document(html);
        let links = extract_internal_links(&doc, &base());
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].path(), "/page1");
        assert_eq!(links[1].path(), "/page2");
    }

    #[test]
    fn fragment_stripped() {
        let html = r##"<html><body><a href="/page#section">Link</a></body></html>"##;
        let doc = Html::parse_document(html);
        let links = extract_internal_links(&doc, &base());
        assert_eq!(links.len(), 1);
        assert!(links[0].fragment().is_none());
    }

    #[test]
    fn crawl_origin_alias_maps_to_public_origin() {
        let html = r#"<html><body><a href="https://preview.test/page">Page</a></body></html>"#;
        let doc = Html::parse_document(html);
        let preview = Url::parse("https://preview.test/").unwrap();
        let links = extract_internal_links_with_alias(&doc, &base(), Some(&preview));
        assert_eq!(
            links,
            vec![Url::parse("https://example.test/page").unwrap()]
        );
    }

    #[test]
    fn word_count() {
        let html = r#"<html><body><p>Hello world this is a test</p>
            <script>const hiddenWords = "not page copy";</script>
            <style>.not-page-copy { color: red; }</style>
        </body></html>"#;
        let doc = Html::parse_document(html);
        assert_eq!(count_words(&doc), 6);
    }

    #[test]
    fn heading_count() {
        let html = r#"<html><body><h1>Title</h1><h2>Sub</h2><h2>Sub2</h2></body></html>"#;
        let doc = Html::parse_document(html);
        assert_eq!(count_headings(&doc), 3);
    }
}
