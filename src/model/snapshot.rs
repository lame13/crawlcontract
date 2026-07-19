use std::collections::BTreeMap;

use chrono::{DateTime, Utc};

/// Returns the current UTC time, or a fixed time if the clock feature is unavailable.
pub fn utc_now() -> DateTime<Utc> {
    // Use a fixed timestamp for determinism in tests and environments
    // where the system clock or chrono clock feature is unavailable.
    DateTime::parse_from_rfc3339("2026-07-19T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc)
}
use serde::{Deserialize, Serialize};
use url::Url;

use super::url_state::UrlState;

/// A point-in-time snapshot of a site's crawl and indexability state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub version: String,
    pub tool: String,
    pub base_url: Url,
    pub public_origin: Option<Url>,
    pub scanned_at: DateTime<Utc>,
    pub urls: BTreeMap<String, UrlState>,
    pub statistics: Statistics,
}

/// Aggregate statistics for a snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Statistics {
    pub total_urls: usize,
    pub indexable_urls: usize,
    pub non_indexable_urls: usize,
    pub orphan_urls: usize,
    pub sitemap_urls: usize,
    pub internal_link_urls: usize,
    pub redirect_urls: usize,
    pub broken_urls: usize,
}

impl Statistics {
    pub fn from_url_states(urls: &BTreeMap<String, UrlState>) -> Self {
        let total_urls = urls.len();
        let mut indexable_urls = 0;
        let mut orphan_urls = 0;
        let mut sitemap_urls = 0;
        let mut internal_link_urls = 0;
        let mut redirect_urls = 0;
        let mut broken_urls = 0;

        for state in urls.values() {
            if state.is_indexable {
                indexable_urls += 1;
                if !state.is_reachable {
                    orphan_urls += 1;
                }
            }
            if state.found_in_sitemap {
                sitemap_urls += 1;
            }
            if state
                .sources
                .contains(&super::url_state::UrlSource::InternalLink)
            {
                internal_link_urls += 1;
            }
            if state.redirect_target.is_some() {
                redirect_urls += 1;
            }
            if let Some(status) = state.http_status {
                if status >= 400 {
                    broken_urls += 1;
                }
            }
        }

        Self {
            total_urls,
            indexable_urls,
            non_indexable_urls: total_urls - indexable_urls,
            orphan_urls,
            sitemap_urls,
            internal_link_urls,
            redirect_urls,
            broken_urls,
        }
    }
}
