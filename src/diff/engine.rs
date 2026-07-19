use crate::model::snapshot::Snapshot;

/// Compare two snapshots and return a human-readable summary of changes.
pub fn diff_snapshots(baseline: &Snapshot, candidate: &Snapshot) -> DiffSummary {
    let mut summary = DiffSummary {
        indexable_baseline: baseline.statistics.indexable_urls,
        indexable_candidate: candidate.statistics.indexable_urls,
        ..DiffSummary::default()
    };

    // Find URLs only in baseline (lost)
    for key in baseline.urls.keys() {
        if !candidate.urls.contains_key(key) {
            summary.lost_urls.push(key.clone());
        }
    }

    // Find URLs only in candidate (gained)
    for key in candidate.urls.keys() {
        if !baseline.urls.contains_key(key) {
            summary.gained_urls.push(key.clone());
        }
    }

    // Compare shared URLs
    for key in baseline.urls.keys() {
        if let Some(candidate_state) = candidate.urls.get(key) {
            let baseline_state = &baseline.urls[key];

            let mut changes = Vec::new();

            // Canonical change
            if baseline_state.effective_canonical != candidate_state.effective_canonical {
                changes.push(UrlChange::CanonicalChanged {
                    from: baseline_state
                        .effective_canonical
                        .as_ref()
                        .map(|u| u.to_string()),
                    to: candidate_state
                        .effective_canonical
                        .as_ref()
                        .map(|u| u.to_string()),
                });
            }

            // Indexability change
            if baseline_state.is_indexable != candidate_state.is_indexable {
                changes.push(UrlChange::IndexChanged {
                    from: baseline_state.is_indexable,
                    to: candidate_state.is_indexable,
                });
            }

            // Robots change
            if baseline_state.effective_robots != candidate_state.effective_robots {
                changes.push(UrlChange::RobotsChanged);
            }

            // Word count change
            if baseline_state.word_count != candidate_state.word_count {
                changes.push(UrlChange::WordCountChanged {
                    from: baseline_state.word_count,
                    to: candidate_state.word_count,
                });
            }

            // Link count change
            let bl = baseline_state.internal_links_out.len();
            let cl = candidate_state.internal_links_out.len();
            if bl != cl {
                changes.push(UrlChange::LinkCountChanged { from: bl, to: cl });
            }

            if !changes.is_empty() {
                summary.changed_urls.push((key.clone(), changes));
            }
        }
    }

    summary
}

#[derive(Debug, Default)]
pub struct DiffSummary {
    pub indexable_baseline: usize,
    pub indexable_candidate: usize,
    pub lost_urls: Vec<String>,
    pub gained_urls: Vec<String>,
    pub changed_urls: Vec<(String, Vec<UrlChange>)>,
}

#[derive(Debug)]
pub enum UrlChange {
    CanonicalChanged {
        from: Option<String>,
        to: Option<String>,
    },
    IndexChanged {
        from: bool,
        to: bool,
    },
    RobotsChanged,
    WordCountChanged {
        from: Option<usize>,
        to: Option<usize>,
    },
    LinkCountChanged {
        from: usize,
        to: usize,
    },
}

impl std::fmt::Display for DiffSummary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "Indexable URLs: {} → {}",
            self.indexable_baseline, self.indexable_candidate
        )?;

        if !self.lost_urls.is_empty() {
            writeln!(f, "\nLost URLs ({}):", self.lost_urls.len())?;
            for url in &self.lost_urls {
                writeln!(f, "  - {url}")?;
            }
        }

        if !self.gained_urls.is_empty() {
            writeln!(f, "\nGained URLs ({}):", self.gained_urls.len())?;
            for url in &self.gained_urls {
                writeln!(f, "  + {url}")?;
            }
        }

        if !self.changed_urls.is_empty() {
            writeln!(f, "\nChanged URLs ({}):", self.changed_urls.len())?;
            for (url, changes) in &self.changed_urls {
                writeln!(f, "  {url}:")?;
                for change in changes {
                    match change {
                        UrlChange::CanonicalChanged { from, to } => {
                            writeln!(f, "    canonical: {:?} → {:?}", from, to)?;
                        }
                        UrlChange::IndexChanged { from, to } => {
                            writeln!(f, "    indexable: {from} → {to}")?;
                        }
                        UrlChange::RobotsChanged => {
                            writeln!(f, "    robots directive changed")?;
                        }
                        UrlChange::WordCountChanged { from, to } => {
                            writeln!(f, "    words: {:?} → {:?}", from, to)?;
                        }
                        UrlChange::LinkCountChanged { from, to } => {
                            writeln!(f, "    links: {from} → {to}")?;
                        }
                    }
                }
            }
        }

        Ok(())
    }
}
