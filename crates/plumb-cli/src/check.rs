// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Plumb Authors

#[cfg(test)]
mod tests {
    use super::*;
    use plumb_core::manifest;
    use std::cell::RefCell;
    use std::fs;
    use std::path::Path;
    use std::process::Command;

    /// Answers without a network, and remembers what it was asked.
    ///
    /// The log is what pins the set difference: a gate that looked up every
    /// name in the manifest rather than the added ones would still produce the
    /// right findings here, and only the log tells the two apart.
    #[derive(Default)]
    struct StubRegistry {
        asked: RefCell<Vec<String>>,
    }

    impl Registry for StubRegistry {
        fn exists(&self, name: &str) -> Existence {
            self.asked.borrow_mut().push(name.to_string());
            match name {
                "requestz" => Existence::Missing,
                "ruff" | "httpx" => Existence::Exists,
                _ => Existence::Unknown,
            }
        }
    }

    impl StubRegistry {
        fn asked(&self) -> Vec<String> {
            self.asked.borrow().clone()
        }
    }

    /// Runs a git command in `dir` and asserts it succeeded.
    ///
    /// Both halves matter, for the reasons `git::tests::run_git` records:
    /// `.output()` alone only checks that git spawned, and ambient config can
    /// leave the fixture with an unborn HEAD, against which `git diff HEAD`
    /// fails and every test here would fail for the wrong reason.
    fn run_git(dir: &Path, args: &[&str]) {
        let no_config = dir.join("no-such-gitconfig");
        let output = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", &no_config)
            .env("GIT_CONFIG_SYSTEM", &no_config)
            .output()
            .expect("git runs");
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn write(dir: &Path, name: &str, body: &str) {
        let path = dir.join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent");
        }
        fs::write(path, body).expect("write");
    }

    /// A repository whose HEAD holds `files`.
    ///
    /// The `.gitattributes` line is load-bearing rather than tidy. A developer
    /// whose global attributes mark `*.txt` as `-diff` gets `Binary files …
    /// differ` from `working_diff` instead of a hunk, and that section carries
    /// no `+++ b/…` line at all — so the manifest never reaches the gate and
    /// every test here passes by finding nothing. `working_diff` honours
    /// ambient config, as production does, so this has to be pinned inside the
    /// repository, where it outranks the global and XDG attribute files.
    fn repo_with(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("temp dir");
        run_git(dir.path(), &["init", "-b", "main"]);
        write(dir.path(), ".gitattributes", "* diff\n");
        run_git(dir.path(), &["config", "user.email", "test@example.com"]);
        run_git(dir.path(), &["config", "user.name", "Test"]);
        for (name, body) in files {
            write(dir.path(), name, body);
        }
        run_git(dir.path(), &["add", "-A"]);
        run_git(dir.path(), &["commit", "-m", "initial"]);
        dir
    }

    fn repo_with_requirements(initial: &str) -> tempfile::TempDir {
        repo_with(&[("requirements.txt", initial)])
    }

    #[test]
    fn flags_a_dependency_that_does_not_exist() {
        let dir = repo_with_requirements("ruff==0.4.0\n");
        write(
            dir.path(),
            "requirements.txt",
            "ruff==0.4.0\nrequestz==2.31.0\n",
        );

        let (output, code) = run(dir.path(), &StubRegistry::default(), false).expect("check runs");

        assert_eq!(code, 1, "output was: {output}");
        assert!(output.contains("requestz"), "output was: {output}");
        assert!(output.contains("requirements.txt"), "output was: {output}");
    }

    #[test]
    fn passes_when_every_added_dependency_exists() {
        let dir = repo_with_requirements("ruff==0.4.0\n");
        write(
            dir.path(),
            "requirements.txt",
            "ruff==0.4.0\nhttpx==0.27.0\n",
        );

        let (output, code) = run(dir.path(), &StubRegistry::default(), false).expect("check runs");

        assert_eq!(code, 0, "output was: {output}");
    }

    #[test]
    fn ignores_a_removed_dependency() {
        let dir = repo_with_requirements("ruff==0.4.0\nrequestz==2.31.0\n");
        write(dir.path(), "requirements.txt", "ruff==0.4.0\n");

        let (output, code) = run(dir.path(), &StubRegistry::default(), false).expect("check runs");

        assert_eq!(code, 0, "output was: {output}");
    }

