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
//! A second, independent gate rides the same loop over changed paths: a
//! `.plumb/architecture.md` at the repository root, when one exists, is
//! parsed once and every non-manifest changed path is checked against it for
//! imports crossing a boundary the architecture does not declare. No
//! architecture file is not an error — most repositories have not written one
//! yet — so its absence disables this gate silently rather than warning about
//! a file that was never required to exist. It reports only violations the
//! change introduces, the same before/after shape as the package-existence
//! gate: a violation already present at HEAD is reported again on every
//! future touch to that file unless it is filtered, which mutes the gate on
//! exactly the projects that would benefit most from it — one legacy
//! crossing, and every unrelated edit to that file warns forever.
//!
//! Failing to establish anything is not the same as having nothing to say, and
//! the two were conflated here once already. A manifest that could not be read
//! or was only partly understood still moves no verdict on any package, but it
//! is a Tier B finding — the run warns, so exit 0 keeps meaning "checked and
//! clean" rather than "checked, or silently skipped".
//!
//! A third gate runs once per invocation, after the loop rather than inside
//! it: whether `CLAUDE.md`, `AGENTS.md`, `.cursorrules` and
//! `.github/copilot-instructions.md` still match what `plumb adapters` would
//! render from `.plumb/` right now. All four `.plumb/` artefacts present is
//! the bar for checking at all — a partially adopted `.plumb/` is `plumb
//! init`'s business, not this one's — and a target that does not exist on
//! disk is not adopted, so it is not drift either. Only a target that exists
//! and differs is.

use crate::git;
use crate::registry::{Existence, Registry};
use anyhow::Result;
use plumb_core::architecture::{self, Architecture};
use plumb_core::boundaries;
use plumb_core::deps::{self, AddedDependency};
use plumb_core::finding::{Finding, Tier, exit_code};
use plumb_core::manifest::{self, Ecosystem, Gap};
use plumb_core::report::{render_json, render_text};
use std::collections::HashSet;
use std::path::Path;

/// The taxonomy section behind the package existence rule.
///
/// Named once rather than spelled at the emission site. Every finding must
/// cite the evidence for its rule, and a citation copied per call site is one
/// that drifts — `every_finding_names_a_section_of_the_taxonomy` checks this
/// against the taxonomy itself.
const PACKAGE_EXISTS_SOURCE: &str = "taxonomy §4.1";

/// What one run of the gate established.
pub struct Outcome {
    /// The rendered report, for stdout.
    pub report: String,
    /// `0` clean, `1` a Tier A violation, `2` warnings only.
    pub code: i32,
}

