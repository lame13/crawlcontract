use scraper::Html;
use url::Url;

/// Extract the `<link rel="canonical" href="...">` URL from an HTML document.
pub fn extract_html_canonical(document: &Html, base_url: &Url) -> Option<Url> {
    let selector = scraper::Selector::parse("link[rel][href]").ok()?;
    document.select(&selector).find_map(|element| {
        let rel = element.value().attr("rel")?;
        if !rel
            .split_ascii_whitespace()
            .any(|token| token.eq_ignore_ascii_case("canonical"))
        {
            return None;
        }
        resolve_url(base_url, element.value().attr("href")?)
    })
}

/// Extract canonical URL from an HTTP `Link` header.
/// Format: `<https://example.test/page>; rel="canonical"`
pub fn extract_http_canonical(link_header: &str, base_url: &Url) -> Option<Url> {
    for segment in link_header.split(',') {
        let segment = segment.trim();
        let has_canonical_rel = segment.split(';').skip(1).any(|parameter| {
            let Some((name, value)) = parameter.split_once('=') else {
                return false;
            };
            name.trim().eq_ignore_ascii_case("rel")
                && value
                    .trim()
                    .trim_matches(['\'', '"'])
                    .split_ascii_whitespace()
                    .any(|token| token.eq_ignore_ascii_case("canonical"))
        });
        if !has_canonical_rel {
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
    let mut url = base.join(href).ok().or_else(|| Url::parse(href).ok())?;
    url.set_fragment(None);
    Some(url)
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
    fn extracts_tokenized_case_insensitive_http_rel() {
        let header = "<https://example.test/canonical>; rel='Alternate CANONICAL'";
        let base = Url::parse("https://example.test/").unwrap();
        assert_eq!(
            extract_http_canonical(header, &base).unwrap().path(),
            "/canonical"
        );
    }

    #[test]
    fn no_canonical_returns_none() {
        let html = r#"<html><head></head></html>"#;
        let doc = Html::parse_document(html);
        let base = Url::parse("https://example.test/").unwrap();
        assert!(extract_html_canonical(&doc, &base).is_none());
    }

    #[test]
    fn canonical_rel_is_tokenized_and_case_insensitive() {
        let html = r#"<html><head><link rel="Alternate CANONICAL" href="/page"></head></html>"#;
        let doc = Html::parse_document(html);
        let base = Url::parse("https://example.test/").unwrap();
        assert_eq!(extract_html_canonical(&doc, &base).unwrap().path(), "/page");
    }
}
