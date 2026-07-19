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

    /// Merge two additive directives using the more restrictive value on conflict.
    pub fn merge(&self, other: &RobotsDirective) -> RobotsDirective {
        RobotsDirective {
            noindex: restrictive_bool(self.noindex, other.noindex),
            nofollow: restrictive_bool(self.nofollow, other.nofollow),
            noarchive: restrictive_bool(self.noarchive, other.noarchive),
            nosnippet: restrictive_bool(self.nosnippet, other.nosnippet),
            max_snippet: restrictive_limit(self.max_snippet, other.max_snippet),
            max_image_preview: restrictive_image_preview(
                self.max_image_preview.as_deref(),
                other.max_image_preview.as_deref(),
            ),
            max_video_preview: restrictive_limit(self.max_video_preview, other.max_video_preview),
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

fn restrictive_bool(left: Option<bool>, right: Option<bool>) -> Option<bool> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left || right),
        (left, right) => left.or(right),
    }
}

fn restrictive_limit(left: Option<i64>, right: Option<i64>) -> Option<i64> {
    match (left, right) {
        (Some(left), Some(right)) => match (left < 0, right < 0) {
            (true, false) => Some(right),
            (false, true) => Some(left),
            _ => Some(left.min(right)),
        },
        (left, right) => left.or(right),
    }
}

fn restrictive_image_preview(left: Option<&str>, right: Option<&str>) -> Option<String> {
    fn rank(value: &str) -> u8 {
        match value.to_ascii_lowercase().as_str() {
            "none" => 0,
            "standard" => 1,
            "large" => 2,
            _ => 3,
        }
    }

    match (left, right) {
        (Some(left), Some(right)) => Some(
            if rank(left) <= rank(right) {
                left
            } else {
                right
            }
            .into(),
        ),
        (Some(value), None) | (None, Some(value)) => Some(value.into()),
        (None, None) => None,
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
            "none" => {
                directive.noindex = Some(true);
                directive.nofollow = Some(true);
            }
            "index" => {
                directive.noindex.get_or_insert(false);
            }
            "follow" => {
                directive.nofollow.get_or_insert(false);
            }
            "all" => {
                directive.noindex.get_or_insert(false);
                directive.nofollow.get_or_insert(false);
            }
            _ if token.starts_with("max-snippet:") => {
                if let Some(val) = token.strip_prefix("max-snippet:") {
                    directive.max_snippet =
                        restrictive_limit(directive.max_snippet, val.trim().parse().ok());
                }
            }
            _ if token.starts_with("max-image-preview:") => {
                if let Some(val) = token.strip_prefix("max-image-preview:") {
                    directive.max_image_preview = Some(val.trim().to_string());
                }
            }
            _ if token.starts_with("max-video-preview:") => {
                if let Some(val) = token.strip_prefix("max-video-preview:") {
                    directive.max_video_preview =
                        restrictive_limit(directive.max_video_preview, val.trim().parse().ok());
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
    fn merge_keeps_more_restrictive_boolean() {
        let a = parse_directive_value("noindex");
        let b = parse_directive_value("index");
        let merged = a.merge(&b);
        assert_eq!(merged.noindex, Some(true));
    }

    #[test]
    fn merge_keeps_more_restrictive_limits() {
        let a = parse_directive_value("max-snippet:100, max-image-preview:large");
        let b = parse_directive_value("max-snippet:20, max-image-preview:none");
        let merged = a.merge(&b);
        assert_eq!(merged.max_snippet, Some(20));
        assert_eq!(merged.max_image_preview.as_deref(), Some("none"));
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

    #[test]
    fn restrictive_tokens_win_within_one_directive() {
        let directive = parse_directive_value(
            "index, noindex, follow, nofollow, max-snippet:-1, max-snippet:20",
        );
        assert!(directive.is_noindex());
        assert!(directive.is_nofollow());
        assert_eq!(directive.max_snippet, Some(20));
    }

    #[test]
    fn none_expands_to_noindex_nofollow() {
        let directive = parse_directive_value("none");
        assert!(directive.is_noindex());
        assert!(directive.is_nofollow());
    }
}