/// Runs the diff-scoped gates and returns the rendered report and exit code.
///
/// `registry` is a `&dyn Registry` so a caller can hand in a stub and stay
/// offline; it is one registry for the whole run, not one per file, because a
/// registry is where the cache and the unreachable-network latch live and both
/// are useless if they are thrown away between manifests.
pub fn run(repo: &Path, registry: &dyn Registry, json: bool) -> Result<Outcome> {
    let findings = collect(repo, registry)?;

    let report = if json {
        render_json(&findings)
    } else {
        render_text(&findings)
    };

    Ok(Outcome {
        report,
        code: exit_code(&findings),
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

/// Collects the findings for the working diff.
///
/// Separate from [`run`] so the findings can be inspected before rendering
/// flattens them into a string.
fn collect(repo: &Path, registry: &dyn Registry) -> Result<Vec<Finding>> {
    let mut findings = Vec::new();

    // Read once for the whole run, not once per path: a registry is reused
    // across manifests for the same reason (see `run`'s doc comment), and an
    // architecture is no different. Absent — most repositories have not
    // written one — the boundary gate is off for this run, silently: a
    // project that never adopted `.plumb/architecture.md` has made no claim
    // this gate could hold it to.
    let architecture: Option<Architecture> =
        std::fs::read_to_string(repo.join(".plumb").join("architecture.md"))
            .ok()
            .map(|text| architecture::parse(&text));

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
            // Not a manifest, so the package-existence gate has nothing to
            // say about it — but it may still be source the boundary gate
            // cares about, when an architecture was found above.
            //
            // `is_scannable` is asked before a single byte is read: both
            // filters it applies — a known source extension, attribution to
            // a declared module — are path-only, so a diff touching a file
            // neither filter would ever let through (an unrelated binary, a
            // file the architecture says nothing about) is never opened at
            // all. Reading it first and filtering after, as this used to,
            // meant a multi-gigabyte binary in the diff was read in full for
            // a scan whose answer was always going to be no findings.
            //
            // Unreadable here is skipped rather than reported. `read_manifest`
            // draws that line for manifests because a manifest Plumb cannot
            // read is a manifest whose added dependencies go unchecked, which
            // is worth a finding. A source file this gate cannot read is not
            // yet — coverage reporting for source files is a future gate's
            // job, not this one's.
            if let Some(architecture) = &architecture
                && boundaries::is_scannable(architecture, &path)
                && let Ok(bytes) = std::fs::read(repo.join(&path))
            {
                let text = String::from_utf8_lossy(&bytes);
                let current = boundaries::violations(architecture, &path, &text);

                // Reported only if it is new, the same shape of fix
                // `deps::added` already is for the manifest gate: a
                // violation already present at HEAD predates this change,
                // so it is legacy adoption noise, not something the change
                // did. Warning on it anyway is the gate-muting hazard Tier B
                // exists to avoid — a comment-only edit to a file that has
                // always had an undeclared import would warn forever, which
                // is exactly the kind of warning a reader learns to ignore
                // and then the whole gate gets turned off. Comparing by
                // `(import, to_module)` pair rather than by line number is
                // what keeps a line move from resurrecting a suppressed
                // warning.
                //
                // `git::show_at_head` is strict about UTF-8 and aborts the
                // whole run for the manifest path (see its doc comment);
                // that is disproportionate for a Tier B gate over a file
                // this gate is not even required to be able to read, so an
                // undecodable HEAD version is folded into the same "no text
                // at HEAD" case as a genuinely new file — both mean every
                // current violation counts as new.
                let at_head = match git::show_at_head(repo, &path) {
                    Ok(Some(head_text)) => boundaries::violations(architecture, &path, &head_text),
                    Ok(None) | Err(_) => Vec::new(),
                };
                let pre_existing: HashSet<(&str, &str)> = at_head
                    .iter()
                    .map(|violation| (violation.import.as_str(), violation.to_module.as_str()))
                    .collect();

                for violation in &current {
                    let key = (violation.import.as_str(), violation.to_module.as_str());
                    if !pre_existing.contains(&key) {
                        findings.push(boundary_violation(violation));
                    }
                }
            }
            continue;
        }

        // `git diff` paths are relative to the repository root and separated
        // by `/`, which is what `manifest::parse` splits on and what `join`
        // accepts on every platform this runs on.
        //
        // A manifest that cannot be read yields no added dependencies and so
        // no package findings. Failing open on the packages is still the right
        // direction — a file this process could not decode is not evidence
        // that a package does not exist — but failing open *quietly* is not.
        // This used to be `read_to_string(...).unwrap_or_default()`, and one
        // Latin-1 byte in a comment therefore unchecked the whole file:
        // `# cafe pinned` blocked, `# caf\xe9 pinned` exited 0 with nothing
        // said on either stream. An undecodable file is the same class as a
        // gap — Plumb established nothing about it — so it reports through the
        // same finding rather than a second channel.
        //
        // Note the asymmetry with `git::show_at_head`, which is strict about
        // UTF-8 and aborts the run with exit 70. The two sides of the same
        // comparison fail in opposite directions, and that is deliberate
        // rather than an oversight: 70 is outside the 0/1/2 contract, so the
        // before-side failure is unmistakable and can never be read as a clean
        // run. The after-side failure could be, which is why it is a finding.
        // Neither is silent; only one is fatal.
        let current = match read_manifest(&repo.join(&path)) {
            Ok(Some(text)) => text,
            Ok(None) => continue,
            Err(reason) => {
                findings.push(manifest_not_fully_read(&path, &reason));
                continue;
            }
        };
        let Some(after) = manifest::parse(&path, &current) else {
            continue;
        };

        for gap in &after.gaps {
            findings.push(manifest_not_fully_read(&path, &gap.describe()));
        }

        // `None` means the manifest is new in this change, so every name in it
        // is an addition.
        //
        // A manifest that exists at HEAD but does not parse yields a `Manifest`
        // with no names, so every current dependency reads as added and gets
        // looked up. The lookups are the bounded cost of that reading; the
        // finding is what keeps it honest, because "everything in this file is
        // new" is an artefact of the unparsed HEAD side, not a fact about the
        // change.
        let before = match git::show_at_head(repo, &path)? {
            None => None,
            Some(text) => match manifest::parse(&path, &text) {
                None => None,
                Some(head) => {
                    if head.gaps.contains(&Gap::Unparseable) {
                        findings.push(manifest_not_fully_read(
                            &path,
                            "the version at HEAD does not parse, so every dependency in it reads as newly added",
                        ));
                    }
                    Some(head.names)
                }
            },
        };

        for dependency in deps::added(&path, before.as_ref(), &after.names) {
            if registry.exists(&dependency.name) == Existence::Missing {
                findings.push(package_does_not_exist(&dependency));
            }
        }
    }

    // A third, independent gate: whether the generated instruction files
    // still match what `plumb adapters` would produce from `.plumb/` today.
    // Not folded into the per-path loop above, because drift is a property
    // of the repository's current state, not of any one changed path — a
    // change that touches nothing in `.plumb/` can still be the run that
    // first notices a drift introduced earlier and never checked. All four
    // artefacts present is the bar for checking at all; a partially adopted
    // `.plumb/` is init's business, not this gate's, so it says nothing
    // rather than guessing at a project name or rendering from gaps.
    if let Some(artefacts) = read_full_plumb(repo) {
        let project_name = plumb_core::layer1::project_name_of(&artefacts.architecture)
            .unwrap_or_else(|| {
                repo.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "project".to_string())
            });
        for adapter in plumb_core::adapters::render(&project_name, &artefacts) {
            // `read`, not `read_to_string`: a target that exists but is not
            // valid UTF-8 must still be compared, and bytes rather than
            // `String` is what lets that comparison happen without an
            // encoding decision — a target this process cannot decode is not
            // equal to a rendered `String` no matter how it is read, so it
            // counts as drifted rather than crashing or being skipped.
            match std::fs::read(repo.join(adapter.path)) {
                Ok(bytes) if !same_ignoring_line_endings(&bytes, adapter.content.as_bytes()) => {
                    findings.push(adapter_drift(adapter.path));
                }
                // Matches (up to line endings), or absent — absent is not
                // adopted, not drifted.
                Ok(_) | Err(_) => {}
            }
        }
    }

    Ok(findings)
}

/// Whether two byte strings are equal once every `\r` is dropped.
///
/// Git normalises line endings at checkout — `core.autocrlf` on Windows, or
/// any repository whose `.gitattributes` marks text files, materialises
/// committed LF content as CRLF on disk — while [`plumb_core::adapters::render`]
/// always produces LF-only content. A byte-for-byte comparison therefore
/// found drift on an ordinary clean checkout on Windows: this was caught by
/// dogfooding on this very repository, where `check --diff` warned on all
/// four generated files with nothing actually out of step with `.plumb/`. A
/// gate that warns on every Windows clone is a gate that gets disabled before
/// it catches anything real — the exact failure mode Tier B exists to avoid.
///
/// Filtering `\r` out of both sides, rather than only the disk side, keeps
/// the comparison symmetric: a repository with `autocrlf=input` normalises
/// committed CRLF to LF on checkout, so the disk side can just as easily be
/// the LF one. Staying at the byte level — never decoding to `String` — is
/// what keeps a non-UTF-8 disk file comparing rather than crashing, which
/// `a_generated_file_that_drifted_warns`'s sibling tests rely on.
fn same_ignoring_line_endings(disk: &[u8], rendered: &[u8]) -> bool {
    disk.iter()
        .filter(|&&b| b != b'\r')
        .eq(rendered.iter().filter(|&&b| b != b'\r'))
}

/// Reads all four `.plumb/` artefacts, if every one of them is present.
///
/// One artefact missing is not an error and not a finding: partial adoption
/// is what `plumb init` and `plumb adapters` are for, and this gate has
/// nothing to compare against until all four exist. Reading with
/// `read_to_string` rather than treating a decode failure as drift is
/// deliberate — an artefact this process cannot even read is not a claim
/// about what any generated file should contain, so it disables the gate the
/// same way absence does rather than manufacturing a comparison from nothing.
fn read_full_plumb(repo: &Path) -> Option<plumb_core::layer1::Artefacts> {
    let plumb_dir = repo.join(".plumb");
    let read = |name: &str| std::fs::read_to_string(plumb_dir.join(name)).ok();
    Some(plumb_core::layer1::Artefacts {
        architecture: read("architecture.md")?,
        conventions: read("conventions.md")?,
        invariants: read("invariants.md")?,
        index: read("index.md")?,
    })
}

