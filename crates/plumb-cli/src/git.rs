// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Plumb Authors

//! Invoking git. Every subprocess the gate needs lives here, never in
//! `plumb-core`, which stays free of the outside world.

use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Returns the root of the repository containing `from`.
///
/// Every path the gate handles is relative to the repository root: that is
/// what [`working_diff`] prints, and what `git show HEAD:<path>` resolves
/// against. So the root is the only directory those paths may be joined onto.
/// Joining onto the working directory instead makes a run from a subdirectory
/// read nothing, parse nothing and report a clean run over a manifest it never
/// opened.
///
/// A failure here is also the gate's "this is not a repository" error, which is
/// why it reports rather than quietly falling back to `from`.
pub fn repo_root(from: &Path) -> Result<PathBuf> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(from)
        .output()
        .context("failed to run git; is it installed and on PATH?")?;

    if !output.status.success() {
        bail!(
            "not inside a git repository: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    // Strict about UTF-8, like `show_at_head`: this is a path this process is
    // about to read files under, and a lossy decoding would produce one that
    // silently resolves to nothing.
    let root = String::from_utf8(output.stdout).context("git produced non-UTF-8 output")?;
    // Only the line terminator is stripped. A trailing space is legal in a
    // directory name on the platforms git runs on, so `trim_end` would be able
    // to turn a real root into one that does not exist.
    Ok(PathBuf::from(root.trim_end_matches(['\n', '\r'])))
}

/// Returns `git diff HEAD` for the given repository.
///
/// Output is decoded lossily, so it can contain U+FFFD wherever the diff
/// carried bytes that are not valid UTF-8. See the body for why.
///
/// # Why the invocation is this long
///
/// The gate reads paths out of this output. Every documented setting that
/// changes how a path is spelled, or that puts anything in front of the
/// header, therefore switches the gate off — not loudly, but by making it
/// report a clean run over a manifest it never opened. All of the settings
/// below are legal and ordinary; `color.ui = always` is someone who likes
/// coloured diffs, and it was enough on its own.
///
/// - `--no-color` beats `color.ui` and `color.diff`. An escape sequence in
///   front of `diff --git` means no file is recorded at all.
/// - `--no-ext-diff` beats `diff.external` and the `GIT_EXTERNAL_DIFF`
///   environment variable, which replace the whole diff with another tool's
///   output, and beats a per-attribute `diff.<driver>.command`.
/// - `--src-prefix=a/ --dst-prefix=b/` beat `diff.mnemonicPrefix`,
///   `diff.noprefix`, `diff.srcPrefix` and `diff.dstPrefix`. `w/…` and
///   `src/…` both survive the `b/` strip and then resolve to nothing on disk.
/// - `--no-textconv` refuses per-attribute content converters. Their output is
///   never read — only paths are — so converting can only cost time or, when
///   the converter is missing, fail the run outright.
/// - `-c core.quotePath=false` stops a non-ASCII path arriving as
///   `"caf\303\251/requirements.txt"`, escaped and quoted, matching no
///   manifest. Overriding it here rather than teaching the parser to unescape
///   keeps the encoding question with the process that knows the answer.
/// - `-c diff.relative=false` is belt and braces: it limits the diff to the
///   working directory and prints paths relative to it. Callers pass the
///   repository root today, which defuses it, but a caller that stopped doing
///   so would get silence rather than an error.
///
/// Command-line arguments are preferred to `-c` wherever git has one, because
/// they outrank configuration whatever a given git version calls the setting.
///
/// Two things this deliberately does **not** do. It does not pass `--text`,
/// which would defeat a `-diff` attribute suppressing the hunk — measured, one
/// changed 8 MB binary turns a 115-byte diff into 16.8 MB, and the gate reads
/// no hunk content anyway. And `core.quotePath=false` does not reach a path
/// holding a quote, a backslash or a control character: those stay escaped and
/// are still skipped.
pub fn working_diff(repo: &Path) -> Result<String> {
    let output = Command::new("git")
        .args([
            "-c",
            "core.quotePath=false",
            "-c",
            "diff.relative=false",
            "diff",
            "--no-color",
            "--no-ext-diff",
            "--no-textconv",
            "--src-prefix=a/",
            "--dst-prefix=b/",
            "HEAD",
        ])
        .current_dir(repo)
        .output()
        .context("failed to run git; is it installed and on PATH?")?;

    if !output.status.success() {
        bail!(
            "git diff failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    // Lossy on purpose. `git diff` writes hunk bytes verbatim for anything its
    // NUL heuristic calls text, so one Latin-1 or Shift-JIS source file changed
    // anywhere in the repository would otherwise fail the whole gate — for a
    // file the gate would never have looked at, with an error the user cannot
    // act on. Only manifest paths and added lines are read from this string,
    // and replacement characters in an unrelated file change none of them.
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Returns the contents of `path` as committed at HEAD.
///
/// `path` must be relative to the repository root and forward-slash separated,
/// the form `git diff` emits. A backslash-separated path resolves to nothing
/// and so reads as absence.
///
/// `Ok(None)` means the file does not exist at HEAD, which is the normal case
/// for a file added by the change under inspection.
///
/// A failing `git show` is read as absence rather than as an error. That is
/// safe here because callers run [`working_diff`] first, which does surface a
/// missing git, a non-repository, or an unborn HEAD as an error; by this point
/// the only realistic cause of failure is the path genuinely not being there.
///
/// Unlike [`working_diff`] this stays strict about UTF-8: a manifest really
/// should be UTF-8, and here the error names a file the user can fix.
pub fn show_at_head(repo: &Path, path: &str) -> Result<Option<String>> {
    let output = Command::new("git")
        .arg("show")
        .arg(format!("HEAD:{path}"))
        .current_dir(repo)
        .output()
        .context("failed to run git; is it installed and on PATH?")?;

    if !output.status.success() {
        return Ok(None);
    }

    let contents = String::from_utf8(output.stdout).context("git produced non-UTF-8 output")?;
    Ok(Some(contents))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;

    /// Runs a git command in `dir` and asserts it succeeded.
    ///
    /// Asserting matters more than it looks. `.output().expect(..)` only checks
    /// that git *spawned*; a command that runs and exits 128 is accepted
    /// silently. A global `commit.gpgsign` with no usable key, or a global
    /// `core.hooksPath`, then leaves the fixture with an unborn HEAD — and
    /// `show_at_head_returns_none_for_a_new_file` passes against that, because
    /// `git show HEAD:…` exits 128 and is mapped to `Ok(None)`. The one test
    /// pinning the absence contract is the one that survives a fixture that
    /// never ran.
    ///
    /// Ambient config is neutralised for the same reason: point
    /// `GIT_CONFIG_GLOBAL` and `GIT_CONFIG_SYSTEM` at a path that does not
    /// exist so the fixture cannot inherit this machine's settings.
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

    fn init_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("temp dir");
        run_git(dir.path(), &["init", "-b", "main"]);
        // No `* diff` attribute here, deliberately. It used to be needed:
        // hunk suppression cost the old parser the `+++` line and with it the
        // path, so the fixture had to forbid it, which meant no test could
        // observe the failure it was hiding. `changed_paths` does not read
        // hunks, so the fixture can now run against the same git a developer
        // has — and `changed_paths_survives_hunk_suppression` proves it.
        run_git(dir.path(), &["config", "user.email", "test@example.com"]);
        run_git(dir.path(), &["config", "user.name", "Test"]);
        fs::write(dir.path().join("requirements.txt"), "ruff==0.4.0\n").expect("write");
        run_git(dir.path(), &["add", "-A"]);
        run_git(dir.path(), &["commit", "-m", "initial"]);
        dir
    }

    #[test]
    fn changed_paths_reports_an_uncommitted_change() {
        let dir = init_repo();
        fs::write(
            dir.path().join("requirements.txt"),
            "ruff==0.4.0\nrequestz==2.31.0\n",
        )
        .expect("write");

        let paths = changed_paths(dir.path()).expect("git succeeds");

        assert_eq!(paths, vec!["requirements.txt"]);
    }

    /// Pins the `HEAD` in `git diff HEAD`. A plain `git diff` reports nothing
    /// once the change is staged, so without this the other tests would pass
    /// against an implementation that silently misses a pre-commit hook's view.
    #[test]
    fn changed_paths_reports_a_staged_change() {
        let dir = init_repo();
        fs::write(
            dir.path().join("requirements.txt"),
            "ruff==0.4.0\nrequestz==2.31.0\n",
        )
        .expect("write");
        run_git(dir.path(), &["add", "-A"]);

        let paths = changed_paths(dir.path()).expect("git succeeds");

        assert_eq!(paths, vec!["requirements.txt"]);
    }

    #[test]
    fn changed_paths_is_empty_for_a_clean_tree() {
        let dir = init_repo();

        assert!(changed_paths(dir.path()).expect("git succeeds").is_empty());
    }

    /// The whole reason this asks git for names instead of a diff.
    ///
    /// A global attributes file marking everything `-diff` makes git replace
    /// every hunk with `Binary files a/… and b/… differ`, which carries no
    /// `+++` line — so a parser reading the diff loses the path and the gate
    /// reports a clean run over a manifest it never opened. It needs no
    /// cooperation from the repository: `core.attributesFile` is an ordinary
    /// dotfile setting, and `$XDG_CONFIG_HOME/git/attributes` is read with no
    /// configuration at all.
    ///
    /// `core.bigFileThreshold` reaches the same suppression by a different
    /// road, without attributes, for any manifest larger than the threshold.
    /// Both are here because they are one class, and the class is what a name
    /// listing closes: there is no hunk to suppress.
    #[test]
    fn changed_paths_survives_hunk_suppression() {
        for (key, value, attributes) in [
            ("core.attributesFile", Some("* -diff"), true),
            ("core.bigFileThreshold", None, false),
        ] {
            let dir = init_repo();
            if attributes {
                let attrs = dir.path().join("global-attributes");
                fs::write(&attrs, format!("{}\n", value.unwrap_or_default())).expect("write");
                // Forward slashes: a backslash is an escape character inside a
                // git config value.
                let spelled = attrs.display().to_string().replace('\\', "/");
                run_git(dir.path(), &["config", key, &spelled]);
            } else {
                run_git(dir.path(), &["config", key, "1"]);
            }
            fs::write(
                dir.path().join("requirements.txt"),
                "ruff==0.4.0\nrequestz==2.31.0\n",
            )
            .expect("write");

            let paths = changed_paths(dir.path()).expect("git succeeds");

            assert_eq!(paths, vec!["requirements.txt"], "defeated by {key}");
        }
    }

    /// A file whose *content* is not UTF-8 must not fail the gate. Under the
    /// old diff-reading invocation this was a live hazard, because the hunk
    /// bytes came back verbatim and one Latin-1 file anywhere in the repository
    /// put invalid UTF-8 into the output. Asking for names removes the hazard
    /// at the source: content never reaches this process.
    ///
    /// Kept because the guarantee still has to hold, and now it holds for a
    /// better reason than lossy decoding.
    #[test]
    fn changed_paths_is_indifferent_to_non_utf8_file_content() {
        let dir = init_repo();
        let latin1 = dir.path().join("latin1.txt");
        fs::write(&latin1, [0xE9, b'\n']).expect("write");
        run_git(dir.path(), &["add", "-A"]);
        run_git(dir.path(), &["commit", "-m", "add a latin-1 file"]);
        fs::write(&latin1, [0xE9, 0xE9, b'\n']).expect("write");

        let paths = changed_paths(dir.path()).expect("git succeeds despite non-UTF-8 content");

        assert_eq!(paths, vec!["latin1.txt"]);
    }

    /// Deletions, renames and mode-only changes all reach the caller.
    ///
    /// The old parser dropped every one of them: a deletion writes
    /// `+++ /dev/null`, and a pure rename or a mode change writes no `+++`
    /// line at all. None of them can add a dependency, so no verdict changes
    /// — but a pure rename of a manifest was invisible, and `check` treats a
    /// path with no version at HEAD as a new manifest, so the renamed one is
    /// now checked exactly as a rename *with* an edit already was.
    #[test]
    fn changed_paths_reports_deletions_renames_and_mode_changes() {
        let dir = init_repo();
        fs::write(dir.path().join("gone.txt"), "x\n").expect("write");
        fs::write(dir.path().join("old-name.txt"), "content\n").expect("write");
        fs::write(dir.path().join("mode.txt"), "y\n").expect("write");
        run_git(dir.path(), &["add", "-A"]);
        run_git(dir.path(), &["commit", "-m", "more files"]);

        fs::remove_file(dir.path().join("gone.txt")).expect("remove");
        run_git(dir.path(), &["mv", "old-name.txt", "new-name.txt"]);
        run_git(dir.path(), &["update-index", "--chmod=+x", "mode.txt"]);

        let paths = changed_paths(dir.path()).expect("git succeeds");

        for expected in ["gone.txt", "new-name.txt", "mode.txt"] {
            assert!(paths.iter().any(|p| p == expected), "{paths:?}");
        }
    }

    /// Every path the gate handles is relative to the repository root: that is
    /// what `git diff` prints and what `git show HEAD:…` resolves against. So
    /// the root is what the gate must join those paths onto. Running from a
    /// subdirectory and joining onto the working directory instead reads
    /// nothing, parses nothing, and reports a clean run over a manifest it
    /// never opened — a silent no-op, which is the worst thing a gate can be.
    ///
    /// Compares by content rather than by string. A temporary directory can be
    /// reached by more than one spelling — a short path on Windows, a symlink
    /// on macOS — and git answers with its own, so `assert_eq!` on the paths
    /// would fail for reasons that have nothing to do with the behaviour.
    #[test]
    fn repo_root_is_found_from_a_subdirectory() {
        let dir = init_repo();
        let nested = dir.path().join("packages").join("api");
        fs::create_dir_all(&nested).expect("create dirs");

        let root = repo_root(&nested).expect("the root is found");

        // The subdirectory holds no manifest, so this alone rules out the
        // input being handed back unchanged.
        assert!(
            root.join("requirements.txt").is_file(),
            "{} is not the repository root",
            root.display()
        );
        assert!(
            root.join("packages").join("api").is_dir(),
            "{} is not an ancestor of the directory asked about",
            root.display()
        );
    }

    /// The one error the gate can give a useful message for. Every other
    /// failure here is git being absent, which the context already names.
    #[test]
    fn repo_root_fails_outside_a_repository() {
        let dir = tempfile::tempdir().expect("temp dir");

        assert!(repo_root(dir.path()).is_err());
    }

    /// git escapes a non-ASCII path into C-style octal by default, so
    /// `café/requirements.txt` arrives as `"caf\303\251/requirements.txt"`,
    /// quotes and all. Nothing downstream unquotes that: the path matches no
    /// manifest, and the gate reports a clean run over a file it never opened.
    ///
    /// Asking git not to escape it is cheaper than teaching the parser to
    /// unescape it, and it is the only place that knows the encoding for
    /// certain. It does not cover a path containing a quote, a backslash or a
    /// control character — those stay escaped whatever this setting says.
    #[test]
    fn changed_paths_reports_awkward_paths_verbatim() {
        let dir = init_repo();
        // A non-ASCII directory and one with a space: git escapes the first
        // into C-style octal and wraps it in quotes, and terminates the second
        // with a tab, in every output format except the NUL-separated one.
        for directory in ["café", "my project"] {
            let nested = dir.path().join(directory);
            fs::create_dir_all(&nested).expect("create dir");
            fs::write(nested.join("requirements.txt"), "ruff==0.4.0\n").expect("write");
        }
        run_git(dir.path(), &["add", "-A"]);
        run_git(dir.path(), &["commit", "-m", "add awkward paths"]);
        // `core.quotePath` is left at its default on purpose: `-z` is what
        // makes it irrelevant, and a test that turned it off would pass
        // against an implementation relying on the override instead.
        for directory in ["café", "my project"] {
            fs::write(
                dir.path().join(directory).join("requirements.txt"),
                "ruff==0.4.0\nrequestz==2.31.0\n",
            )
            .expect("write");
        }

        let paths = changed_paths(dir.path()).expect("git succeeds");

        assert!(
            paths.iter().any(|p| p == "café/requirements.txt"),
            "{paths:?}"
        );
        assert!(
            paths.iter().any(|p| p == "my project/requirements.txt"),
            "{paths:?}"
        );
    }

    /// Sets every configuration found to change the shape of `git diff`
    /// output, in the repository's own config.
    ///
    /// Local rather than global because the process environment is shared with
    /// every other test in this binary. `tests/cli.rs` sets the same settings
    /// through `GIT_CONFIG_GLOBAL`, which is how they reach a developer, and
    /// is the spelling that matters: none of this needs a repository's
    /// cooperation.
    fn make_git_hostile(dir: &Path) {
        for (key, value) in [
            ("color.ui", "always"),
            ("color.diff", "always"),
            ("diff.mnemonicPrefix", "true"),
            ("diff.noprefix", "true"),
            ("diff.srcPrefix", "src/"),
            ("diff.dstPrefix", "dst/"),
            ("diff.relative", "true"),
            ("diff.external", "/nonexistent/external-diff"),
            ("diff.colorMoved", "zebra"),
            ("diff.wsErrorHighlight", "all"),
            ("core.quotePath", "true"),
            // A per-attribute content converter, reached through the
            // `diff=hostile` attribute written below. Its command does not
            // exist, which is what a converter for a format this machine has
            // no tool for looks like; it used to fail the whole gate.
            ("diff.hostile.textconv", "/nonexistent/textconv"),
        ] {
            run_git(dir, &["config", key, value]);
        }
        fs::write(dir.join(".gitattributes"), "* diff=hostile\n").expect("write");
    }

    /// The gate reads paths out of this output, so anything that changes how a
    /// path is spelled silently unchecks the file. Every setting above does
    /// exactly that, and all of them are legal and ordinary.
    ///
    /// The assertion is on the exact header rather than on the path appearing
    /// somewhere: `src/requirements.txt` and `w/requirements.txt` both contain
    /// the name, and both resolve to nothing on disk.
    #[test]
    fn changed_paths_ignores_configuration_that_reshapes_diff_output() {
        let dir = init_repo();
        make_git_hostile(dir.path());
        fs::write(
            dir.path().join("requirements.txt"),
            "ruff==0.4.0\nrequestz==2.31.0\n",
        )
        .expect("write");

        let paths = changed_paths(dir.path()).expect("git succeeds");

        // Exact, not "contains the name". Every one of these settings used to
        // produce a path that still held the word `requirements.txt` — `w/…`,
        // `src/…`, a quoted form — and every one of them resolved to nothing
        // on disk.
        assert_eq!(paths, vec!["requirements.txt"]);
    }

    /// Paths come back relative to the repository root whichever directory
    /// inside the repository this is given.
    ///
    /// `diff.relative` breaks that: it limits the diff to the working
    /// directory and prints what is left relative to it. Callers pass the root
    /// today, which defuses it — so this calls it with a subdirectory on
    /// purpose, because the contract is what the next caller will rely on and
    /// the failure would be silence rather than an error.
    #[test]
    fn changed_paths_are_root_relative_from_any_directory() {
        let dir = init_repo();
        make_git_hostile(dir.path());
        let nested = dir.path().join("packages").join("api");
        fs::create_dir_all(&nested).expect("create dirs");
        fs::write(
            dir.path().join("requirements.txt"),
            "ruff==0.4.0\nrequestz==2.31.0\n",
        )
        .expect("write");

        let paths = changed_paths(&nested).expect("git succeeds");

        assert_eq!(
            paths,
            vec!["requirements.txt"],
            "a manifest outside the directory asked about went missing"
        );
    }

    /// Checked rather than assumed. `git show <rev>:<path>` writes the blob
    /// out as stored — no colour, no textconv, no line-ending conversion, and
    /// no path for `core.quotePath` to escape — so it needs none of the
    /// hardening [`working_diff`] does. If that ever stops being true this
    /// fails, which is the point of pinning it.
    #[test]
    fn show_at_head_is_unaffected_by_the_same_configuration() {
        let dir = init_repo();
        make_git_hostile(dir.path());
        run_git(dir.path(), &["config", "core.autocrlf", "true"]);
        run_git(
            dir.path(),
            &["config", "diff.weird.textconv", "/nonexistent"],
        );
        fs::write(dir.path().join(".gitattributes"), "* diff=weird\n").expect("write");
        fs::write(dir.path().join("requirements.txt"), "changed\n").expect("write");

        let head = show_at_head(dir.path(), "requirements.txt").expect("show succeeds");

        assert_eq!(head.as_deref(), Some("ruff==0.4.0\n"));
    }

    #[test]
    fn show_at_head_returns_the_committed_contents() {
        let dir = init_repo();
        fs::write(
            dir.path().join("requirements.txt"),
            "ruff==0.4.0\nrequestz==2.31.0\n",
        )
        .expect("write");

        let head = show_at_head(dir.path(), "requirements.txt").expect("show succeeds");

        assert_eq!(head.as_deref(), Some("ruff==0.4.0\n"));
    }

    #[test]
    fn show_at_head_returns_none_for_a_new_file() {
        let dir = init_repo();
        fs::write(dir.path().join("pyproject.toml"), "[project]\n").expect("write");

        let head = show_at_head(dir.path(), "pyproject.toml").expect("show succeeds");

        assert_eq!(head, None);
    }
}
