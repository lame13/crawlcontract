use scraper::Html;

use crate::model::robots::{parse_directive_value, RobotsDirective};

/// Extract meta robots directive from `<meta name="robots" content="...">`.
pub fn extract_meta_robots(document: &Html) -> Option<RobotsDirective> {
    let selector = scraper::Selector::parse("meta[name=\"robots\"]").ok()?;
    let element = document.select(&selector).next()?;
    let content = element.value().attr("content")?;
    let directive = parse_directive_value(content);
    if directive.is_set() {
        Some(directive)
    } else {
        None
    }
}

/// Parse the X-Robots-Tag HTTP header value into a RobotsDirective.
pub fn parse_x_robots_tag(header_value: &str) -> Option<RobotsDirective> {
    let directive = parse_directive_value(header_value);
    if directive.is_set() {
        Some(directive)
    } else {
        None
    }
}

/// Combine meta robots and X-Robots-Tag into the effective directive.
/// When both are present, they are merged (X-Robots-Tag takes precedence
/// for conflicting fields, following Google's additive interpretation).
pub fn effective_robots_directive(
    meta: Option<&RobotsDirective>,
    http: Option<&RobotsDirective>,
) -> RobotsDirective {
    match (meta, http) {
        (Some(m), Some(h)) => m.merge(h),
        (Some(m), None) => m.clone(),
        (None, Some(h)) => h.clone(),
        (None, None) => RobotsDirective::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_meta_robots_from_html() {
        let html = r#"<html><head><meta name="robots" content="noindex, nofollow"></head></html>"#;
        let doc = Html::parse_document(html);
        let directive = extract_meta_robots(&doc).unwrap();
        assert!(directive.is_noindex());
        assert!(directive.is_nofollow());
    }

    #[test]
    fn parse_x_robots_tag_header() {
        let directive = parse_x_robots_tag("noindex").unwrap();
        assert!(directive.is_noindex());
        assert!(!directive.is_nofollow());
    }

    #[test]
    fn effective_combines_both() {
        let meta = parse_directive_value("noindex");
        let http = parse_directive_value("nofollow");
        let effective = effective_robots_directive(Some(&meta), Some(&http));
        assert!(effective.is_noindex());
        assert!(effective.is_nofollow());
    }

    #[test]
    fn effective_neither_set() {
        let effective = effective_robots_directive(None, None);
        assert!(!effective.is_set());
    }
}
