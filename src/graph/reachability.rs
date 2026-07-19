use std::collections::{BTreeMap, HashSet, VecDeque};

use url::Url;

use crate::model::url_state::UrlState;
use crate::scanner::html_signals::normalize_url_key;

/// Mark every URL reachable from the scan entry through links or redirects.
pub fn compute_reachability(states: &mut BTreeMap<String, UrlState>, entry_url: &Url) {
    let adjacency: BTreeMap<String, Vec<String>> = states
        .iter()
        .map(|(key, state)| {
            let mut targets: Vec<String> = state
                .internal_links_out
                .iter()
                .map(normalize_url_key)
                .collect();
            if let Some(redirect_target) = &state.redirect_target {
                targets.push(normalize_url_key(redirect_target));
            }
            (key.clone(), targets)
        })
        .collect();

    let entry_key = normalize_url_key(entry_url);
    let mut queue = VecDeque::new();
    let mut visited = HashSet::new();
    if states.contains_key(&entry_key) {
        queue.push_back(entry_key.clone());
        visited.insert(entry_key);
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

    for key in visited {
        if let Some(state) = states.get_mut(&key) {
            state.is_reachable = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follows_links_and_redirects_from_the_entry_only() {
        let home = Url::parse("https://example.test/").unwrap();
        let redirect = Url::parse("https://example.test/old").unwrap();
        let target = Url::parse("https://example.test/new").unwrap();
        let orphan = Url::parse("https://example.test/orphan").unwrap();

        let mut states = BTreeMap::new();
        let mut home_state = UrlState::new(home.clone());
        home_state.internal_links_out = vec![redirect.clone()];
        let mut redirect_state = UrlState::new(redirect.clone());
        redirect_state.redirect_target = Some(target.clone());
        states.insert(home.to_string(), home_state);
        states.insert(redirect.to_string(), redirect_state);
        states.insert(target.to_string(), UrlState::new(target.clone()));
        states.insert(orphan.to_string(), UrlState::new(orphan.clone()));

        compute_reachability(&mut states, &home);
        assert!(states[home.as_str()].is_reachable);
        assert!(states[redirect.as_str()].is_reachable);
        assert!(states[target.as_str()].is_reachable);
        assert!(!states[orphan.as_str()].is_reachable);
    }
}
