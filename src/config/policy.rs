use std::path::Path;

use serde::{Deserialize, Serialize};

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
    #[serde(default = "default_max_heading_loss")]
    pub max_heading_loss_percent: f64,
}

impl Default for DiffPolicy {
    fn default() -> Self {
        Self {
            max_indexable_url_loss_percent: default_max_indexable_loss(),
            max_link_loss_percent: default_max_link_loss(),
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
fn default_max_heading_loss() -> f64 {
    15.0
}

impl Policy {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let policy: Policy = toml::from_str(&content)?;
        Ok(policy)
    }

    /// Returns true if a finding for the given rule_id and url should be excluded.
    pub fn is_excluded(&self, rule_id: &str, url: &str) -> bool {
        self.exclusions
            .iter()
            .any(|ex| ex.rule_id == rule_id && glob_match(&ex.url_pattern, url))
    }

    /// Returns true if the given severity should cause a non-zero exit code.
    pub fn should_fail(&self, severity: &str) -> bool {
        self.general
            .fail_on
            .iter()
            .any(|s| s.eq_ignore_ascii_case(severity))
    }
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
        assert!(!policy.is_excluded("CC-ORPHAN-001", "/public/page"));
        assert!(policy.should_fail("Error"));
        assert!(policy.should_fail("WARNING"));
        assert!(!policy.should_fail("info"));
    }
}
