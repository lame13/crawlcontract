use scraper::Html;
use url::Url;

/// Extract the `<link rel="canonical" href="...">` URL from an HTML document.
pub fn extract_html_canonical(document: &Html, base_url: &Url) -> Option<Url> {
    let selector = scraper::Selector::parse("link[rel=\"canonical\"]").ok()?;
    let element = document.select(&selector).next()?;
    let href = element.value().attr("href")?;
    resolve_url(base_url, href)
}

/// Extract canonical URL from an HTTP `Link` header.
/// Format: `<https://example.test/page>; rel="canonical"`
pub fn extract_http_canonical(link_header: &str, base_url: &Url) -> Option<Url> {
    for segment in link_header.split(',') {
        let segment = segment.trim();
        if !segment.contains("rel=\"canonical\"") {
            continue;
        }
        if let Some(start) = segment.find('<') {
            if let Some(end) = segment[start..].find('>') {
                let url_str = &segment[start + 1..start + end];
                return resolve_url(base_url, url_str);
            }
        }
    }
    None
}

fn resolve_url(base: &Url, href: &str) -> Option<Url> {
    base.join(href).ok().or_else(|| Url::parse(href).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_from_html() {
        let html = r#"<html><head><link rel="canonical" href="/canonical-page"></head></html>"#;
        let doc = Html::parse_document(html);
        let base = Url::parse("https://example.test/").unwrap();
        let canonical = extract_html_canonical(&doc, &base).unwrap();
        assert_eq!(canonical.as_str(), "https://example.test/canonical-page");
    }

    #[test]
    fn extract_from_link_header() {
        let header = r#"<https://example.test/canonical>; rel="canonical""#;
        let base = Url::parse("https://example.test/").unwrap();
        let canonical = extract_http_canonical(header, &base).unwrap();
        assert_eq!(canonical.as_str(), "https://example.test/canonical");
    }

    #[test]
    fn no_canonical_returns_none() {
        let html = r#"<html><head></head></html>"#;
        let doc = Html::parse_document(html);
        let base = Url::parse("https://example.test/").unwrap();
        assert!(extract_html_canonical(&doc, &base).is_none());
    }
}
