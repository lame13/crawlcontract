use url::Url;

/// Resolve a redirect chain starting from `start_url`.
/// In static mode this is a no-op (redirects are not observed).
/// In live mode, the caller provides the chain from HTTP responses.
pub fn normalize_redirect_chain(chain: &[Url]) -> Option<Url> {
    chain.last().cloned()
}

/// Returns true if the chain contains a cycle (a URL appearing more than once).
pub fn has_redirect_cycle(chain: &[Url]) -> bool {
    let mut seen = std::collections::HashSet::new();
    for url in chain {
        if !seen.insert(url.as_str()) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chain_resolves_to_last() {
        let chain = vec![
            Url::parse("https://example.test/a").unwrap(),
            Url::parse("https://example.test/b").unwrap(),
            Url::parse("https://example.test/c").unwrap(),
        ];
        let target = normalize_redirect_chain(&chain).unwrap();
        assert_eq!(target.as_str(), "https://example.test/c");
    }

    #[test]
    fn empty_chain_returns_none() {
        assert!(normalize_redirect_chain(&[]).is_none());
    }

    #[test]
    fn cycle_detected() {
        let chain = vec![
            Url::parse("https://example.test/a").unwrap(),
            Url::parse("https://example.test/b").unwrap(),
            Url::parse("https://example.test/a").unwrap(),
        ];
        assert!(has_redirect_cycle(&chain));
    }

    #[test]
    fn no_cycle() {
        let chain = vec![
            Url::parse("https://example.test/a").unwrap(),
            Url::parse("https://example.test/b").unwrap(),
        ];
        assert!(!has_redirect_cycle(&chain));
    }
}
