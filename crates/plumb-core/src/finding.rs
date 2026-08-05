// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Plumb Authors

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
