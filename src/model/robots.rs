use serde::{Deserialize, Serialize};

/// Effective robots directive for a URL, combining meta robots and X-Robots-Tag.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RobotsDirective {
    pub noindex: Option<bool>,
    pub nofollow: Option<bool>,
    pub noarchive: Option<bool>,
    pub nosnippet: Option<bool>,
    pub max_snippet: Option<i64>,
    pub max_image_preview: Option<String>,
    pub max_video_preview: Option<i64>,
    pub unavailable_after: Option<String>,
}

impl RobotsDirective {
    /// Returns true if the directive explicitly sets noindex.
    pub fn is_noindex(&self) -> bool {
        self.noindex == Some(true)
    }

    /// Returns true if the directive explicitly sets nofollow.
    pub fn is_nofollow(&self) -> bool {
        self.nofollow == Some(true)
    }

    /// Merge two directives. When both set the same field, `other` wins.
    /// This models the combination of meta robots + X-Robots-Tag where the
    /// more restrictive interpretation applies. Google treats them as additive.
    pub fn merge(&self, other: &RobotsDirective) -> RobotsDirective {
        RobotsDirective {
            noindex: other.noindex.or(self.noindex),
            nofollow: other.nofollow.or(self.nofollow),
            noarchive: other.noarchive.or(self.noarchive),
            nosnippet: other.nosnippet.or(self.nosnippet),
            max_snippet: other.max_snippet.or(self.max_snippet),
            max_image_preview: other
                .max_image_preview
                .clone()
                .or_else(|| self.max_image_preview.clone()),
            max_video_preview: other.max_video_preview.or(self.max_video_preview),
            unavailable_after: other
                .unavailable_after
                .clone()
                .or_else(|| self.unavailable_after.clone()),
        }
    }

    /// Returns true if any directive is set (i.e., not an empty/default directive).
    pub fn is_set(&self) -> bool {
        self.noindex.is_some()
            || self.nofollow.is_some()
            || self.noarchive.is_some()
            || self.nosnippet.is_some()
            || self.max_snippet.is_some()
            || self.max_image_preview.is_some()
            || self.max_video_preview.is_some()
            || self.unavailable_after.is_some()
    }
}

/// Parse a robots directive value string like "noindex, nofollow" into a RobotsDirective.
pub fn parse_directive_value(value: &str) -> RobotsDirective {
    let mut directive = RobotsDirective::default();
    for part in value.split(',') {
        let token = part.trim().to_lowercase();
        match token.as_str() {
            "noindex" => directive.noindex = Some(true),
            "nofollow" => directive.nofollow = Some(true),
            "noarchive" => directive.noarchive = Some(true),
            "nosnippet" => directive.nosnippet = Some(true),
            "index" => directive.noindex = Some(false),
            "follow" => directive.nofollow = Some(false),
            _ if token.starts_with("max-snippet:") => {
                if let Some(val) = token.strip_prefix("max-snippet:") {
                    directive.max_snippet = val.trim().parse().ok();
                }
            }
            _ if token.starts_with("max-image-preview:") => {
                if let Some(val) = token.strip_prefix("max-image-preview:") {
                    directive.max_image_preview = Some(val.trim().to_string());
                }
            }
            _ if token.starts_with("max-video-preview:") => {
                if let Some(val) = token.strip_prefix("max-video-preview:") {
                    directive.max_video_preview = val.trim().parse().ok();
                }
            }
            _ if token.starts_with("unavailable_after:") => {
                if let Some(val) = token.strip_prefix("unavailable_after:") {
                    directive.unavailable_after = Some(val.trim().to_string());
                }
            }
            _ => {}
        }
    }
    directive
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_noindex_nofollow() {
        let d = parse_directive_value("noindex, nofollow");
        assert_eq!(d.noindex, Some(true));
        assert_eq!(d.nofollow, Some(true));
        assert!(d.is_noindex());
        assert!(d.is_nofollow());
    }

    #[test]
    fn parse_index_follow() {
        let d = parse_directive_value("index, follow");
        assert_eq!(d.noindex, Some(false));
        assert_eq!(d.nofollow, Some(false));
        assert!(!d.is_noindex());
    }

    #[test]
    fn merge_prefers_other() {
        let a = parse_directive_value("noindex");
        let b = parse_directive_value("index");
        let merged = a.merge(&b);
        // other (b) wins: index overrides noindex
        assert_eq!(merged.noindex, Some(false));
    }

    #[test]
    fn default_is_not_set() {
        let d = RobotsDirective::default();
        assert!(!d.is_set());
    }

    #[test]
    fn parse_max_snippet() {
        let d = parse_directive_value("max-snippet:100");
        assert_eq!(d.max_snippet, Some(100));
        assert!(d.is_set());
    }
}
