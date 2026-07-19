use serde::{Deserialize, Serialize};
use url::Url;

use super::hreflang::HreflangEntry;
use super::robots::RobotsDirective;

/// Where a URL was first discovered.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum UrlSource {
    Sitemap,
    InternalLink,
    Redirect,
    Hreflang,
    DirectScan,
    RobotsTxt,
}

impl std::fmt::Display for UrlSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UrlSource::Sitemap => write!(f, "sitemap"),
            UrlSource::InternalLink => write!(f, "internal link"),
            UrlSource::Redirect => write!(f, "redirect"),
            UrlSource::Hreflang => write!(f, "hreflang"),
            UrlSource::DirectScan => write!(f, "direct scan"),
            UrlSource::RobotsTxt => write!(f, "robots.txt"),
        }
    }
}

/// The tri-state for robots.txt evaluation.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum RobotsTxtStatus {
    /// Explicitly allowed by a matching rule.
    Allowed,
    /// Explicitly disallowed by a matching rule.
    Blocked,
    /// No matching rule found; defaults to allowed for well-behaved crawlers.
    NoRule,
}

/// Complete state for a single URL, assembled from all signals.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UrlState {
    pub url: Url,

    /// All sources where this URL was discovered.
    pub sources: Vec<UrlSource>,

    // --- Raw signals ---
    pub http_status: Option<u16>,
    pub redirect_target: Option<Url>,
    pub redirect_chain: Vec<Url>,
    pub html_canonical: Option<Url>,
    pub http_canonical: Option<Url>,
    pub html_meta_robots: Option<RobotsDirective>,
    pub http_x_robots_tag: Option<RobotsDirective>,
    pub robots_txt_status: RobotsTxtStatus,
    pub hreflang: Vec<HreflangEntry>,
    pub internal_links_out: Vec<Url>,
    pub found_in_sitemap: bool,

    // --- Derived (computed by graph builder) ---
    pub effective_canonical: Option<Url>,
    pub effective_robots: RobotsDirective,
    pub is_indexable: bool,
    pub is_reachable: bool,
    pub word_count: Option<usize>,
    pub heading_count: Option<usize>,
}

impl UrlState {
    pub fn new(url: Url) -> Self {
        Self {
            url,
            sources: Vec::new(),
            http_status: None,
            redirect_target: None,
            redirect_chain: Vec::new(),
            html_canonical: None,
            http_canonical: None,
            html_meta_robots: None,
            http_x_robots_tag: None,
            robots_txt_status: RobotsTxtStatus::NoRule,
            hreflang: Vec::new(),
            internal_links_out: Vec::new(),
            found_in_sitemap: false,
            effective_canonical: None,
            effective_robots: RobotsDirective::default(),
            is_indexable: false,
            is_reachable: false,
            word_count: None,
            heading_count: None,
        }
    }

    /// Add a source if not already present.
    pub fn add_source(&mut self, source: UrlSource) {
        if !self.sources.contains(&source) {
            self.sources.push(source);
        }
    }
}