    /// `Unknown` is a failure to ask, not a rule violation. A firewall, an
    /// outage or an offline run must never be the reason a build stops.
    #[test]
    fn an_unknown_result_does_not_block() {
        let dir = repo_with_requirements("ruff==0.4.0\n");
        write(
            dir.path(),
            "requirements.txt",
            "ruff==0.4.0\nmystery==1.0\n",
        );

        let (output, code) = run(dir.path(), &StubRegistry::default(), false).expect("check runs");

        assert_eq!(code, 0, "output was: {output}");
        assert!(!output.contains("mystery"), "output was: {output}");
    }

    #[test]
    fn json_output_parses_and_carries_the_evidence() {
        let dir = repo_with_requirements("ruff==0.4.0\n");
        write(
            dir.path(),
            "requirements.txt",
            "ruff==0.4.0\nrequestz==2.31.0\n",
        );

        let (output, code) = run(dir.path(), &StubRegistry::default(), true).expect("check runs");
        let parsed: serde_json::Value = serde_json::from_str(&output).expect("valid json");

        assert_eq!(code, 1, "the rendering must not change the verdict");
        assert_eq!(parsed[0]["rule"], "package-exists");
        assert_eq!(parsed[0]["tier"], "A");
        assert_eq!(parsed[0]["source"], "taxonomy §4.1");
        assert_eq!(parsed[0]["location"], "requirements.txt");
    }

    /// Only names the change actually adds reach the registry.
    ///
    /// Two properties at once, and both are about cost rather than verdict: a
    /// gate that looked up every name in the manifest would turn a one-line
    /// edit into a lookup per dependency, and one that looked up every added
    /// *line* would ask about the same name twice.
    #[test]
    fn only_the_names_a_change_adds_reach_the_registry() {
        let dir = repo_with_requirements("ruff==0.4.0\nhttpx==0.27.0\n");
        write(
            dir.path(),
            "requirements.txt",
            "ruff==0.4.1\nhttpx==0.27.0\nrequestz==2.31.0\n",
        );
        let registry = StubRegistry::default();

        let (_, code) = run(dir.path(), &registry, false).expect("check runs");

        assert_eq!(code, 1);
        assert_eq!(registry.asked(), vec!["requestz"]);
    }

    /// A manifest the change creates has no version at HEAD, so every name in
    /// it is an addition.
    #[test]
    fn a_manifest_the_change_adds_has_every_name_checked() {
        let dir = repo_with_requirements("ruff==0.4.0\n");
        write(dir.path(), "requirements-dev.txt", "requestz==2.31.0\n");
        let registry = StubRegistry::default();

        let (output, code) = run(dir.path(), &registry, false).expect("check runs");

        assert_eq!(code, 1, "output was: {output}");
        assert!(
            output.contains("requirements-dev.txt"),
            "output was: {output}"
        );
        assert_eq!(registry.asked(), vec!["requestz"]);
    }

    #[test]
    fn a_missing_dependency_in_a_pyproject_is_flagged() {
        let dir = repo_with(&[(
            "packages/api/pyproject.toml",
            "[project]\ndependencies = [\"ruff\"]\n",
        )]);
        write(
            dir.path(),
            "packages/api/pyproject.toml",
            "[project]\ndependencies = [\"ruff\", \"requestz\"]\n",
        );

        let (output, code) = run(dir.path(), &StubRegistry::default(), false).expect("check runs");

        assert_eq!(code, 1, "output was: {output}");
        assert!(
            output.contains("packages/api/pyproject.toml"),
            "output was: {output}"
        );
    }

    /// The gate reads manifests, not imports. An `import requestz` in a source
    /// file is not a declared dependency, and treating it as one would block a
    /// build over a module the project vendors, aliases or ships itself.
    #[test]
    fn a_change_to_a_source_file_asks_nothing() {
        let dir = repo_with(&[
            ("requirements.txt", "ruff==0.4.0\n"),
            ("src/app.py", "import os\n"),
        ]);
        write(dir.path(), "src/app.py", "import os\nimport requestz\n");
        let registry = StubRegistry::default();

        let (output, code) = run(dir.path(), &registry, false).expect("check runs");

        assert_eq!(code, 0, "output was: {output}");
        assert!(registry.asked().is_empty(), "asked: {:?}", registry.asked());
    }

