use std::collections::BTreeMap;
use url::Url;

use crate::model::url_state::RobotsTxtStatus;

/// A parsed robots.txt with rules for a specific user-agent.
#[derive(Debug, Clone)]
pub struct RobotsTxt {
    /// Maps path prefix → allowed (true) or disallowed (false).
    /// Sorted longest-first for most-specific-match-wins evaluation.
    rules: Vec<(String, bool)>,
    pub crawl_delay: Option<f64>,
    pub sitemaps: Vec<Url>,
}

impl RobotsTxt {
    /// Parse a robots.txt body. `base_url` is used to resolve relative sitemap URLs.
    pub fn parse(body: &str, base_url: &Url) -> Self {
        let mut all_rules: BTreeMap<String, Vec<(String, bool)>> = BTreeMap::new();
        let mut current_agents: Vec<String> = Vec::new();
        let mut crawl_delay: Option<f64> = None;
        let mut sitemaps = Vec::new();
        let mut in_relevant_section = false;

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
                    let is_wildcard = agent == "*";
                    if current_agents.is_empty() || is_wildcard != in_relevant_section {
                        // Start a new section
                        current_agents.clear();
                    }
                    current_agents.push(agent.clone());
                    in_relevant_section = is_wildcard;
                    for agent in &current_agents {
                        all_rules.entry(agent.clone()).or_default();
                    }
                }
                "disallow" => {
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
                "crawl-delay" if current_agents.iter().any(|a| a == "*") => {
                    crawl_delay = value.parse().ok();
                }
                _ => {}
            }
        }

        // Use wildcard rules if present, otherwise empty.
        let rules = all_rules.remove("*").unwrap_or_default();

        // Sort by path length descending (most specific first).
        let mut rules = rules;
        rules.sort_by_key(|b| std::cmp::Reverse(b.0.len()));

        Self {
            rules,
            crawl_delay,
            sitemaps,
        }
    }

    /// Evaluate whether a URL path is allowed for the wildcard user-agent.
    pub fn is_allowed(&self, path: &str) -> RobotsTxtStatus {
        for (rule_path, allowed) in &self.rules {
            if path == rule_path || path.starts_with(rule_path) {
                return if *allowed {
                    RobotsTxtStatus::Allowed
                } else {
                    RobotsTxtStatus::Blocked
                };
            }
        }
        RobotsTxtStatus::NoRule
    }
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
}
