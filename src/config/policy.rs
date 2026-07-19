use std::path::Path;

use serde::{Deserialize, Serialize};
use url::Url;

/// Policy configuration loaded from a TOML file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Policy {
    #[serde(default)]
    pub general: GeneralPolicy,
    #[serde(default)]
    pub exclusions: Vec<Exclusion>,
    #[serde(default)]
    pub diff: DiffPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralPolicy {
    #[serde(default = "default_fail_on")]
    pub fail_on: Vec<String>,
    pub public_origin: Option<String>,
}

impl Default for GeneralPolicy {
    fn default() -> Self {
        Self {
            fail_on: default_fail_on(),
            public_origin: None,
        }
    }
}

fn default_fail_on() -> Vec<String> {
    vec!["error".to_string()]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Exclusion {
    pub rule_id: String,
    pub url_pattern: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffPolicy {
    #[serde(default = "default_max_indexable_loss")]
    pub max_indexable_url_loss_percent: f64,
    #[serde(default = "default_max_link_loss")]
    pub max_link_loss_percent: f64,
    #[serde(default = "default_max_word_loss")]
    pub max_word_loss_percent: f64,
    #[serde(default = "default_max_heading_loss")]
    pub max_heading_loss_percent: f64,
}

impl Default for DiffPolicy {
    fn default() -> Self {
        Self {
            max_indexable_url_loss_percent: default_max_indexable_loss(),
            max_link_loss_percent: default_max_link_loss(),
            max_word_loss_percent: default_max_word_loss(),
            max_heading_loss_percent: default_max_heading_loss(),
        }
    }
}

fn default_max_indexable_loss() -> f64 {
    5.0
}
fn default_max_link_loss() -> f64 {
    10.0
}
fn default_max_word_loss() -> f64 {
    50.0
}
fn default_max_heading_loss() -> f64 {
    15.0
}

impl Policy {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let policy: Policy = toml::from_str(&content)?;
        policy.validate()?;
        Ok(policy)
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        for severity in &self.general.fail_on {
            if !matches!(
                severity.to_ascii_lowercase().as_str(),
                "error" | "warning" | "info"
            ) {
                anyhow::bail!("unknown general.fail_on severity: {severity}");
            }
        }

        if let Some(origin) = &self.general.public_origin {
            parse_public_origin(origin)?;
        }

        for (name, value) in [
            (
                "max_indexable_url_loss_percent",
                self.diff.max_indexable_url_loss_percent,
            ),
            ("max_link_loss_percent", self.diff.max_link_loss_percent),
            ("max_word_loss_percent", self.diff.max_word_loss_percent),
            (
                "max_heading_loss_percent",
                self.diff.max_heading_loss_percent,
            ),
        ] {
            if !(0.0..=100.0).contains(&value) {
                anyhow::bail!("diff.{name} must be between 0 and 100, got {value}");
            }
        }

        Ok(())
    }

    /// Returns true if a finding for the given rule_id and url should be excluded.
    pub fn is_excluded(&self, rule_id: &str, url: &str) -> bool {
        let path_and_query = Url::parse(url)
            .ok()
            .map(|parsed| parsed[url::Position::BeforePath..url::Position::AfterQuery].to_string());

        self.exclusions.iter().any(|exclusion| {
            exclusion.rule_id == rule_id
                && (glob_match(&exclusion.url_pattern, url)
                    || path_and_query
                        .as_deref()
                        .is_some_and(|path| glob_match(&exclusion.url_pattern, path)))
        })
    }

    /// Returns true if the given severity should cause a non-zero exit code.
    pub fn should_fail(&self, severity: &str) -> bool {
        self.general
            .fail_on
            .iter()
            .any(|s| s.eq_ignore_ascii_case(severity))
    }
}

pub fn parse_public_origin(value: &str) -> anyhow::Result<Url> {
    let url = Url::parse(value)?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        anyhow::bail!("public origin must be an absolute HTTP(S) URL: {value}");
    }
    if url.path() != "/" || url.query().is_some() || url.fragment().is_some() {
        anyhow::bail!("public origin must not contain a path, query, or fragment: {value}");
    }
    if !url.username().is_empty() || url.password().is_some() {
        anyhow::bail!("public origin must not contain credentials: {value}");
    }
    Ok(url)
}

/// Simple glob matching supporting `*` (single segment) and `**` (multi-segment).
fn glob_match(pattern: &str, value: &str) -> bool {
    let re = glob_to_regex(pattern);
    simple_match_inner(re.as_bytes(), value.as_bytes())
}

fn glob_to_regex(pattern: &str) -> String {
    let mut regex = String::new();
    let chars: Vec<char> = pattern.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '*' => {
                if i + 1 < chars.len() && chars[i + 1] == '*' {
                    regex.push_str(".*");
                    i += 2;
                    // skip trailing slash if present
                    if i < chars.len() && chars[i] == '/' {
                        i += 1;
                    }
                } else {
                    regex.push_str("[^/]*");
                    i += 1;
                }
            }
            '?' => {
                regex.push('.');
                i += 1;
            }
            '.' | '^' | '$' | '|' | '+' | '(' | ')' | '[' | ']' | '{' | '}' | '\\' => {
                regex.push('\\');
                regex.push(chars[i]);
                i += 1;
            }
            c => {
                regex.push(c);
                i += 1;
            }
        }
    }
    regex
}