    /// A clean tree is a clean run, not an error.
    #[test]
    fn an_unchanged_repository_reports_nothing() {
        let dir = repo_with_requirements("ruff==0.4.0\nrequestz==2.31.0\n");
        let registry = StubRegistry::default();

        let (output, code) = run(dir.path(), &registry, false).expect("check runs");

        assert_eq!(code, 0, "output was: {output}");
        assert!(output.contains("no findings"), "output was: {output}");
        assert!(registry.asked().is_empty(), "asked: {:?}", registry.asked());
    }

    /// `Finding::source` is documented as required but nothing enforces it:
    /// `String::new()` compiles, serialises as `"source": ""` and renders as an
    /// `evidence: ` with nothing after it. The type cannot enforce it without
    /// private fields and a fallible constructor, so the enforcement lives
    /// here, over what the gate actually emits.
    ///
    /// Asserting against the taxonomy itself rather than against a string
    /// shape is the point. The project's one rule is that every rule traces
    /// back to a row in the taxonomy, and a citation of a section that does not
    /// exist satisfies any pattern check while satisfying the rule not at all.
    #[test]
    fn every_finding_names_a_section_of_the_taxonomy() {
        let taxonomy_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("docs")
            .join("research")
            .join("taxonomy.md");
        let taxonomy = fs::read_to_string(&taxonomy_path)
            .unwrap_or_else(|error| panic!("reading {}: {error}", taxonomy_path.display()));

        // Every shape of input this gate emits a finding for. A finding built
        // on a path not covered here would escape the check, so a new emission
        // site means a new row.
        let scenarios: &[(&str, &str, &str)] = &[
            (
                "requirements.txt",
                "ruff==0.4.0\n",
                "ruff==0.4.0\nrequestz==2.31.0\n",
            ),
            (
                "pyproject.toml",
                "[project]\ndependencies = [\"ruff\"]\n",
                "[project]\ndependencies = [\"ruff\", \"requestz\"]\n",
            ),
        ];

        let registry = StubRegistry::default();
        let mut findings = Vec::new();
        for (name, before, after) in scenarios {
            let dir = repo_with(&[(name, before)]);
            write(dir.path(), name, after);
            findings.extend(collect(dir.path(), &registry).expect("check runs"));
        }

        assert_eq!(
            findings.len(),
            scenarios.len(),
            "a scenario emitted no finding, so it proves nothing: {findings:#?}"
        );
        for finding in &findings {
            assert!(!finding.rule.is_empty(), "unnamed rule: {finding:#?}");
            assert!(!finding.message.is_empty(), "empty message: {finding:#?}");
            assert!(!finding.location.is_empty(), "empty location: {finding:#?}");

            let section = finding
                .source
                .strip_prefix("taxonomy §")
                .unwrap_or_else(|| panic!("source does not cite the taxonomy: {finding:#?}"));
            // The trailing space anchors the number: without it a citation of
            // §4.1 would be satisfied by the heading for §4.10.
            let heading = format!("{section} ");
            assert!(
                taxonomy.lines().any(|line| {
                    line.starts_with('#')
                        && line
                            .trim_start_matches('#')
                            .trim_start()
                            .starts_with(&heading)
                }),
                "no section {section} in the taxonomy, cited by {finding:#?}"
            );
        }
    }

    /// The gate decides whether a changed file is a manifest by asking
    /// `manifest::parse` about an empty body, so it can skip reading files it
    /// would never parse. That is only sound while the dispatch is a function
    /// of the filename alone, which is what this pins — the alternative, a
    /// second filename rule in this crate, is exactly the duplicated dispatch
    /// `deps::added` warns against.
    #[test]
    fn a_manifest_is_recognised_from_its_path_alone() {
        for path in [
            "requirements.txt",
            "requirements-dev.txt",
            "pyproject.toml",
            "packages/api/pyproject.toml",
        ] {
            assert!(
                manifest::parse(path, "").is_some(),
                "{path} was not recognised from its name"
            );
        }
        for path in ["src/app.py", "README.md", "requirements_notes.txt"] {
            assert!(
                manifest::parse(path, "ruff==0.4.0\n").is_none(),
                "{path} was treated as a manifest"
            );
        }
    }
}
