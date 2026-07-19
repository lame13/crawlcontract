use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::time::Duration;

use anyhow::Context;
use futures_util::{stream, StreamExt};
use reqwest::Client;
use scraper::Html;
use url::Url;

use crate::graph::reachability::compute_reachability;
use crate::graph::state::compute_derived_state;
use crate::model::snapshot::{Snapshot, Statistics};
use crate::model::url_state::{RobotsTxtStatus, UrlSource, UrlState};
use crate::scanner::html_signals::{extract_html_signals_with_alias, normalize_url_key};
use crate::signals::canonical::extract_http_canonical;
use crate::signals::robots_directive::parse_x_robots_tag;
use crate::signals::robots_txt::RobotsTxt;
use crate::signals::sitemap::{collect_sitemap_urls, parse_sitemap, Sitemap};

const MAX_CRAWL_DELAY: Duration = Duration::from_secs(60);

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
            user_agent: format!(
                "crawlcontract/{} (+https://github.com/lame13/crawlcontract)",
                env!("CARGO_PKG_VERSION")
            ),
            crawl_delay: None,
        }
    }
}

/// Fetched result for a single URL.
struct FetchedPage {
    final_url: Url,
    initial_status: u16,
    status: u16,
    headers: Vec<(String, String)>,
    body: Option<String>,
    redirect_chain: Vec<Url>,
}

