pub mod html_signals;
pub mod live;
pub mod static_dir;

/// The default user-agent used for live scans and robots.txt evaluation.
pub fn default_user_agent() -> String {
    format!(
        "crawlcontract/{} (+https://github.com/lame13/crawlcontract)",
        env!("CARGO_PKG_VERSION")
    )
}
