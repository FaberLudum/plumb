// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Plumb Authors

//! Per-commit longitudinal metrics: volume, deletion ratio, coupling delta,
//! and the decay signature (volume and coupling rising together).

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct CommitStats {
    pub hash: String,
    pub subject: String,
    pub added: u64,
    pub removed: u64,
    /// `None` when no architecture is declared, so coupling cannot be resolved.
    pub coupling_delta: Option<i64>,
}

/// Deletion ratio: removed / added as `f64`.
///
/// Returns `None` when added is 0, never panics on division.
pub fn deletion_ratio(stats: &CommitStats) -> Option<f64> {
    todo!()
}

/// Decay signature: volume delta > 0 AND coupling delta is `Some(d)` with d > 0.
///
/// This is the pattern from taxonomy §4.5 (volume and coupling rising together),
/// surfaced but never judged: no finding, no exit-code effect.
pub fn has_decay_signature(stats: &CommitStats) -> bool {
    todo!()
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Summary {
    pub commits: usize,
    pub added: u64,
    pub removed: u64,
    pub mean_deletion_ratio: Option<f64>,
    pub decay_commits: Vec<String>,
}

/// Summarize the stats: count totals, compute mean deletion ratio, name decay commits.
pub fn summarize(stats: &[CommitStats]) -> Summary {
    todo!()
}

/// Render a text report of the stats and summary.
///
/// One row per commit, followed by a summary block.
pub fn render_text(stats: &[CommitStats]) -> String {
    todo!()
}

/// Render a JSON report of the stats and summary.
///
/// Shape: `{"commits": [...], "summary": {...}}` with deletion_ratio injected.
pub fn render_json(stats: &[CommitStats]) -> String {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(hash: &str, added: u64, removed: u64, coupling: Option<i64>) -> CommitStats {
        CommitStats {
            hash: hash.to_string(),
            subject: format!("subject for {hash}"),
            added,
            removed,
            coupling_delta: coupling,
        }
    }

    #[test]
    fn deletion_ratio_is_removed_over_added() {
        let ratio = deletion_ratio(&commit("a", 100, 25, None));
        assert_eq!(ratio, Some(0.25));
    }

    #[test]
    fn a_commit_that_adds_nothing_has_no_ratio() {
        assert_eq!(deletion_ratio(&commit("a", 0, 10, None)), None);
    }

    #[test]
    fn decay_needs_volume_and_coupling_rising_together() {
        assert!(has_decay_signature(&commit("a", 50, 10, Some(2))));
        assert!(!has_decay_signature(&commit("b", 50, 10, Some(0))));
        assert!(!has_decay_signature(&commit("c", 50, 10, Some(-1))));
        assert!(!has_decay_signature(&commit("d", 5, 10, Some(2))));
        assert!(!has_decay_signature(&commit("e", 50, 10, None)));
    }

    #[test]
    fn summary_counts_totals_and_names_decay_commits() {
        let stats = vec![
            commit("aaaaaaa", 100, 20, Some(3)),
            commit("bbbbbbb", 10, 40, Some(-1)),
            commit("ccccccc", 0, 5, None),
        ];
        let summary = summarize(&stats);
        assert_eq!(summary.commits, 3);
        assert_eq!(summary.added, 110);
        assert_eq!(summary.removed, 65);
        assert_eq!(summary.decay_commits, vec!["aaaaaaa"]);
        // Mean over the two commits that have a ratio: (0.2 + 4.0) / 2.
        assert_eq!(summary.mean_deletion_ratio, Some(2.1));
    }

    #[test]
    fn text_report_carries_rows_and_summary() {
        let stats = vec![
            commit("aaaaaaa1234", 100, 20, Some(3)),
            commit("bbbbbbb5678", 0, 5, None),
        ];
        let text = render_text(&stats);
        assert!(text.contains("aaaaaaa"), "{text}");
        assert!(!text.contains("aaaaaaa1234"), "hash not shortened: {text}");
        assert!(text.contains("+100"), "{text}");
        assert!(text.contains("-20"), "{text}");
        assert!(text.contains("decay signature: 1 of 2"), "{text}");
        assert!(text.ends_with('\n'), "{text:?}");
    }

    #[test]
    fn json_report_is_parseable_and_spells_null_ratios() {
        let stats = vec![commit("aaaaaaa", 0, 5, None)];
        let json = render_json(&stats);
        let value: serde_json::Value = serde_json::from_str(&json).expect("parses");
        assert_eq!(value["commits"][0]["deletion_ratio"], serde_json::Value::Null);
        assert_eq!(value["commits"][0]["coupling_delta"], serde_json::Value::Null);
        assert_eq!(value["summary"]["commits"], 1);
    }

    #[test]
    fn rendering_is_deterministic() {
        let stats = vec![commit("aaaaaaa", 1, 2, Some(1))];
        assert_eq!(render_text(&stats), render_text(&stats));
        assert_eq!(render_json(&stats), render_json(&stats));
    }
}
