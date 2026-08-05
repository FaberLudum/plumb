// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Plumb Authors

#[cfg(test)]
mod tests {
    use super::*;

    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn reports_names_present_after_but_not_before() {
        let before = set(&["ruff"]);
        let after = set(&["ruff", "requestz"]);

        let added = added("requirements.txt", Some(&before), &after);

        assert_eq!(added.len(), 1);
        assert_eq!(added[0].name, "requestz");
        assert_eq!(added[0].manifest, "requirements.txt");
        assert_eq!(added[0].ecosystem, Ecosystem::PyPi);
    }

    #[test]
    fn treats_every_name_as_added_when_the_manifest_is_new() {
        let after = set(&["ruff", "httpx"]);

        let added = added("pyproject.toml", None, &after);

        let names: Vec<&str> = added.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, vec!["httpx", "ruff"]);
    }

    #[test]
    fn reports_nothing_when_a_dependency_is_only_removed() {
        let before = set(&["ruff", "httpx"]);
        let after = set(&["ruff"]);

        assert!(added("requirements.txt", Some(&before), &after).is_empty());
    }

    #[test]
    fn reports_nothing_when_only_the_version_changed() {
        let before = set(&["ruff"]);
        let after = set(&["ruff"]);

        assert!(added("requirements.txt", Some(&before), &after).is_empty());
    }
}
