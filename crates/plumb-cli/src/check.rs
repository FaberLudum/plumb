// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Plumb Authors

//! The `check --diff` gate: from a working diff to findings and an exit code.
//!
//! Wiring, and nothing else. Every decision this file makes was made somewhere
//! testable: what a manifest is, what counts as an added dependency, whether a
//! name resolves, what a finding costs. What is left here is the order they
//! happen in, and one rule of its own — that only [`Existence::Missing`] is a
//! violation. A firewall, an outage, an offline run and a manifest this
//! version cannot read are all failures to establish anything, and a gate that
//! blocked on those would be switched off within the week.
//!
//! Failing to establish anything is not the same as having nothing to say, and
//! the two were conflated here once already. A manifest that could not be read
//! still moves no verdict, but the run now reports it — see [`Unreadable`].

use crate::git;
use crate::registry::{Existence, Registry};
use anyhow::Result;
use plumb_core::deps::{self, AddedDependency};
use plumb_core::finding::{Finding, Tier, exit_code};
use plumb_core::manifest::{self, Ecosystem};
use plumb_core::report::{render_json, render_text};
use std::path::Path;

/// The taxonomy section behind the package existence rule.
///
/// Named once rather than spelled at the emission site. Every finding must
/// cite the evidence for its rule, and a citation copied per call site is one
/// that drifts — `every_finding_names_a_section_of_the_taxonomy` checks this
/// against the taxonomy itself.
const PACKAGE_EXISTS_SOURCE: &str = "taxonomy §4.1";

/// A manifest the change touched that this run could not read.
///
/// Carried out of [`collect`] rather than printed where it is discovered:
/// `check` decides what happened, the command layer decides how to say it, and
/// a module that printed would be a module the tests could not observe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unreadable {
    /// Repository-relative, as git spells it.
    pub path: String,
    /// Why, in a form a user can act on.
    pub reason: String,
}

/// What one run of the gate established.
///
/// [`Outcome::unreadable`] is not folded into the findings on purpose. A
/// finding asserts something about the code under inspection and carries a
/// tier and a citation; "this process could not decode a file" asserts
/// something about this run and cites nothing.
pub struct Outcome {
    /// The rendered report, for stdout.
    pub report: String,
    /// `0` clean, `1` a Tier A violation, `2` warnings only.
    pub code: i32,
    /// Manifests left unchecked because they could not be read.
    pub unreadable: Vec<Unreadable>,
}

/// Runs the diff-scoped gates and returns the rendered report and exit code.
///
/// `registry` is a `&dyn Registry` so a caller can hand in a stub and stay
/// offline; it is one registry for the whole run, not one per file, because a
/// registry is where the cache and the unreachable-network latch live and both
/// are useless if they are thrown away between manifests.
pub fn run(repo: &Path, registry: &dyn Registry, json: bool) -> Result<Outcome> {
    let (findings, unreadable) = collect(repo, registry)?;

    let report = if json {
        render_json(&findings)
    } else {
        render_text(&findings)
    };

    Ok(Outcome {
        report,
        code: exit_code(&findings),
        unreadable,
    })
}

/// Reads a manifest from the working tree.
///
/// Three outcomes, and the whole point is that they are three rather than one.
/// `Ok(None)` is a file that is not there — deleted by the change under
/// inspection, which is ordinary and says nothing. `Err` is a file that is
/// there and could not be turned into text, which is not ordinary and must be
/// said out loud.
///
/// Reading bytes and decoding separately, rather than `read_to_string`, is
/// what separates those two: `read_to_string` reports invalid UTF-8 as an
/// `io::Error` of kind `InvalidData`, indistinguishable at a glance from a
/// genuine I/O failure and previously swallowed along with it.
fn read_manifest(path: &Path) -> std::result::Result<Option<String>, String> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        // A manifest the change deletes is listed by `git diff HEAD` and is
        // not there to read. It adds no dependency, so there is nothing to
        // report and nothing to warn about.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };

    String::from_utf8(bytes).map(Some).map_err(|error| {
        format!(
            "not valid UTF-8 at byte {}",
            error.utf8_error().valid_up_to()
        )
    })
}

