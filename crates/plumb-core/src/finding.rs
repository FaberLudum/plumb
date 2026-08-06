// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Plumb Authors

//! What a rule violation is, and what it costs.
//!
//! A tier is not a severity, it is the strength of the evidence behind the
//! rule. Whether a finding blocks the build follows from what is known about
//! the defect it reports, not from how annoying the defect feels, which is why
//! `source` is a required field: a finding that cannot name the taxonomy
//! section justifying its rule has no business blocking anyone's build.

use serde::Serialize;

/// How strongly a rule is enforced, and on what strength of evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Tier {
    /// Peer-reviewed evidence, objective detection. Blocks the build.
    A,
    /// Preprint evidence, or thresholds still needing calibration. Warns.
    B,
    /// Instrumentation for open hypotheses. Never affects the exit code.
    C,
    /// Style consistency, delegated to formatters. Excluded from defect
    /// counts, so like `C` it never affects the exit code.
    Hygiene,
}

/// A single rule violation, with the evidence that justifies the rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Finding {
    pub rule: String,
    pub tier: Tier,
    pub message: String,
    pub location: String,
    /// The taxonomy section justifying this rule. Callers must supply one;
    /// the type makes it mandatory, nothing here checks that it is non-empty.
    pub source: String,
}

/// Maps findings to a process exit code: 0 clean, 1 Tier A, 2 warnings only.
///
/// The three codes are a public contract — CI blocks on 1 — so they may not be
/// renumbered. `Tier::C` and `Tier::Hygiene` are deliberately absent from the
/// mapping: instrumentation and style are excluded from defect counts, and a
/// run reporting nothing else is a clean run.
pub fn exit_code(findings: &[Finding]) -> i32 {
    if findings.iter().any(|f| f.tier == Tier::A) {
        return 1;
    }
    if findings.iter().any(|f| f.tier == Tier::B) {
        return 2;
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finding(tier: Tier) -> Finding {
        Finding {
            rule: "package-exists".to_string(),
            tier,
            message: "package `requestz` does not exist on PyPI".to_string(),
            location: "requirements.txt".to_string(),
            source: "taxonomy §4.1".to_string(),
        }
    }

    #[test]
    fn no_findings_exits_zero() {
        assert_eq!(exit_code(&[]), 0);
    }

    #[test]
    fn a_tier_a_finding_exits_one() {
        assert_eq!(exit_code(&[finding(Tier::A)]), 1);
    }

    #[test]
    fn warnings_alone_exit_two() {
        assert_eq!(exit_code(&[finding(Tier::B)]), 2);
    }

    #[test]
    fn hygiene_alone_exits_zero() {
        assert_eq!(exit_code(&[finding(Tier::Hygiene)]), 0);
    }

    /// Tier C measures and must never affect the exit code. Without this,
    /// nothing distinguishes "C is ignored" from "C warns"; the Hygiene test
    /// alone does not pin C.
    #[test]
    fn notes_alone_exit_zero() {
        assert_eq!(exit_code(&[finding(Tier::C)]), 0);
    }

    #[test]
    fn tier_a_wins_over_warnings() {
        assert_eq!(exit_code(&[finding(Tier::B), finding(Tier::A)]), 1);
    }

    /// The mirror of `tier_a_wins_over_warnings`. Together the two pin that
    /// the code is a property of the whole slice: one fails for an
    /// implementation that reads only the first finding, the other for one
    /// that reads only the last.
    #[test]
    fn tier_a_wins_over_warnings_reported_after_it() {
        assert_eq!(exit_code(&[finding(Tier::A), finding(Tier::B)]), 1);
    }

    /// Findings that carry no verdict must not suppress one that does.
    #[test]
    fn notes_and_hygiene_do_not_mask_a_warning() {
        assert_eq!(
            exit_code(&[finding(Tier::C), finding(Tier::Hygiene), finding(Tier::B)]),
            2
        );
    }

    /// Stops compiling the moment a tier is added.
    ///
    /// `exit_code` decides with `any`, so a new variant would silently fall
    /// through to 0 — a rule that blocks nothing and warns about nothing.
    /// The `match` here is exhaustive over `Tier`, so adding a variant forces
    /// whoever adds it to state what it costs.
    #[test]
    fn every_tier_must_state_what_it_costs() {
        for tier in [Tier::A, Tier::B, Tier::C, Tier::Hygiene] {
            let expected = match tier {
                Tier::A => 1,
                Tier::B => 2,
                Tier::C | Tier::Hygiene => 0,
            };
            assert_eq!(exit_code(&[finding(tier)]), expected, "tier {tier:?}");
        }
    }
}
