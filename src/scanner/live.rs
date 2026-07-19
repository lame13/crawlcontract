use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::time::Duration;

use anyhow::Context;
use reqwest::Client;
use scraper::Html;
use url::Url;

use crate::model::snapshot::{Snapshot, Statistics};
use crate::model::url_state::{RobotsTxtStatus, UrlSource, UrlState};
use crate::scanner::html_signals::{extract_html_signals, normalize_url_key};
use crate::signals::canonical::extract_http_canonical;
use crate::signals::robots_directive::parse_x_robots_tag;
use crate::signals::robots_txt::RobotsTxt;
use crate::signals::sitemap::{collect_sitemap_urls, parse_sitemap, Sitemap};

/// Configuration for a live scan.
pub struct LiveScanConfig {
    pub start_url: Url,
    pub public_origin: Option<Url>,
    pub max_pages: usize,
    pub concurrency: usize,
    pub request_timeout: Duration,
    pub user_agent: String,
    pub crawl_delay: Option<Duration>,
}

impl Default for LiveScanConfig {
    fn default() -> Self {
        Self {
            start_url: Url::parse("https://example.test").unwrap(),
            public_origin: None,
            max_pages: 500,
            concurrency: 8,
            request_timeout: Duration::from_secs(30),
            user_agent: "crawlcontract/0.1.0 (+https://github.com/lame13/crawlcontract)"
                .to_string(),
            crawl_delay: None,
        }
    }
}

/// Fetched result for a single URL.
#[allow(dead_code)]
struct FetchedPage {
    url: Url,
    final_url: Url,
    status: u16,
    headers: Vec<(String, String)>,
    body: Option<String>,
    redirect_chain: Vec<Url>,
}