/// Collects the findings for the working diff, and the manifests it could not
/// read.
///
/// Separate from [`run`] so the findings can be inspected before rendering
/// flattens them into a string.
fn collect(repo: &Path, registry: &dyn Registry) -> Result<(Vec<Finding>, Vec<Unreadable>)> {
    let mut findings = Vec::new();
    let mut unreadable = Vec::new();

    for path in git::changed_paths(repo)? {
        // Ask the dispatch itself whether this path is a manifest, before
        // reading a byte of it. A change touching a thousand source files
        // would otherwise be a thousand files read into memory to be thrown
        // away, and one of them is eventually a generated blob.
        //
        // The probe is `manifest::parse` rather than a filename rule spelled
        // out here on purpose: a second copy of that rule in this crate is
        // exactly the duplicated dispatch `deps::added` warns against, and it
        // would drift the first time the real one learns a new name. It is
        // sound only while the dispatch is a function of the path alone, which
        // `a_manifest_is_recognised_from_its_path_alone` pins.
        if manifest::parse(&path, "").is_none() {
            continue;
        }

        // `git diff` paths are relative to the repository root and separated
        // by `/`, which is what `manifest::parse` splits on and what `join`
        // accepts on every platform this runs on.
        //
        // A manifest that cannot be read yields no added dependencies and so
        // no findings. Failing open is still the right direction — a file this
        // process could not decode is not evidence that a package does not
        // exist — but failing open *quietly* is not. This used to be
        // `read_to_string(...).unwrap_or_default()`, and one Latin-1 byte in a
        // comment therefore unchecked the whole file: `# cafe pinned` blocked,
        // `# caf\xe9 pinned` exited 0 with nothing said on either stream.
        //
        // Note the asymmetry with `git::show_at_head`, which is strict about
        // UTF-8 and aborts the run with exit 70. The two sides of the same
        // comparison fail in opposite directions, and that is deliberate
        // rather than an oversight: 70 is outside the 0/1/2 contract, so the
        // before-side failure is unmistakable and can never be read as a clean
        // run. The after-side failure could be, which is why it is reported.
        // Neither is silent; only one is fatal.
        let current = match read_manifest(&repo.join(&path)) {
            Ok(Some(text)) => text,
            Ok(None) => continue,
            Err(reason) => {
                unreadable.push(Unreadable {
                    path: path.clone(),
                    reason,
                });
                continue;
            }
        };
        let Some(after) = manifest::parse(&path, &current) else {
            continue;
        };

        // `None` means the manifest is new in this change, so every name in it
        // is an addition.
        //
        // A manifest that exists at HEAD but does not parse is a different
        // matter and is deliberately not special-cased: it yields `Some(empty
        // set)`, so every current dependency reads as added and gets looked
        // up. Nothing here can tell that apart from a manifest that genuinely
        // declared nothing — a case where those names really are additions —
        // without `manifest::parse` growing a way to say "I did not
        // understand this", which is a change to the core contract rather than
        // to this wiring. The cost of the honest reading is bounded: extra
        // lookups, and a finding only for a name the registry actually lacks.
        let before = git::show_at_head(repo, &path)?.and_then(|text| manifest::parse(&path, &text));

        for dependency in deps::added(&path, before.as_ref(), &after) {
            if registry.exists(&dependency.name) == Existence::Missing {
                findings.push(package_does_not_exist(&dependency));
            }
        }
    }

    Ok((findings, unreadable))
}