/// Minimal regex matcher — supports `.` (any), `[^...]` (negated class),
/// and `*`, `+`, `?` quantifiers.
fn simple_match_inner(pattern: &[u8], text: &[u8]) -> bool {
    if pattern.is_empty() {
        return text.is_empty();
    }

    // Handle [^...] or [...] character class (possibly followed by *)
    if pattern[0] == b'[' {
        if let Some(close) = find_bracket_close(pattern) {
            let negated = close > 1 && pattern[1] == b'^';
            let class_start = if negated { 2 } else { 1 };
            let class_body = &pattern[class_start..close];

            // Check for quantifier after ]
            if close + 1 < pattern.len() && pattern[close + 1] == b'*' {
                // Zero or more matches of the character class
                let rest = &pattern[close + 2..];
                for i in 0..=text.len() {
                    let segment = &text[..i];
                    let ok = if negated {
                        segment.iter().all(|c| !class_body.contains(c))
                    } else {
                        segment.iter().all(|c| class_body.contains(c))
                    };
                    if ok && simple_match_inner(rest, &text[i..]) {
                        return true;
                    }
                }
                return false;
            } else {
                // Exactly one match of the character class
                if text.is_empty() {
                    return false;
                }
                let c = text[0];
                let ok = if negated {
                    !class_body.contains(&c)
                } else {
                    class_body.contains(&c)
                };
                if ok {
                    return simple_match_inner(&pattern[close + 1..], &text[1..]);
                }
                return false;
            }
        }
    }

    // Handle .*
    if pattern.len() >= 2 && pattern[0] == b'.' && pattern[1] == b'*' {
        let rest = &pattern[2..];
        for i in 0..=text.len() {
            if simple_match_inner(rest, &text[i..]) {
                return true;
            }
        }
        return false;
    }
    // .+
    if pattern.len() >= 2 && pattern[0] == b'.' && pattern[1] == b'+' {
        let rest = &pattern[2..];
        for i in 1..=text.len() {
            if simple_match_inner(rest, &text[i..]) {
                return true;
            }
        }
        return false;
    }
    // Escaped literal
    if pattern[0] == b'\\' && pattern.len() >= 2 {
        if !text.is_empty() && pattern[1] == text[0] {
            return simple_match_inner(&pattern[2..], &text[1..]);
        }
        return false;
    }
    // Literal or . (any)
    if !text.is_empty() && (pattern[0] == b'.' || pattern[0] == text[0]) {
        return simple_match_inner(&pattern[1..], &text[1..]);
    }
    false
}

/// Find the position of the closing `]` in a bracket expression starting at `pattern[0] == '['`.
fn find_bracket_close(pattern: &[u8]) -> Option<usize> {
    if pattern[0] != b'[' {
        return None;
    }
    // Handle special opening: [^ or [] or [^
    let mut i = 1;
    if i < pattern.len() && pattern[i] == b'^' {
        i += 1;
    }
    // A ] immediately after [ or [^ is treated as a literal
    if i < pattern.len() && pattern[i] == b']' {
        i += 1;
    }
    while i < pattern.len() {
        if pattern[i] == b']' {
            return Some(i);
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glob_match_simple() {
        assert!(glob_match("/admin/*", "/admin/page"));
        assert!(!glob_match("/admin/*", "/admin/sub/page"));
    }

    #[test]
    fn glob_match_double_star() {
        assert!(glob_match("/admin/**", "/admin/page"));
        assert!(glob_match("/admin/**", "/admin/sub/page"));
        assert!(!glob_match("/admin/**", "/other/page"));
    }

    #[test]
    fn load_policy_from_str() {
        let toml = r#"
[general]
fail_on = ["error", "warning"]

[[exclusions]]
rule_id = "CC-ORPHAN-001"
url_pattern = "/admin/**"
reason = "Admin not linked from public pages"

[diff]
max_indexable_url_loss_percent = 10
"#;
        let policy: Policy = toml::from_str(toml).unwrap();
        assert_eq!(policy.general.fail_on, vec!["error", "warning"]);
        assert_eq!(policy.exclusions.len(), 1);
        assert!(policy.is_excluded("CC-ORPHAN-001", "/admin/settings"));
        assert!(policy.is_excluded("CC-ORPHAN-001", "https://example.test/admin/settings"));
        assert!(!policy.is_excluded("CC-ORPHAN-001", "/public/page"));
        assert!(policy.should_fail("Error"));
        assert!(policy.should_fail("WARNING"));
        assert!(!policy.should_fail("info"));
    }

    #[test]
    fn validation_rejects_unknown_severity_and_invalid_threshold() {
        let mut policy = Policy {
            general: GeneralPolicy::default(),
            exclusions: Vec::new(),
            diff: DiffPolicy::default(),
        };
        policy.general.fail_on = vec!["fatal".to_string()];
        assert!(policy.validate().is_err());

        policy.general.fail_on = vec!["error".to_string()];
        policy.diff.max_word_loss_percent = 101.0;
        assert!(policy.validate().is_err());
    }

    #[test]
    fn public_origin_requires_a_bare_http_origin() {
        assert!(parse_public_origin("https://example.test").is_ok());
        assert!(parse_public_origin("https://example.test/subpath").is_err());
        assert!(parse_public_origin("ftp://example.test").is_err());
    }
}