/// Scan a live site starting from the configured URL.
pub async fn scan_live(config: LiveScanConfig) -> anyhow::Result<Snapshot> {
    if config.max_pages == 0 {
        anyhow::bail!("max_pages must be greater than zero");
    }
    if config.concurrency == 0 {
        anyhow::bail!("concurrency must be greater than zero");
    }

    let crawl_origin = origin_url(&config.start_url);
    let base_url = config
        .public_origin
        .as_ref()
        .map(origin_url)
        .unwrap_or_else(|| crawl_origin.clone());
    let entry_url = remap_origin(&config.start_url, &crawl_origin, &base_url);
    let entry_key = normalize_url_key(&entry_url);

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
    let robots_url = crawl_origin.join("/robots.txt").expect("valid origin URL");
    let robots_txt = match fetch_url(&client, &robots_url).await {
        Ok(response) if response.status == 200 => {
            response.body.map(|body| RobotsTxt::parse(&body, &base_url))
        }
        Ok(_) => None,
        Err(error) => return Err(error).context("fetching robots.txt"),
    };
    let effective_delay = resolve_crawl_delay(
        config.crawl_delay,
        robots_txt.as_ref().and_then(|robots| robots.crawl_delay),
    )?;

    // 2. Discover sitemap URLs
    let mut sitemap_urls = BTreeSet::new();
    let sitemap_locations: Vec<Url> = robots_txt
        .as_ref()
        .map(|robots| robots.sitemaps.clone())
        .filter(|locations| !locations.is_empty())
        .unwrap_or_else(|| vec![base_url.join("/sitemap.xml").expect("valid origin URL")])
        .into_iter()
        .map(|url| remap_origin(&url, &crawl_origin, &base_url))
        .collect();
    let has_declared_sitemaps = robots_txt
        .as_ref()
        .is_some_and(|robots| !robots.sitemaps.is_empty());

    for sitemap_url in &sitemap_locations {
        let fetch_url_value = remap_origin(sitemap_url, &base_url, &crawl_origin);
        match fetch_url(&client, &fetch_url_value).await {
            Ok(response) if response.status == 200 => {
                if let Some(body) = &response.body {
                    let sitemap = parse_sitemap(body)
                        .with_context(|| format!("parsing sitemap {sitemap_url}"))?;
                    let urls = collect_sitemap_urls(&sitemap)
                        .into_iter()
                        .map(|url| remap_origin(&url, &crawl_origin, &base_url));
                    sitemap_urls.extend(urls);

                    // Handle one sitemap-index level.
                    if let Sitemap::Index(idx) = &sitemap {
                        for child_url in &idx.sitemaps {
                            let child_url = remap_origin(child_url, &crawl_origin, &base_url);
                            let child_fetch_url =
                                remap_origin(&child_url, &base_url, &crawl_origin);
                            let child_resp = fetch_url(&client, &child_fetch_url)
                                .await
                                .with_context(|| format!("fetching child sitemap {child_url}"))?;
                            if child_resp.status != 200 {
                                anyhow::bail!(
                                    "child sitemap {child_url} returned HTTP {}",
                                    child_resp.status
                                );
                            }
                            if let Some(child_body) = &child_resp.body {
                                let child_sitemap =
                                    parse_sitemap(child_body).with_context(|| {
                                        format!("parsing child sitemap {child_url}")
                                    })?;
                                sitemap_urls.extend(
                                    collect_sitemap_urls(&child_sitemap)
                                        .into_iter()
                                        .map(|url| remap_origin(&url, &crawl_origin, &base_url)),
                                );
                            }
                        }
                    }
                }
            }
            Ok(response) if has_declared_sitemaps => {
                anyhow::bail!(
                    "declared sitemap {sitemap_url} returned HTTP {}",
                    response.status
                );
            }
            Ok(_) => {}
            Err(error) if has_declared_sitemaps => {
                return Err(error).with_context(|| format!("fetching sitemap {sitemap_url}"));
            }
            Err(_) => {}
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
        state.add_source(UrlSource::Sitemap);
        state.found_in_sitemap = true;
    }

    // 3. Seed the queue with the requested entry page, then same-origin sitemap pages.
    queue.push_back(entry_url.clone());
    for sitemap_url in &sitemap_urls {
        if sitemap_url.origin() == base_url.origin() && sitemap_url != &entry_url {
            queue.push_back(sitemap_url.clone());
        }
    }

    // 4. BFS crawl
    let mut pages_fetched = 0;
    let mut has_fetched_page = false;

    while !queue.is_empty() && pages_fetched < config.max_pages {
        let batch_size = if effective_delay.is_some() {
            1
        } else {
            config.concurrency
        };
        let mut batch = Vec::new();

        while batch.len() < batch_size && pages_fetched < config.max_pages {
            let Some(url) = queue.pop_front() else {
                break;
            };
            let key = normalize_url_key(&url);
            if !visited.insert(key.clone()) {
                continue;
            }

            let robots_status = robots_txt
                .as_ref()
                .map(|robots| robots.is_allowed(path_and_query(&url)))
                .unwrap_or(RobotsTxtStatus::NoRule);
            let state = states
                .entry(key.clone())
                .or_insert_with(|| UrlState::new(url.clone()));
            state.add_source(UrlSource::DirectScan);
            state.robots_txt_status = robots_status;

            if robots_status == RobotsTxtStatus::Blocked {
                if key == entry_key {
                    anyhow::bail!("entry URL {entry_url} is blocked by robots.txt");
                }
                continue;
            }

            pages_fetched += 1;
            batch.push(url);
        }

        if batch.is_empty() {
            continue;
        }
        if has_fetched_page {
            if let Some(delay) = effective_delay {
                tokio::time::sleep(delay).await;
            }
        }

        let mut fetched_pages = stream::iter(batch)
            .map(|url| {
                let client = client.clone();
                let crawl_origin = crawl_origin.clone();
                let base_url = base_url.clone();
                async move {
                    let request_url = remap_origin(&url, &base_url, &crawl_origin);
                    let fetched =
                        follow_redirects(&client, &request_url, 10, &crawl_origin, &base_url).await;
                    (url, fetched)
                }
            })
            .buffer_unordered(batch_size)
            .collect::<Vec<_>>()
            .await;
        has_fetched_page = true;
        fetched_pages.sort_by(|(left, _), (right, _)| left.as_str().cmp(right.as_str()));

        for (requested_url, fetched) in fetched_pages {
            let fetched = match fetched {
                Ok(fetched) => fetched,
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("fetching discovered page {requested_url}"));
                }
            };

            if normalize_url_key(&requested_url) == entry_key {
                if fetched.status != 200 {
                    anyhow::bail!(
                        "entry URL {entry_url} did not resolve to HTTP 200 (terminal status: \
                         HTTP {})",
                        fetched.status
                    );
                }
                if fetched.body.is_none() {
                    anyhow::bail!("entry URL {entry_url} returned HTTP 200 without an HTML body");
                }
            }

            let requested_key = normalize_url_key(&requested_url);
            let redirect_chain: Vec<Url> = fetched
                .redirect_chain
                .iter()
                .map(|url| remap_origin(url, &crawl_origin, &base_url))
                .collect();
            let final_url = remap_origin(&fetched.final_url, &crawl_origin, &base_url);
            let redirected = redirect_chain.len() > 1;

            if let Some(state) = states.get_mut(&requested_key) {
                state.http_status = Some(fetched.initial_status);
                if redirected {
                    state.redirect_chain = redirect_chain.clone();
                    state.redirect_target = redirect_chain.last().cloned();
                }
            }

            let content_key = normalize_url_key(&final_url);
            let state = states
                .entry(content_key.clone())
                .or_insert_with(|| UrlState::new(final_url.clone()));
            state.add_source(if redirected {
                UrlSource::Redirect
            } else {
                UrlSource::DirectScan
            });
            state.http_status = Some(fetched.status);
            state.robots_txt_status = robots_txt
                .as_ref()
                .map(|robots| robots.is_allowed(path_and_query(&final_url)))
                .unwrap_or(RobotsTxtStatus::NoRule);

            for (name, value) in &fetched.headers {
                match name.to_ascii_lowercase().as_str() {
                    "x-robots-tag" => {
                        if let Some(parsed) = parse_x_robots_tag(value) {
                            state.http_x_robots_tag = Some(match state.http_x_robots_tag.take() {
                                Some(current) => current.merge(&parsed),
                                None => parsed,
                            });
                        }
                    }
                    "link" => {
                        if let Some(canonical) = extract_http_canonical(value, &final_url) {
                            state.http_canonical =
                                Some(remap_origin(&canonical, &crawl_origin, &base_url));
                        }
                    }
                    _ => {}
                }
            }

            let mut urls_to_register: Vec<(Url, UrlSource)> = Vec::new();
            if let Some(canonical) = state.http_canonical.clone() {
                let canonical_key = normalize_url_key(&canonical);
                if canonical.origin() == base_url.origin() && !visited.contains(&canonical_key) {
                    queue.push_back(canonical.clone());
                }
                urls_to_register.push((canonical, UrlSource::Canonical));
            }
            if let Some(body) = &fetched.body {
                let document = Html::parse_document(body);
                let signals =
                    extract_html_signals_with_alias(&document, &final_url, Some(&crawl_origin));

                state.html_canonical = signals.canonical;
                state.html_meta_robots = signals.meta_robots;
                state.hreflang = signals.hreflang;
                state.internal_links_out = signals.internal_links.clone();
                state.word_count = Some(signals.word_count);
                state.heading_count = Some(signals.heading_count);

                if let Some(canonical) = &state.html_canonical {
                    let canonical_key = normalize_url_key(canonical);
                    if canonical.origin() == base_url.origin() && !visited.contains(&canonical_key)
                    {
                        queue.push_back(canonical.clone());
                    }
                    urls_to_register.push((canonical.clone(), UrlSource::Canonical));
                }

                for link_url in &signals.internal_links {
                    let link_key = normalize_url_key(link_url);
                    if !visited.contains(&link_key) {
                        queue.push_back(link_url.clone());
                    }
                    urls_to_register.push((link_url.clone(), UrlSource::InternalLink));
                }

                for entry in &state.hreflang {
                    let href_key = normalize_url_key(&entry.url);
                    if entry.url.origin() == base_url.origin() && !visited.contains(&href_key) {
                        queue.push_back(entry.url.clone());
                    }
                    urls_to_register.push((entry.url.clone(), UrlSource::Hreflang));
                }
            }

            if redirected {
                visited.insert(content_key);
            }
            for (discovered_url, source) in urls_to_register {
                let key = normalize_url_key(&discovered_url);
                let state = states
                    .entry(key)
                    .or_insert_with(|| UrlState::new(discovered_url));
                state.add_source(source);
            }
        }
    }

    if pages_fetched == config.max_pages
        && queue.iter().any(|url| {
            !visited.contains(&normalize_url_key(url))
                && robots_txt
                    .as_ref()
                    .map(|robots| robots.is_allowed(path_and_query(url)))
                    .unwrap_or(RobotsTxtStatus::NoRule)
                    != RobotsTxtStatus::Blocked
        })
    {
        anyhow::bail!(
            "page limit of {} reached before the live scan completed; increase --max-pages",
            config.max_pages
        );
    }

    // 5. Compute derived state
    compute_derived_state(&mut states);

    // 6. Compute reachability from the start URL
    compute_reachability(&mut states, &entry_url);

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
        Some(
            response
                .text()
                .await
                .with_context(|| format!("reading response body from {url}"))?,
        )
    } else {
        None
    };

    Ok(FetchedPage {
        final_url: url.clone(),
        initial_status: status,
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
    crawl_origin: &Url,
    public_origin: &Url,
) -> anyhow::Result<FetchedPage> {
    let mut current_url = start_url.clone();
    let mut chain = vec![current_url.clone()];
    let mut hops = 0;
    let mut initial_status = None;

    loop {
        let response = client
            .get(current_url.as_str())
            .send()
            .await
            .with_context(|| format!("fetching {current_url}"))?;

        let status = response.status().as_u16();
        initial_status.get_or_insert(status);

        // Follow 3xx redirects
        if (300..400).contains(&status) && hops < max_hops {
            if let Some(location) = response.headers().get("location") {
                let location_str = location.to_str().unwrap_or("");
                let next_url = current_url
                    .join(location_str)
                    .or_else(|_| Url::parse(location_str))
                    .ok();

                if let Some(mut next) = next_url {
                    if next.origin() == public_origin.origin() {
                        next = remap_origin(&next, public_origin, crawl_origin);
                    }

                    if next.origin() != crawl_origin.origin() {
                        chain.push(next.clone());
                        let headers: Vec<(String, String)> = response
                            .headers()
                            .iter()
                            .map(|(key, value)| {
                                (key.to_string(), value.to_str().unwrap_or("").to_string())
                            })
                            .collect();
                        return Ok(FetchedPage {
                            final_url: current_url,
                            initial_status: initial_status.unwrap_or(status),
                            status,
                            headers,
                            body: None,
                            redirect_chain: chain,
                        });
                    }

                    // Detect cycles
                    if chain.contains(&next) {
                        chain.push(next);
                        let headers: Vec<(String, String)> = response
                            .headers()
                            .iter()
                            .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
                            .collect();
                        return Ok(FetchedPage {
                            final_url: current_url,
                            initial_status: initial_status.unwrap_or(status),
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
                .map(is_html_content_type)
                .unwrap_or(false)
        {
            Some(
                response
                    .text()
                    .await
                    .with_context(|| format!("reading response body from {current_url}"))?,
            )
        } else {
            None
        };

        return Ok(FetchedPage {
            final_url: current_url,
            initial_status: initial_status.unwrap_or(status),
            status,
            headers,
            body,
            redirect_chain: chain,
        });
    }
}

fn origin_url(url: &Url) -> Url {
    let mut origin = url.clone();
    origin.set_path("/");
    origin.set_query(None);
    origin.set_fragment(None);
    origin
}

fn remap_origin(url: &Url, from: &Url, to: &Url) -> Url {
    if url.origin() != from.origin() {
        return url.clone();
    }

    let mut mapped = to.clone();
    mapped.set_path(url.path());
    mapped.set_query(url.query());
    mapped.set_fragment(url.fragment());
    mapped
}

fn path_and_query(url: &Url) -> &str {
    &url[url::Position::BeforePath..url::Position::AfterQuery]
}

fn is_html_content_type(value: &str) -> bool {
    matches!(
        value
            .split(';')
            .next()
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase()
            .as_str(),
        "text/html" | "application/xhtml+xml"
    )
}

fn resolve_crawl_delay(
    configured: Option<Duration>,
    robots_seconds: Option<f64>,
) -> anyhow::Result<Option<Duration>> {
    let delay = match configured {
        Some(delay) => Some(delay),
        None => robots_seconds
            .map(Duration::try_from_secs_f64)
            .transpose()
            .context("invalid robots.txt Crawl-delay")?,
    };

    if delay.is_some_and(|delay| delay > MAX_CRAWL_DELAY) {
        anyhow::bail!(
            "crawl delay exceeds the supported maximum of {} seconds",
            MAX_CRAWL_DELAY.as_secs()
        );
    }
    Ok(delay)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{SocketAddr, TcpListener};
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    struct TestResponse {
        status: &'static str,
        content_type: &'static str,
        headers: String,
        body: String,
    }

    impl TestResponse {
        fn new(status: &'static str, content_type: &'static str, body: impl Into<String>) -> Self {
            Self {
                status,
                content_type,
                headers: String::new(),
                body: body.into(),
            }
        }

        fn with_header(mut self, name: &str, value: &str) -> Self {
            self.headers.push_str(name);
            self.headers.push_str(": ");
            self.headers.push_str(value);
            self.headers.push_str("\r\n");
            self
        }
    }

    struct TestServer {
        address: SocketAddr,
        stop_tx: Option<mpsc::Sender<()>>,
        thread: Option<thread::JoinHandle<()>>,
    }

    impl TestServer {
        fn start(
            handler: impl Fn(&str, SocketAddr) -> Option<TestResponse> + Send + 'static,
        ) -> std::io::Result<Self> {
            let listener = TcpListener::bind("127.0.0.1:0")?;
            listener.set_nonblocking(true)?;
            let address = listener.local_addr()?;
            let (stop_tx, stop_rx) = mpsc::channel();
            let thread = thread::spawn(move || loop {
                if !matches!(stop_rx.try_recv(), Err(mpsc::TryRecvError::Empty)) {
                    break;
                }
                let (mut stream, _) = match listener.accept() {
                    Ok(connection) => connection,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                    Err(error) => panic!("accepting test connection: {error}"),
                };
                let mut request = [0_u8; 4096];
                let count = stream.read(&mut request).unwrap();
                let request = String::from_utf8_lossy(&request[..count]);
                let path = request
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .unwrap();

                let Some(response) = handler(path, address) else {
                    continue;
                };
                let wire_response = format!(
                    "HTTP/1.1 {}\r\nContent-Type: {}\r\n{}Content-Length: {}\r\n\
                     Connection: close\r\n\r\n{}",
                    response.status,
                    response.content_type,
                    response.headers,
                    response.body.len(),
                    response.body
                );
                stream.write_all(wire_response.as_bytes()).unwrap();
            });

            Ok(Self {
                address,
                stop_tx: Some(stop_tx),
                thread: Some(thread),
            })
        }

        fn url(&self, path: &str) -> Url {
            Url::parse(&format!("http://{}{}", self.address, path)).unwrap()
        }
    }

    impl Drop for TestServer {
        fn drop(&mut self) {
            if let Some(stop_tx) = self.stop_tx.take() {
                let _ = stop_tx.send(());
            }
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }

    #[test]
    fn remaps_only_the_configured_origin() {
        let preview = Url::parse("https://preview.test/").unwrap();
        let public = Url::parse("https://example.test/").unwrap();
        let page = Url::parse("https://preview.test/path?q=1#part").unwrap();
        assert_eq!(
            remap_origin(&page, &preview, &public).as_str(),
            "https://example.test/path?q=1#part"
        );

        let external = Url::parse("https://external.test/path").unwrap();
        assert_eq!(remap_origin(&external, &preview, &public), external);
    }

    #[test]
    fn recognizes_html_media_types_case_insensitively() {
        assert!(is_html_content_type("Text/HTML; charset=UTF-8"));
        assert!(is_html_content_type("application/xhtml+xml"));
        assert!(!is_html_content_type("application/json"));
    }

    #[test]
    fn rejects_crawl_delays_that_could_stall_the_scan() {
        assert_eq!(
            resolve_crawl_delay(None, Some(1.5)).unwrap(),
            Some(Duration::from_millis(1500))
        );
        assert!(resolve_crawl_delay(None, Some(61.0)).is_err());
        assert!(resolve_crawl_delay(None, Some(1.0e300)).is_err());
        assert!(resolve_crawl_delay(Some(Duration::from_secs(61)), None).is_err());
    }

    #[tokio::test]
    async fn live_scan_maps_preview_requests_to_public_url_states() {
        let server = match TestServer::start(move |path, address| {
            let preview_origin = format!("http://{address}");
            Some(match path {
                "/robots.txt" => TestResponse::new(
                    "200 OK",
                    "text/plain",
                    format!("User-agent: *\nAllow: /\nSitemap: {preview_origin}/sitemap.xml"),
                ),
                "/sitemap.xml" => TestResponse::new(
                    "200 OK",
                    "application/xml",
                    format!(
                        "<urlset><url><loc>{preview_origin}/</loc></url>\
                         <url><loc>{preview_origin}/about</loc></url></urlset>"
                    ),
                ),
                "/" => TestResponse::new(
                    "200 OK",
                    "text/html",
                    format!(
                        "<html><head><link rel=\"canonical\" href=\"{preview_origin}/\">\
                         <link rel=\"alternate\" hreflang=\"en\" \
                         href=\"{preview_origin}/\"></head><body>\
                         <a href=\"{preview_origin}/about\">About</a></body></html>"
                    ),
                )
                .with_header("Link", &format!("<{preview_origin}/>; rel=\"canonical\"")),
                "/about" => TestResponse::new(
                    "200 OK",
                    "text/html",
                    format!(
                        "<html><head><link rel=\"canonical\" \
                         href=\"{preview_origin}/about\"></head><body>\
                         <a href=\"{preview_origin}/\">Home</a></body></html>"
                    ),
                ),
                _ => TestResponse::new("404 Not Found", "text/plain", "missing"),
            })
        }) {
            Ok(server) => server,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => return,
            Err(error) => panic!("binding test server: {error}"),
        };

        let snapshot = scan_live(LiveScanConfig {
            start_url: server.url("/"),
            public_origin: Some(Url::parse("https://example.test/").unwrap()),
            max_pages: 10,
            concurrency: 2,
            ..LiveScanConfig::default()
        })
        .await
        .unwrap();

        assert_eq!(snapshot.base_url.as_str(), "https://example.test/");
        assert_eq!(snapshot.statistics.total_urls, 2);
        assert_eq!(snapshot.statistics.indexable_urls, 2);
        assert_eq!(snapshot.statistics.orphan_urls, 0);
        assert_eq!(
            snapshot.urls["https://example.test/about"].http_status,
            Some(200)
        );
        let home = &snapshot.urls["https://example.test/"];
        assert_eq!(
            home.http_canonical.as_ref().unwrap().as_str(),
            home.url.as_str()
        );
        assert_eq!(
            home.html_canonical.as_ref().unwrap().as_str(),
            home.url.as_str()
        );
        assert_eq!(home.hreflang[0].url, home.url);
        assert!(snapshot
            .urls
            .values()
            .all(|state| state.url.origin() == snapshot.base_url.origin()));
    }

    #[tokio::test]
    async fn live_scan_fails_when_the_entry_response_is_not_successful_html() {
        let server = match TestServer::start(|path, _| {
            Some(match path {
                "/robots.txt" | "/sitemap.xml" => {
                    TestResponse::new("404 Not Found", "text/plain", "missing")
                }
                "/" => TestResponse::new("503 Service Unavailable", "text/html", "unavailable"),
                _ => TestResponse::new("404 Not Found", "text/plain", "missing"),
            })
        }) {
            Ok(server) => server,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => return,
            Err(error) => panic!("binding test server: {error}"),
        };

        let error = scan_live(LiveScanConfig {
            start_url: server.url("/"),
            max_pages: 10,
            ..LiveScanConfig::default()
        })
        .await
        .unwrap_err();
        assert!(error.to_string().contains("terminal status: HTTP 503"));
    }

    #[tokio::test]
    async fn live_scan_fails_instead_of_discarding_a_request_error() {
        let server = match TestServer::start(|path, _| match path {
            "/robots.txt" | "/sitemap.xml" => {
                Some(TestResponse::new("404 Not Found", "text/plain", "missing"))
            }
            "/" => Some(TestResponse::new(
                "200 OK",
                "text/html",
                "<html><body><a href=\"/broken\">Broken</a></body></html>",
            )),
            "/broken" => None,
            _ => Some(TestResponse::new("404 Not Found", "text/plain", "missing")),
        }) {
            Ok(server) => server,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => return,
            Err(error) => panic!("binding test server: {error}"),
        };

        let error = scan_live(LiveScanConfig {
            start_url: server.url("/"),
            max_pages: 10,
            ..LiveScanConfig::default()
        })
        .await
        .unwrap_err();
        assert!(error.to_string().contains("fetching discovered page"));
        assert!(error.to_string().contains("/broken"));
    }

    #[tokio::test]
    async fn live_scan_fails_when_the_page_budget_is_exhausted() {
        let server = match TestServer::start(|path, _| {
            Some(match path {
                "/robots.txt" | "/sitemap.xml" => {
                    TestResponse::new("404 Not Found", "text/plain", "missing")
                }
                "/" => TestResponse::new(
                    "200 OK",
                    "text/html",
                    "<html><body><a href=\"/next\">Next</a></body></html>",
                ),
                "/next" => TestResponse::new("200 OK", "text/html", "<html></html>"),
                _ => TestResponse::new("404 Not Found", "text/plain", "missing"),
            })
        }) {
            Ok(server) => server,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => return,
            Err(error) => panic!("binding test server: {error}"),
        };

        let error = scan_live(LiveScanConfig {
            start_url: server.url("/"),
            max_pages: 1,
            ..LiveScanConfig::default()
        })
        .await
        .unwrap_err();
        assert!(error.to_string().contains("page limit of 1 reached"));
    }
}