/// The finding for a dependency no registry has.
///
/// Tier A: taxonomy §4.1 is peer-reviewed and the detection is objective — the
/// name is in the registry or it is not.
fn package_does_not_exist(dependency: &AddedDependency) -> Finding {
    // Exhaustive on purpose, and it stops compiling the moment a second
    // ecosystem lands — which is the moment this message becomes a lie and
    // this whole function starts naming the wrong registry. It is the same
    // guard `deps::added` keeps over the ecosystem it hard-codes.
    let registry_name = match dependency.ecosystem {
        Ecosystem::PyPi => "PyPI",
    };

    Finding {
        rule: "package-exists".to_string(),
        tier: Tier::A,
        message: format!(
            "package `{}` does not exist on {registry_name}",
            dependency.name
        ),
        location: dependency.manifest.clone(),
        source: PACKAGE_EXISTS_SOURCE.to_string(),
    }
}

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
    /// This used to write a `* diff` attribute, because a developer whose
    /// global attributes marked `*.txt` as `-diff` got `Binary files … differ`
    /// instead of a hunk, and that section carries no `+++ b/…` line — so the
    /// manifest never reached the gate and every test here passed by finding
    /// nothing. Forbidding it inside the fixture also meant no test could
    /// observe the failure. `changed_paths` reads names rather than hunks, so
    /// there is nothing left to suppress and the fixture runs against the same
    /// git a developer has.
    fn repo_with(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("temp dir");
        run_git(dir.path(), &["init", "-b", "main"]);
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

        let outcome = run(dir.path(), &StubRegistry::default(), false).expect("check runs");
        let (output, code) = (outcome.report, outcome.code);

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

        let outcome = run(dir.path(), &StubRegistry::default(), false).expect("check runs");
        let (output, code) = (outcome.report, outcome.code);

        assert_eq!(code, 0, "output was: {output}");
    }

    #[test]
    fn ignores_a_removed_dependency() {
        let dir = repo_with_requirements("ruff==0.4.0\nrequestz==2.31.0\n");
        write(dir.path(), "requirements.txt", "ruff==0.4.0\n");

        let outcome = run(dir.path(), &StubRegistry::default(), false).expect("check runs");
        let (output, code) = (outcome.report, outcome.code);

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

        let outcome = run(dir.path(), &StubRegistry::default(), false).expect("check runs");
        let (output, code) = (outcome.report, outcome.code);

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

        let outcome = run(dir.path(), &StubRegistry::default(), true).expect("check runs");
        let (output, code) = (outcome.report, outcome.code);
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

        let code = run(dir.path(), &registry, false).expect("check runs").code;

        assert_eq!(code, 1);
        assert_eq!(registry.asked(), vec!["requestz"]);
    }

    /// A manifest the change creates has no version at HEAD, so every name in
    /// it is an addition.
    #[test]
    fn a_manifest_the_change_adds_has_every_name_checked() {
        let dir = repo_with_requirements("ruff==0.4.0\n");
        write(dir.path(), "requirements-dev.txt", "requestz==2.31.0\n");
        // Staged, because `git diff HEAD` does not report untracked files —
        // see `an_untracked_manifest_is_not_seen_until_it_is_staged`.
        run_git(dir.path(), &["add", "-A"]);
        let registry = StubRegistry::default();

        let outcome = run(dir.path(), &registry, false).expect("check runs");
        let (output, code) = (outcome.report, outcome.code);

        assert_eq!(code, 1, "output was: {output}");
        assert!(
            output.contains("requirements-dev.txt"),
            "output was: {output}"
        );
        assert_eq!(registry.asked(), vec!["requestz"]);
    }

    /// Recorded, not endorsed.
    ///
    /// The gate reads `git diff HEAD`, which reports tracked changes only, so
    /// a manifest that has never been added is invisible to it. Staging it is
    /// what makes it visible, which suits a pre-commit hook and not a
    /// developer running the gate by hand after creating a file. Widening it
    /// means asking git for untracked files as well and deciding what an
    /// unstaged file even means for a gate on a change, which is a scope
    /// decision, not wiring.
    ///
    /// This used to add "and a CI checkout" to the list of places that see
    /// the content anyway. That was false, and it was the sentence that made
    /// the larger blind spot below read as handled. A CI checkout has a
    /// **clean tree**: `git diff HEAD` lists nothing, so the run checks
    /// nothing and exits 0 no matter what the commit under test added. The
    /// scope of `--diff` is the working tree and the index against HEAD, and
    /// anything already committed is outside it — see
    /// `a_committed_change_is_outside_the_scope_of_diff`, which pins that
    /// directly rather than leaving it to a comment.
    ///
    /// Pinned so the limitation is visible rather than lurking: whoever
    /// changes it will delete this test on purpose.
    #[test]
    fn an_untracked_manifest_is_not_seen_until_it_is_staged() {
        let dir = repo_with_requirements("ruff==0.4.0\n");
        write(dir.path(), "requirements-dev.txt", "requestz==2.31.0\n");
        let registry = StubRegistry::default();

        let unstaged = run(dir.path(), &registry, false).expect("check runs").code;
        assert_eq!(unstaged, 0);
        assert!(registry.asked().is_empty(), "asked: {:?}", registry.asked());

        run_git(dir.path(), &["add", "-A"]);

        let staged = run(dir.path(), &registry, false).expect("check runs").code;
        assert_eq!(staged, 1, "staging is what makes the manifest visible");
    }

    /// The scope of `--diff` is the working tree and the index against HEAD,
    /// so a change that is already committed has nothing left to compare and
    /// is not checked at all.
    ///
    /// Recorded, not endorsed, and it is the sharpest edge this version has.
    /// The same content blocks while it is uncommitted and passes the moment
    /// it is committed, so a CI job that clones a branch and runs the gate is
    /// permanently green: the checkout is clean, `git diff HEAD` lists
    /// nothing, and the run exits 0 having looked at no manifest. That is a
    /// silent no-op — the failure this project exists to catch — reached
    /// through the ordinary way people wire a gate into CI.
    ///
    /// Not fixed here, because fixing it is a new scope rather than a
    /// correction: the design spec reserves `plumb check --repo` for
    /// comparing a commit range, and inventing it under `--diff` would change
    /// what every existing invocation means. Fixed by disclosure instead —
    /// the README says plainly what `--diff` compares and names `--repo` as
    /// unimplemented.
    ///
    /// Whoever implements `--repo` should leave this test alone: it pins
    /// `--diff`, which is still meant to mean the working diff. What must
    /// change is the README, once the tool has an answer for a commit.
    #[test]
    fn a_committed_change_is_outside_the_scope_of_diff() {
        let dir = repo_with_requirements("ruff==0.4.0\n");
        write(
            dir.path(),
            "requirements.txt",
            "ruff==0.4.0\nrequestz==2.31.0\n",
        );
        let registry = StubRegistry::default();

        // Uncommitted, the gate blocks on it.
        let uncommitted = run(dir.path(), &registry, false).expect("check runs").code;
        assert_eq!(
            uncommitted, 1,
            "the finding must exist before it can vanish"
        );

        run_git(dir.path(), &["add", "-A"]);
        run_git(
            dir.path(),
            &["commit", "-m", "add a package that does not exist"],
        );
        let registry = StubRegistry::default();

        let outcome = run(dir.path(), &registry, false).expect("check runs");
        let (output, committed) = (outcome.report, outcome.code);

        assert_eq!(
            committed, 0,
            "recorded, not endorsed: committing the change is what hides it"
        );
        assert!(output.contains("no findings"), "output was: {output}");
        assert!(
            registry.asked().is_empty(),
            "nothing was looked up at all: {:?}",
            registry.asked()
        );
    }

    /// A partially staged manifest is checked on the side that is not being
    /// committed.
    ///
    /// Recorded, not endorsed. `git diff HEAD` lists a path when *either* the
    /// index or the worktree differs from HEAD, and the after-side is then
    /// read from the **worktree**. For a manifest that is staged one way and
    /// left another way on disk those are different files, and the gate reads
    /// the one that is not about to be committed. Here the commit lands with a
    /// package PyPI does not have, and the gate said nothing.
    ///
    /// This defeats the pre-commit use case specifically, which is the one
    /// place a gate on a working diff is meant to be wired in.
    ///
    /// Not fixed in this version, and the obvious fix is worse than it looks.
    /// Reading the after-side from `git show :<path>` — the index — regresses
    /// the case the README leads with: an ordinary unstaged edit has an index
    /// copy identical to HEAD, so it would add nothing and report nothing.
    /// Measured on git 2.47.1, with `requirements.txt` edited and not staged:
    ///
    /// ```text
    /// git diff --name-only HEAD  ->  requirements.txt
    /// cat requirements.txt       ->  ruff==0.4.0
    ///                                plumbnope-zq7x4v==1.0
    /// git show :requirements.txt ->  ruff==0.4.0
    /// ```
    ///
    /// So the honest fix is the union of the index and the worktree as the
    /// after-side, which changes what "added" means and needs its own round of
    /// evidence. Until then the README says the pre-commit story is not yet
    /// honest.
    #[test]
    fn a_partially_staged_manifest_is_checked_on_the_wrong_side() {
        let dir = repo_with_requirements("ruff==0.4.0\n");
        write(
            dir.path(),
            "requirements.txt",
            "ruff==0.4.0\nrequestz==2.31.0\n",
        );
        run_git(dir.path(), &["add", "requirements.txt"]);
        // The worktree goes back to what HEAD holds; the index keeps the
        // addition. `git commit` will commit the index.
        write(dir.path(), "requirements.txt", "ruff==0.4.0\n");
        let registry = StubRegistry::default();

        let outcome = run(dir.path(), &registry, false).expect("check runs");

        assert_eq!(
            outcome.code, 0,
            "recorded, not endorsed: the staged addition is invisible"
        );
        assert!(
            registry.asked().is_empty(),
            "the staged name was never looked up: {:?}",
            registry.asked()
        );

        // And the commit lands with it, which is what makes this worth pinning
        // rather than merely noting.
        run_git(dir.path(), &["commit", "-m", "land it"]);
        let committed = git::show_at_head(dir.path(), "requirements.txt")
            .expect("show succeeds")
            .unwrap_or_default();
        assert!(
            committed.contains("requestz"),
            "the fixture did not actually commit the staged addition: {committed:?}"
        );
    }

    /// One byte that is not UTF-8 used to uncheck a whole manifest in silence.
    ///
    /// `read_to_string(...).unwrap_or_default()` made `after` empty, so no
    /// name was looked up and the run exited 0 with nothing on either stream.
    /// The verdict is still 0 — a file this process cannot decode is not
    /// evidence that a package does not exist — but the run now says which
    /// file it gave up on.
    ///
    /// The registry log is the load-bearing assertion. A version that decoded
    /// lossily would also exit 0 here and would look fixed, while having
    /// quietly turned `caf<U+FFFD>` into a name; asserting that nothing was
    /// asked pins that the file was skipped rather than mangled.
    #[test]
    fn a_manifest_that_is_not_utf8_is_reported_rather_than_skipped_silently() {
        let dir = repo_with_requirements("ruff==0.4.0\n");
        // `\xe9` is `é` in Latin-1, and is not valid UTF-8 on its own.
        fs::write(
            dir.path().join("requirements.txt"),
            b"# caf\xe9 pinned\nruff==0.4.0\nrequestz==2.31.0\n",
        )
        .expect("write");
        let registry = StubRegistry::default();

        let outcome = run(dir.path(), &registry, false).expect("check runs");

        assert_eq!(
            outcome.code, 0,
            "an undecodable file is not evidence about a package"
        );
        assert!(registry.asked().is_empty(), "asked: {:?}", registry.asked());
        assert_eq!(outcome.unreadable.len(), 1, "{:?}", outcome.unreadable);
        assert_eq!(outcome.unreadable[0].path, "requirements.txt");
        assert!(
            outcome.unreadable[0].reason.contains("UTF-8"),
            "reason was: {}",
            outcome.unreadable[0].reason
        );
    }

    /// The other half, and the one a fix for the test above could easily
    /// break. `git diff HEAD` lists a manifest the change *deleted*, which is
    /// then not there to read — an ordinary thing to do, saying nothing about
    /// any package. Reporting it would put a line on the stderr of every run
    /// that tidied up a requirements file, and a note that fires on ordinary
    /// work is one every reader learns to skip.
    #[test]
    fn a_manifest_the_change_deletes_is_not_reported_as_unreadable() {
        let dir = repo_with(&[
            ("requirements.txt", "ruff==0.4.0\n"),
            ("requirements-dev.txt", "pytest==8.0\n"),
        ]);
        fs::remove_file(dir.path().join("requirements-dev.txt")).expect("remove");
        let registry = StubRegistry::default();

        let outcome = run(dir.path(), &registry, false).expect("check runs");

        assert_eq!(outcome.code, 0);
        assert!(
            outcome.unreadable.is_empty(),
            "a deletion was reported as a failure to read: {:?}",
            outcome.unreadable
        );
    }

    /// A run with nothing wrong reports nothing, so the note stays meaningful.
    #[test]
    fn a_readable_manifest_produces_no_note() {
        let dir = repo_with_requirements("ruff==0.4.0\n");
        write(
            dir.path(),
            "requirements.txt",
            "ruff==0.4.0\nrequestz==2.31.0\n",
        );

        let outcome = run(dir.path(), &StubRegistry::default(), false).expect("check runs");

        assert_eq!(outcome.code, 1);
        assert!(outcome.unreadable.is_empty(), "{:?}", outcome.unreadable);
    }

    /// A directory with a space in its name is ordinary, and git terminates
    /// the path with a tab when it contains one. End to end because that is
    /// where it bites: the parser hands on a path ending in a tab, the
    /// manifest dispatch does not recognise it, and the gate reports a clean
    /// run over a manifest it never opened.
    #[test]
    fn a_manifest_under_a_path_containing_a_space_is_checked() {
        let dir = repo_with(&[("my project/requirements.txt", "ruff==0.4.0\n")]);
        write(
            dir.path(),
            "my project/requirements.txt",
            "ruff==0.4.0\nrequestz==2.31.0\n",
        );

        let outcome = run(dir.path(), &StubRegistry::default(), false).expect("check runs");
        let (output, code) = (outcome.report, outcome.code);

        assert_eq!(code, 1, "output was: {output}");
        assert!(
            output.contains("my project/requirements.txt"),
            "output was: {output}"
        );
    }

    /// The same silent skip as the test above, reached a different way: git
    /// escapes a non-ASCII path unless told not to, and the escaped form
    /// matches no manifest. A repository whose directories are not spelled in
    /// English is not an edge case.
    #[test]
    fn a_manifest_under_a_non_ascii_path_is_checked() {
        let dir = repo_with(&[("café/requirements.txt", "ruff==0.4.0\n")]);
        write(
            dir.path(),
            "café/requirements.txt",
            "ruff==0.4.0\nrequestz==2.31.0\n",
        );

        let outcome = run(dir.path(), &StubRegistry::default(), false).expect("check runs");
        let (output, code) = (outcome.report, outcome.code);

        assert_eq!(code, 1, "output was: {output}");
        assert!(
            output.contains("café/requirements.txt"),
            "output was: {output}"
        );
    }

    /// A manifest moved with no edit is checked at its new path.
    ///
    /// This is the one verdict the switch to a name listing changed, pinned at
    /// the level where it is visible. Git pairs the deletion and the addition
    /// as a rename, and at 100% similarity the section carries `rename from` /
    /// `rename to` and no `+++` line at all — so the manifest was invisible.
    /// Now it is read at its new path, where nothing exists at HEAD, so every
    /// name in it counts as added.
    ///
    /// The blocking finding here is the cost of that, not an accident:
    /// `requestz` was in the manifest before the move and is still in it
    /// after. See `git::changed_paths` for why the trade is taken anyway.
    ///
    /// Only an exact move does this. A manifest moved *and* edited is paired
    /// as an inexact rename, which does carry a `+++` line — measured at 76%
    /// similarity — so that shape was never invisible and is not what this
    /// covers.
    #[test]
    fn a_manifest_moved_without_editing_it_is_checked_at_its_new_path() {
        let dir = repo_with(&[("old/requirements.txt", "ruff==0.4.0\nrequestz==2.31.0\n")]);
        fs::create_dir_all(dir.path().join("new")).expect("create dir");
        run_git(
            dir.path(),
            &["mv", "old/requirements.txt", "new/requirements.txt"],
        );

        let outcome = run(dir.path(), &StubRegistry::default(), false).expect("check runs");
        let (output, code) = (outcome.report, outcome.code);

        assert_eq!(code, 1, "output was: {output}");
        assert!(
            output.contains("new/requirements.txt"),
            "output was: {output}"
        );
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

        let outcome = run(dir.path(), &StubRegistry::default(), false).expect("check runs");
        let (output, code) = (outcome.report, outcome.code);

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

        let outcome = run(dir.path(), &registry, false).expect("check runs");
        let (output, code) = (outcome.report, outcome.code);

        assert_eq!(code, 0, "output was: {output}");
        assert!(registry.asked().is_empty(), "asked: {:?}", registry.asked());
    }

    /// A clean tree is a clean run, not an error.
    #[test]
    fn an_unchanged_repository_reports_nothing() {
        let dir = repo_with_requirements("ruff==0.4.0\nrequestz==2.31.0\n");
        let registry = StubRegistry::default();

        let outcome = run(dir.path(), &registry, false).expect("check runs");
        let (output, code) = (outcome.report, outcome.code);

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
            findings.extend(collect(dir.path(), &registry).expect("check runs").0);
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
