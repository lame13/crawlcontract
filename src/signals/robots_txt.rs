use std::collections::{BTreeMap, HashMap};
use url::Url;

use crate::model::url_state::RobotsTxtStatus;

/// A parsed robots.txt with rules for a specific user-agent.
#[derive(Debug, Clone)]
pub struct RobotsTxt {
    /// Wildcard-agent rules evaluated with most-specific-match-wins semantics.
    rules: Vec<RobotsRule>,
    pub crawl_delay: Option<f64>,
    pub sitemaps: Vec<Url>,
}

#[derive(Debug, Clone)]
struct RobotsRule {
    pattern: String,
    allowed: bool,
    specificity: usize,
}

impl RobotsTxt {
    /// Parse a robots.txt body. `base_url` is used to resolve relative sitemap URLs.
    pub fn parse(body: &str, base_url: &Url) -> Self {
        let mut all_rules: BTreeMap<String, Vec<(String, bool)>> = BTreeMap::new();
        let mut current_agents: Vec<String> = Vec::new();
        let mut crawl_delay: Option<f64> = None;
        let mut sitemaps = Vec::new();
        let mut directives_started = false;

        for line in body.lines() {
            let line = match line.find('#') {
                Some(pos) => &line[..pos],
                None => line,
            };
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            let (key, value) = match line.find(':') {
                Some(pos) => (
                    line[..pos].trim().to_lowercase(),
                    line[pos + 1..].trim().to_string(),
                ),
                None => continue,
            };

            match key.as_str() {
                "user-agent" => {
                    let agent = value.to_lowercase();
                    if directives_started {
                        current_agents.clear();
                        directives_started = false;
                    }
                    current_agents.push(agent.clone());
                    all_rules.entry(agent).or_default();
                }
                "disallow" => {
                    directives_started = true;
                    if !value.is_empty() {
                        for agent in &current_agents {
                            all_rules
                                .entry(agent.clone())
                                .or_default()
                                .push((value.clone(), false));
                        }
                    }
                }
                "allow" => {
                    directives_started = true;
                    if !value.is_empty() {
                        for agent in &current_agents {
                            all_rules
                                .entry(agent.clone())
                                .or_default()
                                .push((value.clone(), true));
                        }
                    }
                }
                "sitemap" => {
                    if let Ok(url) = Url::parse(&value).or_else(|_| base_url.join(&value)) {
                        sitemaps.push(url);
                    }
                }
                "crawl-delay" => {
                    directives_started = true;
                    if current_agents.iter().any(|agent| agent == "*") {
                        crawl_delay = value
                            .parse::<f64>()
                            .ok()
                            .filter(|delay| delay.is_finite() && *delay >= 0.0);
                    }
                }
                _ => {
                    if !current_agents.is_empty() {
                        directives_started = true;
                    }
                }
            }
        }

        // Use wildcard rules if present, otherwise empty.
        let rules = all_rules.remove("*").unwrap_or_default();

        let rules = rules
            .into_iter()
            .map(|(pattern, allowed)| RobotsRule {
                specificity: pattern
                    .trim_end_matches('$')
                    .bytes()
                    .filter(|byte| *byte != b'*')
                    .count(),
                pattern,
                allowed,
            })
            .collect();

        Self {
            rules,
            crawl_delay,
            sitemaps,
        }
    }

    /// Evaluate whether a URL path is allowed for the wildcard user-agent.
    pub fn is_allowed(&self, path: &str) -> RobotsTxtStatus {
        let mut best: Option<(usize, bool)> = None;

        for rule in &self.rules {
            if !robots_pattern_matches(&rule.pattern, path) {
                continue;
            }

            best = match best {
                Some((specificity, allowed)) if specificity > rule.specificity => {
                    Some((specificity, allowed))
                }
                Some((specificity, allowed)) if specificity == rule.specificity => {
                    Some((specificity, allowed || rule.allowed))
                }
                _ => Some((rule.specificity, rule.allowed)),
            };
        }

        match best {
            Some((_, true)) => RobotsTxtStatus::Allowed,
            Some((_, false)) => RobotsTxtStatus::Blocked,
            None => RobotsTxtStatus::NoRule,
        }
    }
}

fn robots_pattern_matches(pattern: &str, path: &str) -> bool {
    let anchored_end = pattern.ends_with('$');
    let pattern = pattern.strip_suffix('$').unwrap_or(pattern);
    robots_pattern_matches_inner(
        pattern.as_bytes(),
        path.as_bytes(),
        0,
        0,
        anchored_end,
        &mut HashMap::new(),
    )
}

