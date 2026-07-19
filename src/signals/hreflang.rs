use scraper::Html;
use url::Url;

use crate::model::hreflang::HreflangEntry;

/// Extract all hreflang entries from `<link rel="alternate" hreflang="..." href="...">`.
pub fn extract_hreflang(document: &Html, base_url: &Url) -> Vec<HreflangEntry> {
    let selector = match scraper::Selector::parse("link[rel][hreflang][href]") {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    document
        .select(&selector)
        .filter_map(|el| {
            let is_alternate = el
                .value()
                .attr("rel")?
                .split_ascii_whitespace()
                .any(|token| token.eq_ignore_ascii_case("alternate"));
            if !is_alternate {
                return None;
            }
            let lang = el.value().attr("hreflang")?.to_string();
            let href = el.value().attr("href")?;
            let mut url = base_url.join(href).ok().or_else(|| Url::parse(href).ok())?;
            url.set_fragment(None);
            Some(HreflangEntry { lang, url })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_hreflang_entries() {
        let html = r#"
<html><head>
  <link rel="alternate" hreflang="en" href="/en/page">
  <link rel="alternate" hreflang="es" href="/es/page">
  <link rel="alternate" hreflang="x-default" href="/page">
</head></html>"#;
        let doc = Html::parse_document(html);
        let base = Url::parse("https://example.test/").unwrap();
        let entries = extract_hreflang(&doc, &base);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].lang, "en");
        assert_eq!(entries[1].lang, "es");
        assert_eq!(entries[2].lang, "x-default");
    }

    #[test]
    fn no_hreflang_returns_empty() {
        let html = r#"<html><head></head></html>"#;
        let doc = Html::parse_document(html);
        let base = Url::parse("https://example.test/").unwrap();
        assert!(extract_hreflang(&doc, &base).is_empty());
    }

    #[test]
    fn alternate_rel_is_tokenized_and_case_insensitive() {
        let html = r#"<html><head>
            <link rel="stylesheet alternate" hreflang="en" href="/en">
            <link rel="ALTERNATE" hreflang="es" href="/es">
        </head></html>"#;
        let doc = Html::parse_document(html);
        let base = Url::parse("https://example.test/").unwrap();
        assert_eq!(extract_hreflang(&doc, &base).len(), 2);
    }
}
