use std::collections::{BTreeMap, HashMap};
use url::Url;

use crate::model::url_state::RobotsTxtStatus;

/// A parsed robots.txt with rules for a specific user-agent.
#[derive(Debug, Clone)]
pub struct RobotsTxt {
    /// Rules from the group that applies to [`Self::user_agent`], evaluated with
    /// most-specific-match-wins semantics.
    rules: Vec<RobotsRule>,
    pub crawl_delay: Option<f64>,
    pub sitemaps: Vec<Url>,
    /// The user-agent string this parse was evaluated for.
    pub user_agent: String,
    /// The `User-agent` group that was selected, or `*` when no group matched.
    pub matched_agent: String,
}

#[derive(Debug, Clone)]
struct RobotsRule {
    pattern: String,
    allowed: bool,
    specificity: usize,
}

impl RobotsTxt {
    /// Parse a robots.txt body for `user_agent`.
    ///
    /// `base_url` is used to resolve relative sitemap URLs. Rule groups are
    /// selected by the longest case-insensitive token match against
    /// `user_agent`, and the `*`
    /// group is the fallback when no group names this agent.
    pub fn parse(body: &str, base_url: &Url, user_agent: &str) -> Self {
        let mut groups: BTreeMap<String, Vec<(String, bool)>> = BTreeMap::new();
        let mut delays: BTreeMap<String, f64> = BTreeMap::new();
        let mut current_agents: Vec<String> = Vec::new();
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
                    let agent = agent.trim();
                    if agent.is_empty() {
                        continue;
                    }
                    if directives_started {
                        current_agents.clear();
                        directives_started = false;
                    }
                    current_agents.push(agent.to_string());
                    // Register the group even when it carries no rules: matching
                    // an empty group means "no restrictions" rather than
                    // falling back to the wildcard group.
                    groups.entry(agent.to_string()).or_default();
                }
                "disallow" => {
                    directives_started = true;
                    if !value.is_empty() {
                        for agent in &current_agents {
                            groups
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
                            groups
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
                    if let Some(seconds) = value
                        .parse::<f64>()
                        .ok()
                        .filter(|delay| delay.is_finite() && *delay >= 0.0)
                    {
                        for agent in &current_agents {
                            delays.insert(agent.clone(), seconds);
                        }
                    }
                }
                _ => {
                    if !current_agents.is_empty() {
                        directives_started = true;
                    }
                }
            }
        }

        let matched_agent = select_group(&groups, user_agent);
        let rules = groups.remove(&matched_agent).unwrap_or_default();
        let crawl_delay = delays.get(&matched_agent).copied();

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
            user_agent: user_agent.to_string(),
            matched_agent,
        }
    }

    /// Evaluate whether a URL path is allowed for the configured user-agent.
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