/// Scan a live site starting from the configured URL.
pub async fn scan_live(config: LiveScanConfig) -> anyhow::Result<Snapshot> {
    let origin = config.start_url.origin().ascii_serialization();
    let base_url = config.public_origin.clone().unwrap_or_else(|| {
        Url::parse(&format!("{}/", origin)).unwrap_or_else(|_| config.start_url.clone())
    });

    let client = Client::builder()
        .user_agent(&config.user_agent)
        .timeout(config.request_timeout)
        .redirect(reqwest::redirect::Policy::none()) // We track redirects manually
        .build()
        .context("building HTTP client")?;

    let mut states: BTreeMap<String, UrlState> = BTreeMap::new();
    let mut visited: BTreeSet<String> = BTreeSet::new();
    let mut queue: VecDeque<Url> = VecDeque::new();

    // 1. Fetch and parse robots.txt
    let robots_url = base_url.join("/robots.txt").unwrap();
    let robots_txt = match fetch_url(&client, &robots_url).await {
        Ok(response) if response.status == 200 => {
            response.body.map(|body| RobotsTxt::parse(&body, &base_url))
        }
        _ => None,
    };

    // 2. Discover sitemap URLs
    let mut sitemap_urls = BTreeSet::new();
    if let Some(ref rt) = robots_txt {
        for sitemap_url in &rt.sitemaps {
            if let Ok(response) = fetch_url(&client, sitemap_url).await {
                if response.status == 200 {
                    if let Some(body) = &response.body {
                        if let Ok(sitemap) = parse_sitemap(body) {
                            let urls = collect_sitemap_urls(&sitemap);
                            sitemap_urls.extend(urls);

                            // Handle sitemap index: fetch child sitemaps
                            if let Sitemap::Index(idx) = &sitemap {
                                for child_url in &idx.sitemaps {
                                    if let Ok(child_resp) = fetch_url(&client, child_url).await {
                                        if child_resp.status == 200 {
                                            if let Some(child_body) = &child_resp.body {
                                                if let Ok(child_sitemap) = parse_sitemap(child_body)
                                                {
                                                    sitemap_urls.extend(collect_sitemap_urls(
                                                        &child_sitemap,
                                                    ));
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Mark sitemap URLs in states
    for url in &sitemap_urls {
        let key = normalize_url_key(url);
        let state = states.entry(key).or_insert_with(|| {
            let mut s = UrlState::new(url.clone());
            s.add_source(UrlSource::Sitemap);
            s
        });
        state.found_in_sitemap = true;
    }

    // 3. Seed the queue with the start URL
    queue.push_back(config.start_url.clone());

    // 4. BFS crawl
    let mut pages_fetched = 0;
    let effective_delay = config.crawl_delay.or(robots_txt
        .as_ref()
        .and_then(|rt| rt.crawl_delay.map(Duration::from_secs_f64)));

    while let Some(url) = queue.pop_front() {
        if pages_fetched >= config.max_pages {
            break;
        }

        let key = normalize_url_key(&url);
        if visited.contains(&key) {
            continue;
        }

        // Check robots.txt
        if let Some(ref rt) = robots_txt {
            let status = rt.is_allowed(url.path());
            if status == RobotsTxtStatus::Blocked {
                let state = states.entry(key.clone()).or_insert_with(|| {
                    let mut s = UrlState::new(url.clone());
                    s.add_source(UrlSource::DirectScan);
                    s
                });
                state.robots_txt_status = RobotsTxtStatus::Blocked;
                visited.insert(key);
                continue;
            }
        }

        // Crawl delay
        if let Some(delay) = effective_delay {
            tokio::time::sleep(delay).await;
        }

        // Fetch the page
        let fetched = match follow_redirects(&client, &url, 10).await {
            Ok(f) => f,
            Err(e) => {
                tracing::warn!("Failed to fetch {}: {}", url, e);
                visited.insert(key);
                continue;
            }
        };

        pages_fetched += 1;
        visited.insert(key.clone());

        // Collect URLs to register (to avoid borrow conflicts)
        let mut urls_to_register: Vec<(Url, UrlSource)> = Vec::new();

        // Build or update the URL state
        {
            let state = states.entry(key).or_insert_with(|| {
                let mut s = UrlState::new(url.clone());
                s.add_source(UrlSource::DirectScan);
                s
            });

            state.http_status = Some(fetched.status);
            state.robots_txt_status = robots_txt
                .as_ref()
                .map(|rt| rt.is_allowed(url.path()))
                .unwrap_or(RobotsTxtStatus::NoRule);

            // Track redirect chain
            if fetched.redirect_chain.len() > 1 {
                state.redirect_chain = fetched.redirect_chain.clone();
                state.redirect_target = fetched.redirect_chain.last().cloned();

                if let Some(target) = &state.redirect_target {
                    urls_to_register.push((target.clone(), UrlSource::Redirect));
                }
            }

            // Extract HTTP headers
            for (name, value) in &fetched.headers {
                let name_lower = name.to_lowercase();
                match name_lower.as_str() {
                    "x-robots-tag" => {
                        state.http_x_robots_tag = parse_x_robots_tag(value);
                    }
                    "link" => {
                        if let Some(canonical) = extract_http_canonical(value, &url) {
                            state.http_canonical = Some(canonical);
                        }
                    }
                    _ => {}
                }
            }

            // Parse HTML and extract signals
            if let Some(body) = &fetched.body {
                let document = Html::parse_document(body);
                let signals = extract_html_signals(&document, &url);

                state.html_canonical = signals.canonical;
                state.html_meta_robots = signals.meta_robots;
                state.hreflang = signals.hreflang;
                state.internal_links_out = signals.internal_links.clone();
                state.word_count = Some(signals.word_count);
                state.heading_count = Some(signals.heading_count);

                // Queue discovered internal links
                for link_url in &signals.internal_links {
                    let link_key = normalize_url_key(link_url);
                    if !visited.contains(&link_key) {
                        queue.push_back(link_url.clone());
                    }
                    urls_to_register.push((link_url.clone(), UrlSource::InternalLink));
                }

                // Queue hreflang targets
                for entry in &state.hreflang {
                    let href_key = normalize_url_key(&entry.url);
                    if !visited.contains(&href_key) {
                        queue.push_back(entry.url.clone());
                    }
                    urls_to_register.push((entry.url.clone(), UrlSource::Hreflang));
                }
            }
        }

        // Register discovered URLs
        for (discovered_url, source) in urls_to_register {
            let dkey = normalize_url_key(&discovered_url);
            states.entry(dkey).or_insert_with(|| {
                let mut s = UrlState::new(discovered_url);
                s.add_source(source);
                s
            });
        }
    }

    // 5. Compute derived state
    compute_derived_state(&mut states);

    // 6. Compute reachability from the start URL
    compute_reachability(&mut states, &base_url);

    // 7. Build snapshot
    let statistics = Statistics::from_url_states(&states);
    Ok(Snapshot {
        version: "1.0".to_string(),
        tool: "crawlcontract".to_string(),
        base_url,
        public_origin: config.public_origin,
        scanned_at: crate::model::snapshot::utc_now(),
        urls: states,
        statistics,
    })
}

/// Compute effective canonical, effective robots, and indexability.
fn compute_derived_state(states: &mut BTreeMap<String, UrlState>) {
    // First pass: effective canonical
    let canonical_map: BTreeMap<String, Option<Url>> = states
        .iter()
        .map(|(key, state)| {
            let effective = state
                .html_canonical
                .as_ref()
                .or(state.http_canonical.as_ref())
                .cloned();
            (key.clone(), effective)
        })
        .collect();

    for (key, canonical) in &canonical_map {
        if let Some(state) = states.get_mut(key) {
            state.effective_canonical = canonical.clone();
        }
    }

    // Second pass: effective robots and indexability
    let keys: Vec<String> = states.keys().cloned().collect();
    for key in &keys {
        let state = states.get(key).unwrap();

        let effective_robots = crate::signals::robots_directive::effective_robots_directive(
            state.html_meta_robots.as_ref(),
            state.http_x_robots_tag.as_ref(),
        );

        let no_noindex = !effective_robots.is_noindex();
        let not_blocked = state.robots_txt_status != RobotsTxtStatus::Blocked;
        let ok_status = state.http_status.map(|s| s < 400).unwrap_or(true);
        let is_canonical = state
            .effective_canonical
            .as_ref()
            .map(|c| c == &state.url)
            .unwrap_or(true);

        let is_indexable = no_noindex && not_blocked && ok_status && is_canonical;

        let state = states.get_mut(key).unwrap();
        state.effective_robots = effective_robots;
        state.is_indexable = is_indexable;
    }
}

/// Compute reachability via BFS from entry points.
fn compute_reachability(states: &mut BTreeMap<String, UrlState>, base_url: &Url) {
    use std::collections::{HashSet, VecDeque};

    let adjacency: BTreeMap<String, Vec<String>> = states
        .iter()
        .map(|(key, state)| {
            let targets: Vec<String> = state
                .internal_links_out
                .iter()
                .map(normalize_url_key)
                .collect();
            (key.clone(), targets)
        })
        .collect();

    let mut queue = VecDeque::new();
    let mut visited = HashSet::new();

    // Seed from homepage and any directly scanned URL
    let homepage_key = normalize_url_key(base_url);
    if states.contains_key(&homepage_key) {
        queue.push_back(homepage_key.clone());
        visited.insert(homepage_key);
    }

    for (key, state) in states.iter() {
        if state.sources.contains(&UrlSource::DirectScan) && !visited.contains(key) {
            queue.push_back(key.clone());
            visited.insert(key.clone());
        }
    }

    while let Some(current) = queue.pop_front() {
        if let Some(neighbors) = adjacency.get(&current) {
            for neighbor in neighbors {
                if visited.insert(neighbor.clone()) {
                    queue.push_back(neighbor.clone());
                }
            }
        }
    }

    for key in &visited {
        if let Some(state) = states.get_mut(key) {
            state.is_reachable = true;
        }
    }
}

/// Fetch a URL and return the response metadata + body.
async fn fetch_url(client: &Client, url: &Url) -> anyhow::Result<FetchedPage> {
    let response = client
        .get(url.as_str())
        .send()
        .await
        .with_context(|| format!("fetching {url}"))?;

    let status = response.status().as_u16();
    let headers: Vec<(String, String)> = response
        .headers()
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
        .collect();

    let body = if status == 200 {
        response.text().await.ok()
    } else {
        None
    };

    Ok(FetchedPage {
        url: url.clone(),
        final_url: url.clone(),
        status,
        headers,
        body,
        redirect_chain: vec![url.clone()],
    })
}

/// Follow redirect chains manually (up to `max_hops`).
async fn follow_redirects(
    client: &Client,
    start_url: &Url,
    max_hops: usize,
) -> anyhow::Result<FetchedPage> {
    let mut current_url = start_url.clone();
    let mut chain = vec![current_url.clone()];
    let mut hops = 0;

    loop {
        let response = client
            .get(current_url.as_str())
            .send()
            .await
            .with_context(|| format!("fetching {current_url}"))?;

        let status = response.status().as_u16();

        // Follow 3xx redirects
        if (300..400).contains(&status) && hops < max_hops {
            if let Some(location) = response.headers().get("location") {
                let location_str = location.to_str().unwrap_or("");
                let next_url = current_url
                    .join(location_str)
                    .or_else(|_| Url::parse(location_str))
                    .ok();

                if let Some(next) = next_url {
                    // Detect cycles
                    if chain.contains(&next) {
                        // Return the current state — cycle detected
                        let headers: Vec<(String, String)> = response
                            .headers()
                            .iter()
                            .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
                            .collect();
                        return Ok(FetchedPage {
                            url: start_url.clone(),
                            final_url: current_url,
                            status,
                            headers,
                            body: None,
                            redirect_chain: chain,
                        });
                    }

                    chain.push(next.clone());
                    current_url = next;
                    hops += 1;
                    continue;
                }
            }
        }

        // Terminal response
        let headers: Vec<(String, String)> = response
            .headers()
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
            .collect();

        let body = if status == 200
            && response
                .headers()
                .get("content-type")
                .and_then(|ct| ct.to_str().ok())
                .map(|ct| ct.contains("text/html"))
                .unwrap_or(false)
        {
            response.text().await.ok()
        } else {
            None
        };

        return Ok(FetchedPage {
            url: start_url.clone(),
            final_url: current_url,
            status,
            headers,
            body,
            redirect_chain: chain,
        });
    }
}
