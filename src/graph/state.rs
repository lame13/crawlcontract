use std::collections::BTreeMap;

use url::Url;

use crate::model::url_state::{RobotsTxtStatus, UrlState};
use crate::signals::robots_directive::effective_robots_directive;

/// Compute canonical, robots, and indexability state after all signals are loaded.
pub fn compute_derived_state(states: &mut BTreeMap<String, UrlState>) {
    let canonicals: BTreeMap<String, Option<Url>> = states
        .iter()
        .map(|(key, state)| {
            (
                key.clone(),
                state
                    .html_canonical
                    .as_ref()
                    .or(state.http_canonical.as_ref())
                    .cloned(),
            )
        })
        .collect();

    for (key, canonical) in canonicals {
        if let Some(state) = states.get_mut(&key) {
            state.effective_canonical = canonical;
        }
    }

    for state in states.values_mut() {
        state.effective_robots = effective_robots_directive(
            state.html_meta_robots.as_ref(),
            state.http_x_robots_tag.as_ref(),
        );
        let is_self_canonical = state
            .effective_canonical
            .as_ref()
            .map(|canonical| canonical == &state.url)
            .unwrap_or(true);

        state.is_indexable = !state.effective_robots.is_noindex()
            && state.robots_txt_status != RobotsTxtStatus::Blocked
            && state.http_status == Some(200)
            && is_self_canonical;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::robots::parse_directive_value;

    #[test]
    fn requires_a_verified_self_canonical_200_page() {
        let url = Url::parse("https://example.test/page").unwrap();
        let mut state = UrlState::new(url.clone());
        state.http_status = Some(200);
        state.html_canonical = Some(url.clone());
        let mut states = BTreeMap::from([(url.to_string(), state)]);

        compute_derived_state(&mut states);
        assert!(states[url.as_str()].is_indexable);

        states.get_mut(url.as_str()).unwrap().html_meta_robots =
            Some(parse_directive_value("noindex"));
        compute_derived_state(&mut states);
        assert!(!states[url.as_str()].is_indexable);
    }
}
