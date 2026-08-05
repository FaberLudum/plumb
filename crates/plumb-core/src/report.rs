// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Plumb Authors

//! Renders findings for the two things that read them: a person and a machine.
//!
//! Both renderings carry `source`. A finding that reaches a reader without the
//! evidence behind its rule is indistinguishable from an opinion, and the
//! project's one rule is that it never ships opinions.

use crate::finding::{Finding, Tier};

/// Renders findings for a terminal.
pub fn render_text(findings: &[Finding]) -> String {
    if findings.is_empty() {
        return "plumb: no findings\n".to_string();
    }

    let mut out = String::new();
    for finding in findings {
        let label = match finding.tier {
            Tier::A => "blocking",
            Tier::B => "warning",
            Tier::C => "note",
            Tier::Hygiene => "hygiene",
        };
        out.push_str(&format!(
            "{}: {} [{}]\n  {}\n  evidence: {}\n",
            label, finding.message, finding.rule, finding.location, finding.source
        ));
    }
    out
}

/// Renders findings as a JSON array for machine consumption.
///
/// `Finding` serialises strings and a fieldless enum, none of which
/// `serde_json` can fail on, so the fallback is unreachable rather than a
/// silent failure mode. It is written to be harmless if that ever stops being
/// true: the gate is `finding::exit_code`, which reads the findings and not
/// this string, so an empty array here cannot turn a blocking run into a
/// passing one. Make this fallible before making this output the gate.
pub fn render_json(findings: &[Finding]) -> String {
    serde_json::to_string_pretty(findings).unwrap_or_else(|_| "[]".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::finding::{Finding, Tier};

    fn at_tier(tier: Tier) -> Finding {
        Finding {
            rule: "package-exists".to_string(),
            tier,
            message: "package `requestz` does not exist on PyPI".to_string(),
            location: "requirements.txt".to_string(),
            source: "taxonomy §4.1".to_string(),
        }
    }

    fn sample() -> Vec<Finding> {
        vec![at_tier(Tier::A)]
    }

    #[test]
    fn text_output_names_the_rule_location_and_source() {
        let text = render_text(&sample());
        assert!(text.contains("package-exists"), "was: {text}");
        assert!(text.contains("requirements.txt"), "was: {text}");
        assert!(text.contains("taxonomy §4.1"), "was: {text}");
    }

    /// Pins the whole line shape, not just the presence of three fields.
    /// Without this, dropping the message or the tier label from the format
    /// string breaks nothing.
    #[test]
    fn text_output_has_a_fixed_shape() {
        assert_eq!(
            render_text(&sample()),
            "blocking: package `requestz` does not exist on PyPI [package-exists]\n  \
             requirements.txt\n  evidence: taxonomy §4.1\n"
        );
    }

    /// A reader has to be able to tell a blocking finding from a note at a
    /// glance, so no two tiers may share a label.
    #[test]
    fn every_tier_gets_its_own_label() {
        let labels: Vec<String> = [Tier::A, Tier::B, Tier::C, Tier::Hygiene]
            .into_iter()
            .map(|tier| {
                let text = render_text(&[at_tier(tier)]);
                text.split(':').next().unwrap_or_default().to_string()
            })
            .collect();

        assert_eq!(labels, ["blocking", "warning", "note", "hygiene"]);
    }

    /// Every finding gets reported, not just the first one.
    #[test]
    fn text_output_reports_every_finding() {
        let text = render_text(&[at_tier(Tier::A), at_tier(Tier::B)]);

        let starts = |prefix: &str| text.lines().filter(|l| l.starts_with(prefix)).count();
        assert_eq!(starts("blocking:"), 1, "was: {text}");
        assert_eq!(starts("warning:"), 1, "was: {text}");
    }

    /// Pins the wire spelling of every tier, not just the ones a sample
    /// happens to use.
    ///
    /// The JSON payload is a public contract: CI and the command layer match
    /// on these strings. Pinning only `A` and `Hygiene` leaves a rename of `B`
    /// or `C` — to `"warn"` and `"note"`, say — passing the whole suite while
    /// breaking every consumer.
    #[test]
    fn json_output_spells_every_tier_as_its_variant_name() {
        let findings: Vec<Finding> = [Tier::A, Tier::B, Tier::C, Tier::Hygiene]
            .into_iter()
            .map(at_tier)
            .collect();

        let json: serde_json::Value =
            serde_json::from_str(&render_json(&findings)).expect("valid json");

        assert_eq!(json[0]["tier"], "A");
        assert_eq!(json[1]["tier"], "B");
        assert_eq!(json[2]["tier"], "C");
        assert_eq!(json[3]["tier"], "Hygiene");
    }

    #[test]
    fn text_output_says_so_when_clean() {
        assert!(render_text(&[]).contains("no findings"));
    }

    #[test]
    fn json_output_is_an_array_of_findings() {
        let json: serde_json::Value =
            serde_json::from_str(&render_json(&sample())).expect("valid json");
        assert_eq!(json[0]["rule"], "package-exists");
        assert_eq!(json[0]["tier"], "A");
        assert_eq!(json[0]["source"], "taxonomy §4.1");
        assert_eq!(
            json[0]["message"],
            "package `requestz` does not exist on PyPI"
        );
        assert_eq!(json[0]["location"], "requirements.txt");
    }

    /// Every finding reaches the array, and its tier reaches it as the
    /// variant name a consumer can match on.
    #[test]
    fn json_output_lists_every_finding() {
        let json: serde_json::Value =
            serde_json::from_str(&render_json(&[at_tier(Tier::A), at_tier(Tier::Hygiene)]))
                .expect("valid json");

        let array = json.as_array().expect("an array");
        assert_eq!(array.len(), 2);
        assert_eq!(array[0]["tier"], "A");
        assert_eq!(array[1]["tier"], "Hygiene");
    }

    #[test]
    fn json_output_is_an_empty_array_when_clean() {
        assert_eq!(render_json(&[]).trim(), "[]");
    }
}
