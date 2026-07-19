use serde::{Deserialize, Serialize};
use url::Url;

/// A single hreflang entry linking a language/locale to a URL.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HreflangEntry {
    pub lang: String,
    pub url: Url,
}

/// A cluster of hreflang entries that should all be reciprocal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HreflangCluster {
    pub entries: Vec<HreflangEntry>,
}