/// Pick the rule group that applies to `user_agent`.
///
/// The longest `User-agent` token contained in the crawler's user-agent string
/// wins; `*` is used when no named group matches.
fn select_group(groups: &BTreeMap<String, Vec<(String, bool)>>, user_agent: &str) -> String {
    let haystack = user_agent.to_lowercase();
    let mut best: Option<&str> = None;

    for agent in groups.keys() {
        if agent == "*" || agent.is_empty() || !haystack.contains(agent.as_str()) {
            continue;
        }
        best = match best {
            Some(current) if current.len() >= agent.len() => Some(current),
            _ => Some(agent.as_str()),
        };
    }

    best.unwrap_or("*").to_string()
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

    /// Parse with the default crawlcontract user-agent.
    fn parse(body: &str) -> RobotsTxt {
        RobotsTxt::parse(
            body,
            &base(),
            "crawlcontract/0.4.0 (+https://example.test/bot)",
        )
    }

    /// Parse as an arbitrary user-agent.
    fn parse_as(body: &str, user_agent: &str) -> RobotsTxt {
        RobotsTxt::parse(body, &base(), user_agent)
    }

    #[test]
    fn parse_simple() {
        let body = r#"
User-agent: *
Disallow: /admin/
Allow: /admin/login
Sitemap: https://example.test/sitemap.xml
"#;
        let rt = parse(body);
        assert_eq!(rt.sitemaps.len(), 1);
        assert_eq!(rt.matched_agent, "*");
        assert_eq!(rt.is_allowed("/public/page"), RobotsTxtStatus::NoRule);
        assert_eq!(rt.is_allowed("/admin/secret"), RobotsTxtStatus::Blocked);
        assert_eq!(rt.is_allowed("/admin/login"), RobotsTxtStatus::Allowed);
    }

    #[test]
    fn empty_robots_txt() {
        let rt = parse("");
        assert_eq!(rt.is_allowed("/anything"), RobotsTxtStatus::NoRule);
    }

    #[test]
    fn most_specific_wins() {
        let body = r#"
User-agent: *
Disallow: /
Allow: /public/
"#;
        let rt = parse(body);
        assert_eq!(rt.is_allowed("/secret"), RobotsTxtStatus::Blocked);
        assert_eq!(rt.is_allowed("/public/page"), RobotsTxtStatus::Allowed);
    }

    #[test]
    fn allow_wins_when_match_lengths_are_equal() {
        let body = "User-agent: *\nDisallow: /page\nAllow: /page";
        let rt = parse(body);
        assert_eq!(rt.is_allowed("/page"), RobotsTxtStatus::Allowed);
    }

    #[test]
    fn supports_wildcards_end_anchors_and_queries() {
        let body = "User-agent: *\nDisallow: /*?preview=*$\nDisallow: /*.pdf$";
        let rt = parse(body);
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
        let rt = parse(body);
        assert_eq!(rt.matched_agent, "*");
        assert_eq!(rt.is_allowed("/private"), RobotsTxtStatus::Blocked);
        assert_eq!(rt.is_allowed("/public"), RobotsTxtStatus::NoRule);
    }

    #[test]
    fn anchored_wildcard_can_backtrack_to_the_last_match() {
        let rt = parse("User-agent: *\nDisallow: /foo*bar$");
        assert_eq!(rt.is_allowed("/foobarbazbar"), RobotsTxtStatus::Blocked);
    }

    #[test]
    fn invalid_crawl_delay_is_ignored() {
        let rt = parse("User-agent: *\nCrawl-delay: -1");
        assert!(rt.crawl_delay.is_none());
    }

    #[test]
    fn named_group_replaces_the_wildcard_group_for_a_matching_agent() {
        let body = "User-agent: *\nDisallow: /private\n\n\
                    User-agent: crawlcontract\nDisallow: /for-crawlcontract-only";
        let rt = parse(body);
        assert_eq!(rt.matched_agent, "crawlcontract");
        // The named group applies, so the wildcard rule does not.
        assert_eq!(rt.is_allowed("/private"), RobotsTxtStatus::NoRule);
        assert_eq!(
            rt.is_allowed("/for-crawlcontract-only"),
            RobotsTxtStatus::Blocked
        );

        // A different crawler still falls back to the wildcard group.
        let other = parse_as(body, "Googlebot/2.1");
        assert_eq!(other.matched_agent, "*");
        assert_eq!(other.is_allowed("/private"), RobotsTxtStatus::Blocked);
        assert_eq!(
            other.is_allowed("/for-crawlcontract-only"),
            RobotsTxtStatus::NoRule
        );
    }

    #[test]
    fn longest_matching_agent_token_wins() {
        let body = "User-agent: crawl\nDisallow: /short\n\n\
                    User-agent: crawlcontract\nDisallow: /long";
        let rt = parse(body);
        assert_eq!(rt.matched_agent, "crawlcontract");
        assert_eq!(rt.is_allowed("/long"), RobotsTxtStatus::Blocked);
        // The shorter token's group must not leak into the longer one.
        assert_eq!(rt.is_allowed("/short"), RobotsTxtStatus::NoRule);
    }

    #[test]
    fn empty_named_group_does_not_inherit_wildcard_rules() {
        let body = "User-agent: *\nDisallow: /\n\nUser-agent: crawlcontract";
        let rt = parse(body);
        assert_eq!(rt.matched_agent, "crawlcontract");
        assert_eq!(rt.is_allowed("/anything"), RobotsTxtStatus::NoRule);
    }

    #[test]
    fn crawl_delay_comes_from_the_matched_group() {
        let body = "User-agent: *\nCrawl-delay: 1\n\nUser-agent: crawlcontract\nCrawl-delay: 3";
        assert_eq!(parse(body).crawl_delay, Some(3.0));
        assert_eq!(parse_as(body, "Googlebot").crawl_delay, Some(1.0));
    }

    #[test]
    fn group_without_crawl_delay_does_not_inherit_the_wildcard_delay() {
        let body = "User-agent: *\nCrawl-delay: 5\n\nUser-agent: crawlcontract\nDisallow: /x";
        let rt = parse(body);
        assert_eq!(rt.matched_agent, "crawlcontract");
        assert!(rt.crawl_delay.is_none());
    }
}