fn robots_pattern_matches_inner(
    pattern: &[u8],
    path: &[u8],
    pattern_index: usize,
    path_index: usize,
    anchored_end: bool,
    memo: &mut HashMap<(usize, usize), bool>,
) -> bool {
    if let Some(result) = memo.get(&(pattern_index, path_index)) {
        return *result;
    }

    let result = if pattern_index == pattern.len() {
        !anchored_end || path_index == path.len()
    } else if pattern[pattern_index] == b'*' {
        (path_index..=path.len()).any(|next_path_index| {
            robots_pattern_matches_inner(
                pattern,
                path,
                pattern_index + 1,
                next_path_index,
                anchored_end,
                memo,
            )
        })
    } else {
        path.get(path_index)
            .is_some_and(|byte| *byte == pattern[pattern_index])
            && robots_pattern_matches_inner(
                pattern,
                path,
                pattern_index + 1,
                path_index + 1,
                anchored_end,
                memo,
            )
    };

    memo.insert((pattern_index, path_index), result);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Url {
        Url::parse("https://example.test").unwrap()
    }

    #[test]
    fn parse_simple() {
        let body = r#"
User-agent: *
Disallow: /admin/
Allow: /admin/login
Sitemap: https://example.test/sitemap.xml
"#;
        let rt = RobotsTxt::parse(body, &base());
        assert_eq!(rt.sitemaps.len(), 1);
        assert_eq!(rt.is_allowed("/public/page"), RobotsTxtStatus::NoRule);
        assert_eq!(rt.is_allowed("/admin/secret"), RobotsTxtStatus::Blocked);
        assert_eq!(rt.is_allowed("/admin/login"), RobotsTxtStatus::Allowed);
    }

    #[test]
    fn empty_robots_txt() {
        let rt = RobotsTxt::parse("", &base());
        assert_eq!(rt.is_allowed("/anything"), RobotsTxtStatus::NoRule);
    }

    #[test]
    fn most_specific_wins() {
        let body = r#"
User-agent: *
Disallow: /
Allow: /public/
"#;
        let rt = RobotsTxt::parse(body, &base());
        assert_eq!(rt.is_allowed("/secret"), RobotsTxtStatus::Blocked);
        assert_eq!(rt.is_allowed("/public/page"), RobotsTxtStatus::Allowed);
    }

    #[test]
    fn allow_wins_when_match_lengths_are_equal() {
        let body = "User-agent: *\nDisallow: /page\nAllow: /page";
        let rt = RobotsTxt::parse(body, &base());
        assert_eq!(rt.is_allowed("/page"), RobotsTxtStatus::Allowed);
    }

    #[test]
    fn supports_wildcards_end_anchors_and_queries() {
        let body = "User-agent: *\nDisallow: /*?preview=*$\nDisallow: /*.pdf$";
        let rt = RobotsTxt::parse(body, &base());
        assert_eq!(
            rt.is_allowed("/page?preview=true"),
            RobotsTxtStatus::Blocked
        );
        assert_eq!(rt.is_allowed("/file.pdf"), RobotsTxtStatus::Blocked);
        assert_eq!(
            rt.is_allowed("/file.pdf?download=1"),
            RobotsTxtStatus::NoRule
        );
    }

    #[test]
    fn user_agent_after_directives_starts_a_new_group() {
        let body = "User-agent: *\nDisallow: /private\nUser-agent: otherbot\nDisallow: /public";
        let rt = RobotsTxt::parse(body, &base());
        assert_eq!(rt.is_allowed("/private"), RobotsTxtStatus::Blocked);
        assert_eq!(rt.is_allowed("/public"), RobotsTxtStatus::NoRule);
    }

    #[test]
    fn anchored_wildcard_can_backtrack_to_the_last_match() {
        let rt = RobotsTxt::parse("User-agent: *\nDisallow: /foo*bar$", &base());
        assert_eq!(rt.is_allowed("/foobarbazbar"), RobotsTxtStatus::Blocked);
    }

    #[test]
    fn invalid_crawl_delay_is_ignored() {
        let rt = RobotsTxt::parse("User-agent: *\nCrawl-delay: -1", &base());
        assert!(rt.crawl_delay.is_none());
    }
}