/// A manifest Plumb recognised and did not fully read.
///
/// Tier B, so it warns and never blocks: a gate that fails on every project in
/// a format it has not learned yet is uninstalled before it catches anything.
/// The message names the file and what was missed, because a warning a reader
/// cannot act on is one they learn to skip.
fn manifest_not_fully_read(path: &str, detail: &str) -> Finding {
    Finding {
        rule: "manifest-understood".to_string(),
        tier: Tier::B,
        message: format!("{path} was not fully read: {detail}"),
        location: path.to_string(),
        source: "taxonomy §10".to_string(),
    }
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

/// A boundary crossing the architecture does not declare.
///
/// Tier B: the evidence (taxonomy §4.5) is a preprint, and path-based module
/// attribution plus line-based import extraction is a heuristic that needs
/// per-project calibration before it can block anything.
fn boundary_violation(violation: &boundaries::Violation) -> Finding {
    Finding {
        rule: "boundary".to_string(),
        tier: Tier::B,
        message: format!(
            "{} imports `{}`, but {} may not depend on {}",
            violation.file, violation.import, violation.from_module, violation.to_module
        ),
        location: violation.file.clone(),
        source: "taxonomy §4.5".to_string(),
    }
}

/// A generated instruction file that no longer matches its source.
///
/// Tier B, and it describes this repository's artefacts rather than the code
/// under inspection — the same honesty class as taxonomy §10. An agent reads
/// the generated file; when the file and `.plumb/` disagree, the agent is
/// working from rules nobody chose.
fn adapter_drift(path: &str) -> Finding {
    Finding {
        rule: "adapter-drift".to_string(),
        tier: Tier::B,
        message: format!("{path} does not match .plumb/; run `plumb adapters`"),
        location: path.to_string(),
        source: "taxonomy §11".to_string(),
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

    fn write_architecture(dir: &Path, body: &str) {
        fs::create_dir_all(dir.join(".plumb")).expect("create .plumb");
        fs::write(dir.join(".plumb").join("architecture.md"), body).expect("write");
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
    /// scope of `--diff` is the working tree against HEAD, and anything
    /// already committed is outside it — see
    /// `a_committed_change_is_outside_the_scope_of_diff`, which pins that
    /// directly rather than leaving it to a comment.
    ///
    /// Note what staging does and does not do here. It does not put the
    /// staged *content* in scope — `git diff HEAD` compares the working tree,
    /// not the index. What it does is make the file **tracked**, and an
    /// untracked file is not reported at all. That is the whole of the index's
    /// role in this gate, and it is what this test pins.
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

    /// The scope of `--diff` is the working tree against HEAD, so a change
    /// that is already committed has nothing left to compare and is not
    /// checked at all.
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

    /// A staged addition is invisible once the working tree matches HEAD
    /// again.
    ///
    /// Recorded, not endorsed. `changed_paths` runs `git diff --name-only -z
    /// HEAD` with no `--cached`, which compares the **working tree** against
    /// HEAD and says nothing about the index. So a path is listed only when
    /// the working tree differs, and an addition that is staged and then
    /// reverted on disk is not listed at all — the manifest is never opened.
    /// Measured on git 2.47.1, with the addition staged and the working tree
    /// restored to HEAD content:
    ///
    /// ```text
    /// git diff --cached --name-only HEAD  ->  requirements.txt
    /// git diff --name-only HEAD           ->  (nothing)
    /// ```
    ///
    /// The commit then lands the package, because `git commit` commits the
    /// index. This defeats the pre-commit use case specifically, which is the
    /// one place a gate on a working diff is meant to be wired in.
    ///
    /// An earlier version of this comment said `git diff HEAD` lists a path
    /// when *either* the index or the working tree differs from HEAD. That is
    /// false — it is the `--cached` form that consults the index — and the two
    /// lines above are what settles it. The assertions below were right for
    /// the wrong stated reason.
    ///
    /// Not fixed in this version, and the obvious fix is worse than it looks.
    /// Reading the after-side from `git show :<path>` — the index — regresses
    /// the case the README leads with: an ordinary unstaged edit has an index
    /// copy identical to HEAD, so it would add nothing and report nothing.
    /// Measured on the same git, with `requirements.txt` edited and not
    /// staged:
    ///
    /// ```text
    /// git diff --name-only HEAD  ->  requirements.txt
    /// cat requirements.txt       ->  ruff==0.4.0
    ///                                plumbnope-zq7x4v==1.0
    /// git show :requirements.txt ->  ruff==0.4.0
    /// ```
    ///
    /// So the honest fix is the union of the index and the working tree as the
    /// after-side, which changes what "added" means and needs its own round of
    /// evidence. Until then the README says the pre-commit story is not yet
    /// honest.
    #[test]
    fn a_staged_addition_is_invisible_once_the_working_tree_matches_head() {
        let dir = repo_with_requirements("ruff==0.4.0\n");
        write(
            dir.path(),
            "requirements.txt",
            "ruff==0.4.0\nrequestz==2.31.0\n",
        );
        run_git(dir.path(), &["add", "requirements.txt"]);
        // The working tree goes back to what HEAD holds; the index keeps the
        // addition. `git commit` will commit the index.
        write(dir.path(), "requirements.txt", "ruff==0.4.0\n");
        let registry = StubRegistry::default();

        // The path is not even listed, so this is a manifest never opened
        // rather than one read on the wrong side.
        assert!(
            git::changed_paths(dir.path())
                .expect("git succeeds")
                .is_empty(),
            "an index-only change must not be listed by `git diff HEAD`"
        );

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

    /// The other half, and the one that genuinely earns the phrase "checked on
    /// the wrong side".
    ///
    /// When the working tree differs from HEAD the path *is* listed, and the
    /// after-side is then read from the working tree — while `git commit`
    /// commits the index. A manifest staged one way and edited another is
    /// therefore checked on content that is not about to be committed: here
    /// the gate looks up `httpx`, which is in the working tree and will not be
    /// committed, and never looks at `requestz`, which is staged and will be.
    ///
    /// Recorded, not endorsed, and it is the case that survives any fix aimed
    /// only at listing more paths — the path is already listed. Only reading
    /// the after-side as the union of the index and the working tree closes
    /// it.
    #[test]
    fn a_manifest_staged_one_way_and_edited_another_is_checked_on_the_working_tree() {
        let dir = repo_with_requirements("ruff==0.4.0\n");
        write(
            dir.path(),
            "requirements.txt",
            "ruff==0.4.0\nrequestz==2.31.0\n",
        );
        run_git(dir.path(), &["add", "requirements.txt"]);
        // Same path, different content on disk: this is what gets read.
        write(
            dir.path(),
            "requirements.txt",
            "ruff==0.4.0\nhttpx==0.27.0\n",
        );
        let registry = StubRegistry::default();

        let outcome = run(dir.path(), &registry, false).expect("check runs");

        assert_eq!(
            outcome.code, 0,
            "recorded, not endorsed: the staged addition was never examined"
        );
        assert_eq!(
            registry.asked(),
            vec!["httpx"],
            "the gate asked about the working-tree name and not the staged one"
        );

        run_git(dir.path(), &["commit", "-m", "land the staged version"]);
        let committed = git::show_at_head(dir.path(), "requirements.txt")
            .expect("show succeeds")
            .unwrap_or_default();
        assert!(
            committed.contains("requestz") && !committed.contains("httpx"),
            "the commit should carry the staged content, not the checked one: {committed:?}"
        );
    }

    /// One byte that is not UTF-8 used to uncheck a whole manifest in silence.
    ///
    /// `read_to_string(...).unwrap_or_default()` made `after` empty, so no
    /// name was looked up and the run exited 0 with nothing on either stream.
    /// An undecodable file is still not evidence about any package — no Tier A
    /// violation — but it is a finding, so the exit code is 2 and the report
    /// names the file and the byte that stopped the read.
    ///
    /// The registry log is the load-bearing assertion. A version that decoded
    /// lossily would produce no coverage finding and would look fixed, while
    /// having quietly turned `caf<U+FFFD>` into a name; asserting that nothing
    /// was asked pins that the file was skipped rather than mangled.
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
        let (output, code) = (outcome.report, outcome.code);

        assert_eq!(
            code, 2,
            "an undecodable file warns without blocking; output was: {output}"
        );
        assert!(registry.asked().is_empty(), "asked: {:?}", registry.asked());
        assert!(output.contains("requirements.txt"), "output was: {output}");
        assert!(output.contains("UTF-8"), "output was: {output}");
    }

    /// The other half, and the one a fix for the test above could easily
    /// break. `git diff HEAD` lists a manifest the change *deleted*, which is
    /// then not there to read — an ordinary thing to do, saying nothing about
    /// any package. A finding for it would warn on every run that tidied up a
    /// requirements file, and a warning that fires on ordinary work is one
    /// every reader learns to skip.
    #[test]
    fn a_manifest_the_change_deletes_is_not_reported_as_unreadable() {
        let dir = repo_with(&[
            ("requirements.txt", "ruff==0.4.0\n"),
            ("requirements-dev.txt", "pytest==8.0\n"),
        ]);
        fs::remove_file(dir.path().join("requirements-dev.txt")).expect("remove");
        let registry = StubRegistry::default();

        let outcome = run(dir.path(), &registry, false).expect("check runs");

        assert_eq!(
            outcome.code, 0,
            "a deletion was reported as a failure to read; output was: {}",
            outcome.report
        );
    }

    /// A run with nothing wrong warns about nothing, so the finding stays
    /// meaningful.
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
        assert!(
            !outcome.report.contains("manifest-understood"),
            "a fully read manifest produced a coverage finding: {}",
            outcome.report
        );
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

    #[test]
    fn an_undeclared_cross_module_import_warns() {
        let dir = repo_with(&[("crates/core/src/lib.rs", "// empty\n")]);
        write_architecture(
            dir.path(),
            "## crates/core\n\nMay depend on: (none).\n\n## crates/cli\n\nMay depend on: core.\n",
        );
        write(dir.path(), "crates/core/src/lib.rs", "use cli::main;\n");

        let outcome = run(dir.path(), &StubRegistry::default(), false).expect("check runs");
        let (output, code) = (outcome.report, outcome.code);

        assert_eq!(code, 2, "output was: {output}");
        assert!(output.contains("boundary"), "output was: {output}");
        assert!(
            output.contains("crates/core/src/lib.rs"),
            "output was: {output}"
        );
    }

    #[test]
    fn a_declared_dependency_does_not_warn() {
        let dir = repo_with(&[("crates/cli/src/main.rs", "// empty\n")]);
        write_architecture(
            dir.path(),
            "## crates/core\n\nMay depend on: (none).\n\n## crates/cli\n\nMay depend on: core.\n",
        );
        write(dir.path(), "crates/cli/src/main.rs", "use core::check;\n");

        let outcome = run(dir.path(), &StubRegistry::default(), false).expect("check runs");

        assert_eq!(outcome.code, 0, "output was: {}", outcome.report);
    }

    #[test]
    fn without_an_architecture_no_boundary_checking_happens() {
        let dir = repo_with(&[("crates/core/src/lib.rs", "// empty\n")]);
        write(dir.path(), "crates/core/src/lib.rs", "use cli::main;\n");

        let outcome = run(dir.path(), &StubRegistry::default(), false).expect("check runs");

        assert_eq!(outcome.code, 0, "output was: {}", outcome.report);
    }

    /// The gate-muting hazard: the boundary gate used to scan the whole
    /// working-tree file, so a legacy import that already violated the
    /// boundary at HEAD re-warned on every touch to the file — even a
    /// comment-only one, which is what this pins. The manifest gate already
    /// solved this class of false positive by comparing before against
    /// after and reporting only additions; the boundary gate must do the
    /// same, or a project with one pre-existing crossing gets a warning on
    /// every unrelated edit to that file forever, which is exactly how a
    /// Tier B gate gets muted.
    #[test]
    fn a_pre_existing_violation_does_not_re_warn_on_an_unrelated_touch() {
        let dir = repo_with(&[("crates/core/src/lib.rs", "use cli::main;\n")]);
        write_architecture(
            dir.path(),
            "## crates/core\n\nMay depend on: (none).\n\n## crates/cli\n\nMay depend on: core.\n",
        );
        // The violation is already there at HEAD; this change adds only an
        // unrelated comment line.
        write(
            dir.path(),
            "crates/core/src/lib.rs",
            "use cli::main;\n// unrelated comment\n",
        );

        let outcome = run(dir.path(), &StubRegistry::default(), false).expect("check runs");

        assert_eq!(
            outcome.code, 0,
            "a violation that predates the change re-warned on an unrelated touch: {}",
            outcome.report
        );
    }

    /// The other half: a change that adds a genuinely new undeclared import
    /// on top of a pre-existing one reports only the new one. Line moves
    /// must not resurrect a suppressed warning either, which is why the
    /// comparison is by `(import, to_module)` pair rather than by line
    /// number — not separately pinned here because nothing in this gate's
    /// extraction is line-position-sensitive to begin with.
    #[test]
    fn a_new_import_is_reported_while_a_pre_existing_one_is_not() {
        let dir = repo_with(&[("crates/core/src/lib.rs", "use cli::main;\n")]);
        write_architecture(
            dir.path(),
            "## crates/core\n\nMay depend on: (none).\n\n\
             ## crates/cli\n\nMay depend on: core.\n\n\
             ## crates/extra\n\nMay depend on: (none).\n",
        );
        write(
            dir.path(),
            "crates/core/src/lib.rs",
            "use cli::main;\nuse extra::thing;\n",
        );

        let outcome = run(dir.path(), &StubRegistry::default(), false).expect("check runs");
        let (output, code) = (outcome.report, outcome.code);

        assert_eq!(code, 2, "output was: {output}");
        assert_eq!(
            output.matches("[boundary]").count(),
            1,
            "the pre-existing violation must not be reported alongside the new one: {output}"
        );
        assert!(output.contains("extra"), "output was: {output}");
        assert!(
            !output.contains("may not depend on crates/cli"),
            "the pre-existing violation re-warned: {output}"
        );
    }

    /// `git::show_at_head` is strict about UTF-8 and aborts the whole run
    /// with exit 70 for the manifest path — see its doc comment. That is
    /// disproportionate here: a HEAD version of a scanned file this process
    /// cannot decode is not a reason to abort a Tier B gate, so it is read
    /// as "no text at HEAD", the same as a new file, meaning every current
    /// violation counts as new rather than the run failing outside the 0/1/2
    /// contract.
    #[test]
    fn an_undecodable_head_version_of_a_scanned_file_does_not_abort_the_run() {
        let dir = tempfile::tempdir().expect("temp dir");
        run_git(dir.path(), &["init", "-b", "main"]);
        run_git(dir.path(), &["config", "user.email", "test@example.com"]);
        run_git(dir.path(), &["config", "user.name", "Test"]);
        fs::create_dir_all(dir.path().join("crates/core/src")).expect("create dirs");
        // `\xFF` is not valid UTF-8 in any position.
        fs::write(
            dir.path().join("crates/core/src/lib.rs"),
            [b'/', b'/', 0xFF, b'\n'],
        )
        .expect("write");
        run_git(dir.path(), &["add", "-A"]);
        run_git(dir.path(), &["commit", "-m", "initial"]);
        write_architecture(
            dir.path(),
            "## crates/core\n\nMay depend on: (none).\n\n## crates/cli\n\nMay depend on: core.\n",
        );
        write(dir.path(), "crates/core/src/lib.rs", "use cli::main;\n");

        let outcome = run(dir.path(), &StubRegistry::default(), false).expect("check runs");
        let (output, code) = (outcome.report, outcome.code);

        assert_eq!(
            code, 2,
            "an undecodable HEAD version must not abort the run, and the current \
             violation, having no comparable HEAD text, must still be reported: {output}"
        );
        assert!(output.contains("boundary"), "output was: {output}");
    }

    /// A file `boundaries::is_scannable` would reject — no declared module
    /// owns `tools/` here — must never trip the gate, however unreadable its
    /// content is. This is the shape of file the filter exists to keep
    /// `std::fs::read` away from entirely: before `is_scannable` gated the
    /// read, a diff-wide multi-gigabyte binary under no declared module was
    /// still read in full for a scan that could only ever answer "no
    /// findings". The invalid UTF-8 byte here is not the point — it stands
    /// in for "content this gate has no business opening" and pins that
    /// opening it anyway would at least not crash the run.
    #[test]
    fn a_file_under_no_declared_module_never_blocks_however_it_is_encoded() {
        let dir = repo_with(&[("tools/gen.rs", "// empty\n")]);
        write_architecture(dir.path(), "## crates/core\n\nMay depend on: (none).\n");
        fs::write(
            dir.path().join("tools/gen.rs"),
            [b'u', b's', b'e', b' ', 0xFF, b';', b'\n'],
        )
        .expect("write");

        let outcome = run(dir.path(), &StubRegistry::default(), false).expect("check runs");

        assert_eq!(outcome.code, 0, "output was: {}", outcome.report);
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

        // Every emission site in `collect` that a UTF-8 fixture can reach.
        // A finding built on a path not covered here would escape the check, so
        // a new emission site means a new row.
        //
        // The sentence used to claim all of them and list two, which is the
        // kind of comment that makes a gap read as covered. There are four
        // sites: the package finding, a gap on the after-side, the
        // HEAD-unparseable finding, and a file that could not be decoded. The
        // first three are below. The fourth needs a body that is not valid
        // UTF-8, which a `&str` table cannot hold; it builds the same
        // `manifest_not_fully_read` finding as the two rows that follow it —
        // same rule, same tier, same citation — and is pinned end to end by
        // `a_manifest_that_is_not_utf8_is_reported_rather_than_skipped_silently`.
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
            // The coverage finding: a Poetry-shaped manifest is recognised and
            // not read, which is an emission site of its own.
            (
                "pyproject.toml",
                "[project]\nname = \"demo\"\n",
                "[tool.poetry.dependencies]\nrequests = \"^2.0\"\n",
            ),
            // The HEAD-unparseable finding, which is built on the before-side
            // and so is reached by no row above: the after-side here parses and
            // has no gap of its own.
            (
                "pyproject.toml",
                "[project]\nname = \"demo\"\n{{{",
                "[project]\nname = \"demo\"\ndependencies = [\"ruff\"]\n",
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

        // The boundary finding is emitted from a different site than the
        // manifest scenarios above and needs an architecture in place.
        let dir = repo_with(&[("crates/core/src/lib.rs", "// empty\n")]);
        write_architecture(
            dir.path(),
            "## crates/core\n\nMay depend on: (none).\n\n## crates/cli\n\nMay depend on: core.\n",
        );
        write(dir.path(), "crates/core/src/lib.rs", "use cli::main;\n");
        let boundary_findings = collect(dir.path(), &registry).expect("check runs");
        assert_eq!(
            boundary_findings.len(),
            1,
            "the boundary scenario emitted no finding: {boundary_findings:#?}"
        );
        findings.extend(boundary_findings);

        // The drift finding is emitted from a fourth site and needs a full
        // `.plumb/` plus at least one generated target on disk. Only
        // `CLAUDE.md` is generated here, rather than all four targets via
        // `generate_adapters`: every rendered target embeds all four
        // artefacts in its body, so drifting one artefact after generating
        // all four would make all four targets differ and this scenario
        // would no longer prove "exactly one finding, exactly one citation" —
        // it would still pass, but by coincidence rather than by design.
        let dir = repo_with_requirements("ruff==0.4.0\n");
        write_full_plumb(dir.path());
        {
            use plumb_core::adapters::render;
            use plumb_core::layer1::Artefacts;
            let read = |name: &str| {
                fs::read_to_string(dir.path().join(".plumb").join(name)).expect("read artefact")
            };
            let artefacts = Artefacts {
                architecture: read("architecture.md"),
                conventions: read("conventions.md"),
                invariants: read("invariants.md"),
                index: read("index.md"),
            };
            let claude_md = render("demo", &artefacts)
                .into_iter()
                .find(|adapter| adapter.path == "CLAUDE.md")
                .expect("render includes CLAUDE.md");
            fs::write(dir.path().join(claude_md.path), claude_md.content).expect("write adapter");
        }
        let conventions = dir.path().join(".plumb").join("conventions.md");
        let mut text = fs::read_to_string(&conventions).expect("read");
        text.push_str("\n## New rule\n\nAdded after generation.\n");
        fs::write(&conventions, text).expect("write");
        let drift_findings = collect(dir.path(), &registry).expect("check runs");
        assert_eq!(
            drift_findings.len(),
            1,
            "the drift scenario emitted the wrong number of findings: {drift_findings:#?}"
        );
        findings.extend(drift_findings);

        // The stale-index finding is emitted from a fifth site: a scanned
        // index that a fresh scan of the tracked sources contradicts.
        let dir = repo_with(&[("src/lib.rs", "pub fn old() {}\n")]);
        write_full_plumb(dir.path());
        scan_index_into_plumb(dir.path());
        write(dir.path(), "src/lib.rs", "pub fn old() {}\npub fn brand_new() {}\n");
        run_git(dir.path(), &["add", "-A"]);
        run_git(dir.path(), &["commit", "-m", "grow the surface"]);
        let stale_index_findings = collect(dir.path(), &registry).expect("check runs");
        assert_eq!(
            stale_index_findings.len(),
            1,
            "the stale-index scenario emitted the wrong number of findings: {stale_index_findings:#?}"
        );
        findings.extend(stale_index_findings);

        for finding in &findings {
            assert!(!finding.rule.is_empty(), "unnamed rule: {finding:#?}");
            assert!(!finding.message.is_empty(), "empty message: {finding:#?}");
            assert!(!finding.location.is_empty(), "empty location: {finding:#?}");

            let section = finding
                .source
                .strip_prefix("taxonomy §")
                .unwrap_or_else(|| panic!("source does not cite the taxonomy: {finding:#?}"));
            // The trailing space or dot-space anchors the number: without one
            // a citation of §4.1 would be satisfied by the heading for §4.10.
            // Both spellings appear in the taxonomy — subsections read
            // "### 4.1 Package hallucination", top-level sections read
            // "## 10. Coverage reporting" — and neither anchor can be
            // satisfied by a longer number.
            let has_heading = |anchored: &str| {
                taxonomy.lines().any(|line| {
                    line.starts_with('#')
                        && line
                            .trim_start_matches('#')
                            .trim_start()
                            .starts_with(anchored)
                })
            };
            assert!(
                has_heading(&format!("{section} ")) || has_heading(&format!("{section}. ")),
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
    ///
    /// The gaps assertion is the second half of "a function of the filename
    /// alone", and it is not decoration. The probe hands `parse` a body no user
    /// wrote, so anything the probe's `Manifest` says about that body is an
    /// artefact of this call site. An empty string is a legal, empty TOML
    /// document and an empty requirements file, so it must yield no gap; an
    /// implementation that read it as unparseable would attach a gap to every
    /// changed manifest in every repository, and the moment a caller reports
    /// gaps that becomes a warning on every run. `is_some()` alone passes
    /// against that implementation, which is why the emptiness is asserted
    /// here rather than assumed.
    #[test]
    fn a_manifest_is_recognised_from_its_path_alone() {
        for path in [
            "requirements.txt",
            "requirements-dev.txt",
            "pyproject.toml",
            "packages/api/pyproject.toml",
        ] {
            let probed = manifest::parse(path, "")
                .unwrap_or_else(|| panic!("{path} was not recognised from its name"));
            assert!(
                probed.gaps.is_empty(),
                "the empty-body probe invented a gap for {path}: {:?}",
                probed.gaps
            );
        }
        for path in ["src/app.py", "README.md", "requirements_notes.txt"] {
            assert!(
                manifest::parse(path, "ruff==0.4.0\n").is_none(),
                "{path} was treated as a manifest"
            );
        }
    }

    /// A Poetry manifest used to yield an empty dependency set and exit 0 —
    /// a silent false negative on a blocking gate. The gap is now a Tier B
    /// finding: the run warns, names the file and the shape it did not read.
    #[test]
    fn a_manifest_in_an_unread_format_warns_rather_than_passing() {
        let dir = repo_with(&[("pyproject.toml", "[project]\nname = \"demo\"\n")]);
        write(
            dir.path(),
            "pyproject.toml",
            "[tool.poetry.dependencies]\nrequests = \"^2.0\"\n",
        );

        let outcome = run(dir.path(), &StubRegistry::default(), false).expect("check runs");
        let (output, code) = (outcome.report, outcome.code);

        assert_eq!(code, 2, "output was: {output}");
        assert!(output.contains("pyproject.toml"), "output was: {output}");
        assert!(
            output.contains("tool.poetry.dependencies"),
            "output was: {output}"
        );
    }

    /// A manifest that does not parse *at HEAD* makes the whole after-side
    /// read as newly added, and the run says so.
    ///
    /// This is the emission site with the least to hold it up, because nothing
    /// about it is visible in the verdict on an ordinary day: the after-side
    /// parses, every name in it resolves, and the run would exit 0 with no
    /// finding at all. What it would be hiding is that "these dependencies are
    /// new" is an artefact of the unparsed before-side — the change added
    /// nothing, it only repaired a broken file — and on a repository whose
    /// packages come from a private index, that artefact is a wall of blocking
    /// findings with no stated cause.
    ///
    /// The registry log is the load-bearing half. Asserting exit 2 alone
    /// passes against an implementation that warns for any reason; asserting
    /// that `ruff` was looked up pins that the name really was treated as
    /// added, which is the thing the sentence claims. `ruff` resolves in the
    /// stub, so the verdict stays 2 and the coverage finding is not hidden
    /// behind a Tier A one.
    #[test]
    fn a_manifest_that_does_not_parse_at_head_says_so() {
        let dir = repo_with(&[("pyproject.toml", "[project]\nname = \"demo\"\n{{{")]);
        write(
            dir.path(),
            "pyproject.toml",
            "[project]\nname = \"demo\"\ndependencies = [\"ruff\"]\n",
        );
        let registry = StubRegistry::default();

        let outcome = run(dir.path(), &registry, false).expect("check runs");
        let (output, code) = (outcome.report, outcome.code);

        assert_eq!(code, 2, "output was: {output}");
        assert!(output.contains("pyproject.toml"), "output was: {output}");
        assert!(
            output.contains("HEAD"),
            "the report must name the side that failed to parse: {output}"
        );
        assert_eq!(
            registry.asked(),
            vec!["ruff"],
            "an unparsed HEAD is what makes an unchanged dependency read as added"
        );
    }

    /// The other direction, and what keeps the test above from being satisfied
    /// by warning on every `pyproject.toml`: when HEAD parses, no HEAD finding
    /// is emitted, and a dependency that was already there is not looked up.
    #[test]
    fn a_manifest_that_parses_at_head_produces_no_head_finding() {
        let dir = repo_with(&[(
            "pyproject.toml",
            "[project]\nname = \"demo\"\ndependencies = [\"ruff\"]\n",
        )]);
        write(
            dir.path(),
            "pyproject.toml",
            "[project]\nname = \"demo2\"\ndependencies = [\"ruff\"]\n",
        );
        let registry = StubRegistry::default();

        let outcome = run(dir.path(), &registry, false).expect("check runs");

        assert_eq!(outcome.code, 0, "output was: {}", outcome.report);
        assert!(
            !outcome.report.contains("HEAD"),
            "output was: {}",
            outcome.report
        );
        assert!(registry.asked().is_empty(), "asked: {:?}", registry.asked());
    }

    /// The structural rule reaches the report, and the report names the table.
    ///
    /// End to end rather than at the parser, because the value of naming the
    /// table is only realised in the sentence a user reads. `[tool.pdm]` is one
    /// of five tools that reached exit 0 with `plumb: no findings` over a
    /// package that does not exist, and none of them is named anywhere in the
    /// code — the path in the message comes from the file.
    #[test]
    fn a_dependency_table_under_tool_warns_and_names_it() {
        let dir = repo_with(&[("pyproject.toml", "[project]\nname = \"demo\"\n")]);
        write(
            dir.path(),
            "pyproject.toml",
            "[project]\nname = \"demo\"\n\n[tool.pdm.dev-dependencies]\ntest = [\"requestz\"]\n",
        );
        let registry = StubRegistry::default();

        let outcome = run(dir.path(), &registry, false).expect("check runs");
        let (output, code) = (outcome.report, outcome.code);

        assert_eq!(code, 2, "output was: {output}");
        assert!(
            output.contains("tool.pdm.dev-dependencies"),
            "output was: {output}"
        );
        // The gap reports the blind spot; it does not close it. The name inside
        // the unread table is still never looked up, and saying so here keeps
        // the warning from being mistaken for coverage.
        assert!(registry.asked().is_empty(), "asked: {:?}", registry.asked());
    }

    /// The other direction: a manifest read in full must not start warning.
    /// A coverage note that fires on ordinary work is one every reader learns
    /// to skip, and then the gate is uninstalled.
    #[test]
    fn an_ordinary_manifest_still_passes_silently() {
        let dir = repo_with(&[("requirements.txt", "ruff==0.4.0\n")]);
        write(
            dir.path(),
            "requirements.txt",
            "ruff==0.4.0\nhttpx==0.27.0\n",
        );

        let outcome = run(dir.path(), &StubRegistry::default(), false).expect("check runs");

        assert_eq!(outcome.code, 0, "output was: {}", outcome.report);
    }

    /// Tier A must keep winning. A file can carry both a missing package and a
    /// gap; the verdict is 1, not 2, and both stories appear in the report.
    #[test]
    fn a_missing_package_still_blocks_when_the_same_file_has_a_gap() {
        let dir = repo_with(&[("requirements.txt", "ruff==0.4.0\n")]);
        write(
            dir.path(),
            "requirements.txt",
            "ruff==0.4.0\nrequestz==2.31.0\n-r base.txt\n",
        );

        let outcome = run(dir.path(), &StubRegistry::default(), false).expect("check runs");
        let (output, code) = (outcome.report, outcome.code);

        assert_eq!(code, 1, "output was: {output}");
        assert!(output.contains("requestz"), "output was: {output}");
        assert!(output.contains("base.txt"), "output was: {output}");
    }

    /// An undecodable manifest is the same class as a gap — Plumb could not
    /// establish anything about this file — and now reports the same way: a
    /// Tier B finding and exit 2, not a stderr note and exit 0.
    #[test]
    fn a_manifest_that_cannot_be_decoded_warns_rather_than_reporting_on_stderr() {
        let dir = repo_with(&[("requirements.txt", "ruff==0.4.0\n")]);
        std::fs::write(
            dir.path().join("requirements.txt"),
            [b'#', b' ', 0xE9, b'\n', b'r', b'u', b'f', b'f', b'\n'],
        )
        .expect("write");

        let outcome = run(dir.path(), &StubRegistry::default(), false).expect("check runs");
        let (output, code) = (outcome.report, outcome.code);

        assert_eq!(code, 2, "output was: {output}");
        assert!(output.contains("requirements.txt"), "output was: {output}");
    }

    fn write_full_plumb(dir: &Path) {
        use plumb_core::layer1::{Bootstrap, greenfield};
        let artefacts = greenfield(&Bootstrap {
            project_name: "demo".to_string(),
            modules: vec!["core".to_string()],
            error_handling: "errors propagate".to_string(),
            test_shape: "one behaviour per test".to_string(),
        });
        fs::create_dir_all(dir.join(".plumb")).expect("create .plumb");
        for (name, content) in [
            ("architecture.md", &artefacts.architecture),
            ("conventions.md", &artefacts.conventions),
            ("invariants.md", &artefacts.invariants),
            ("index.md", &artefacts.index),
        ] {
            fs::write(dir.join(".plumb").join(name), content).expect("write artefact");
        }
    }

    /// Scans `dir`'s tracked files into `.plumb/index.md`, the same way
    /// `plumb index` does. A second copy of the scan loop rather than a call
    /// into `index_cmd::run`, because that entry point also requires and
    /// reports on `.plumb/`'s presence and writes progress lines to a
    /// `Write` — noise this fixture has no use for. `scan_index` in
    /// `index_cmd.rs` is the shared core both this helper's production
    /// counterpart (the stale-index gate) and `index_cmd::run` call, so the
    /// two cannot drift on what counts as a symbol; this test helper stays a
    /// thin duplicate of the same loop shape for that reason.
    fn scan_index_into_plumb(dir: &Path) {
        use plumb_core::index::{render, symbols};
        let mut entries = Vec::new();
        for path in crate::git::tracked_files(dir).expect("tracked files") {
            if let Ok(bytes) = fs::read(dir.join(&path)) {
                let text = String::from_utf8_lossy(&bytes);
                entries.push((path.clone(), symbols(&path, &text)));
            }
        }
        fs::write(dir.join(".plumb").join("index.md"), render(&entries)).expect("write index");
    }

    #[test]
    fn a_scanned_index_that_no_longer_matches_warns() {
        let dir = repo_with(&[("src/lib.rs", "pub fn old() {}\n")]);
        write_full_plumb(dir.path());
        scan_index_into_plumb(dir.path());
        // The public surface changes; nobody re-runs the scan.
        write(dir.path(), "src/lib.rs", "pub fn old() {}\npub fn brand_new() {}\n");
        run_git(dir.path(), &["add", "-A"]);
        run_git(dir.path(), &["commit", "-m", "grow the surface"]);

        let outcome = run(dir.path(), &StubRegistry::default(), false).expect("check runs");
        let (output, code) = (outcome.report, outcome.code);

        assert_eq!(code, 2, "output was: {output}");
        assert!(output.contains("index-stale"), "output was: {output}");
        assert!(output.contains("plumb index"), "output was: {output}");
    }

    #[test]
    fn a_matching_scanned_index_does_not_warn() {
        let dir = repo_with(&[("src/lib.rs", "pub fn only() {}\n")]);
        write_full_plumb(dir.path());
        scan_index_into_plumb(dir.path());

        let outcome = run(dir.path(), &StubRegistry::default(), false).expect("check runs");

        assert_eq!(outcome.code, 0, "output was: {}", outcome.report);
    }

    #[test]
    fn a_never_scanned_placeholder_index_is_not_stale() {
        let dir = repo_with(&[("src/lib.rs", "pub fn anything() {}\n")]);
        write_full_plumb(dir.path());
        // write_full_plumb leaves greenfield's placeholder index in place:
        // it carries no SCAN_MARKER, so adoption without a first scan stays
        // silent.

        let outcome = run(dir.path(), &StubRegistry::default(), false).expect("check runs");

        assert_eq!(outcome.code, 0, "output was: {}", outcome.report);
    }

    fn generate_adapters(dir: &Path) {
        use plumb_core::adapters::render;
        use plumb_core::layer1::Artefacts;
        let read =
            |name: &str| fs::read_to_string(dir.join(".plumb").join(name)).expect("read artefact");
        let artefacts = Artefacts {
            architecture: read("architecture.md"),
            conventions: read("conventions.md"),
            invariants: read("invariants.md"),
            index: read("index.md"),
        };
        for adapter in render("demo", &artefacts) {
            let path = dir.join(adapter.path);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("create parent");
            }
            fs::write(path, adapter.content).expect("write adapter");
        }
    }

    #[test]
    fn a_generated_file_that_drifted_warns() {
        let dir = repo_with_requirements("ruff==0.4.0\n");
        write_full_plumb(dir.path());
        generate_adapters(dir.path());
        // Drift: the working agreement changes, nobody regenerates.
        let conventions = dir.path().join(".plumb").join("conventions.md");
        let mut text = fs::read_to_string(&conventions).expect("read");
        text.push_str("\n## New rule\n\nAdded after generation.\n");
        fs::write(&conventions, text).expect("write");

        let outcome = run(dir.path(), &StubRegistry::default(), false).expect("check runs");
        let (output, code) = (outcome.report, outcome.code);

        assert_eq!(code, 2, "output was: {output}");
        assert!(output.contains("adapter-drift"), "output was: {output}");
        assert!(output.contains("plumb adapters"), "output was: {output}");
    }

    #[test]
    fn matching_generated_files_do_not_warn() {
        let dir = repo_with_requirements("ruff==0.4.0\n");
        write_full_plumb(dir.path());
        generate_adapters(dir.path());

        let outcome = run(dir.path(), &StubRegistry::default(), false).expect("check runs");

        assert_eq!(outcome.code, 0, "output was: {}", outcome.report);
    }

    /// Git normalises line endings at checkout (`core.autocrlf` on Windows,
    /// or any repository whose `.gitattributes` declares text files), and
    /// `render` produces LF-only content. A checkout of exactly what was
    /// committed can therefore hold CRLF bytes for content that is, per git,
    /// unchanged and undrifted — the false positive the dogfood run on this
    /// repository actually hit on Windows, warning on all four targets on a
    /// clean tree. The comparison must not mistake a line-ending convention
    /// for a content difference, or this gate warns on every ordinary
    /// Windows clone and gets turned off before it catches anything real.
    #[test]
    fn a_target_that_differs_only_by_line_ending_does_not_warn() {
        let dir = repo_with_requirements("ruff==0.4.0\n");
        write_full_plumb(dir.path());
        generate_adapters(dir.path());
        let claude_md = dir.path().join("CLAUDE.md");
        let content = fs::read_to_string(&claude_md).expect("read");
        fs::write(&claude_md, content.replace('\n', "\r\n")).expect("write CRLF");

        let outcome = run(dir.path(), &StubRegistry::default(), false).expect("check runs");

        assert_eq!(outcome.code, 0, "output was: {}", outcome.report);
    }

    #[test]
    fn an_absent_generated_file_is_not_drift() {
        let dir = repo_with_requirements("ruff==0.4.0\n");
        write_full_plumb(dir.path());
        // No adapters generated at all: adoption is optional, absence is not drift.

        let outcome = run(dir.path(), &StubRegistry::default(), false).expect("check runs");

        assert_eq!(outcome.code, 0, "output was: {}", outcome.report);
    }
}
