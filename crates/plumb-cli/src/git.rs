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
        // Per-directory attributes outrank the global attributes file and the
        // XDG default at `~/.config/git/attributes`. Without this, a developer
        // whose global attributes mark `*.txt` as `binary` or `-diff` gets a
        // suppressed hunk from `working_diff` — which honours ambient config,
        // as production does — while the fixture neutralises that config for
        // its own commands. Pinning it here removes the one variable the tests
        // cannot otherwise control.
        fs::write(dir.path().join(".gitattributes"), "* diff\n").expect("write");
        run_git(dir.path(), &["config", "user.email", "test@example.com"]);
        run_git(dir.path(), &["config", "user.name", "Test"]);
        fs::write(dir.path().join("requirements.txt"), "ruff==0.4.0\n").expect("write");
        run_git(dir.path(), &["add", "-A"]);
        run_git(dir.path(), &["commit", "-m", "initial"]);
        dir
    }

    #[test]
    fn working_diff_reports_uncommitted_changes() {
        let dir = init_repo();
        fs::write(
            dir.path().join("requirements.txt"),
            "ruff==0.4.0\nrequestz==2.31.0\n",
        )
        .expect("write");

        let diff = working_diff(dir.path()).expect("diff succeeds");

        assert!(diff.contains("+requestz==2.31.0"), "diff was: {diff}");
    }

    /// Pins the `HEAD` in `git diff HEAD`. A plain `git diff` reports nothing
    /// once the change is staged, so without this the other tests would pass
    /// against an implementation that silently misses a pre-commit hook's view.
    #[test]
    fn working_diff_reports_staged_changes() {
        let dir = init_repo();
        fs::write(
            dir.path().join("requirements.txt"),
            "ruff==0.4.0\nrequestz==2.31.0\n",
        )
        .expect("write");
        run_git(dir.path(), &["add", "-A"]);

        let diff = working_diff(dir.path()).expect("diff succeeds");

        assert!(diff.contains("+requestz==2.31.0"), "diff was: {diff}");
    }

    /// `git diff` writes hunk bytes verbatim for anything its NUL heuristic
    /// calls text, so one Latin-1 or Shift-JIS file changed anywhere in the
    /// repository must not fail the whole gate. A lone `0xE9` is valid Latin-1,
    /// invalid UTF-8, and carries no NUL, so git emits it raw rather than
    /// suppressing the hunk as binary.
    #[test]
    fn working_diff_tolerates_non_utf8_content() {
        let dir = init_repo();
        let latin1 = dir.path().join("latin1.txt");
        fs::write(&latin1, [0xE9, b'\n']).expect("write");
        run_git(dir.path(), &["add", "-A"]);
        run_git(dir.path(), &["commit", "-m", "add a latin-1 file"]);
        fs::write(&latin1, [0xE9, 0xE9, b'\n']).expect("write");

        let diff = working_diff(dir.path()).expect("diff succeeds despite non-UTF-8 content");

        // Assert on the replacement character, not on the path: a suppressed
        // hunk still carries the path, so a path assertion would pass without
        // the lossy decoding ever being exercised. Suppression needs no
        // configuration key at all — `*.txt -diff` in the XDG default
        // `~/.config/git/attributes` is enough, and `-diff` alone does it
        // without the full `binary` macro — which is why `init_repo` pins
        // `* diff` inside the repository, where it outranks both.
        assert!(diff.contains('\u{FFFD}'), "diff was: {diff}");
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
    fn working_diff_does_not_escape_a_non_ascii_path() {
        let dir = init_repo();
        let nested = dir.path().join("café");
        fs::create_dir_all(&nested).expect("create dir");
        fs::write(nested.join("requirements.txt"), "ruff==0.4.0\n").expect("write");
        run_git(dir.path(), &["add", "-A"]);
        run_git(dir.path(), &["commit", "-m", "add a non-ascii path"]);
        fs::write(
            nested.join("requirements.txt"),
            "ruff==0.4.0\nrequestz==2.31.0\n",
        )
        .expect("write");

        let diff = working_diff(dir.path()).expect("diff succeeds");

        assert!(
            diff.contains("+++ b/café/requirements.txt"),
            "diff was: {diff}"
        );
    }

    /// Sets every configuration found to change the shape of `git diff`
    /// output, in the repository's own config.
    ///
    /// Local rather than global because the process environment is shared with
    /// every other test in this binary; command-line arguments outrank both, so
    /// this exercises the same defence. `tests/cli.rs` sets the same settings
    /// through `GIT_CONFIG_GLOBAL`, which is how they reach a developer.
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
            // no tool for looks like; without `--no-textconv` git fails and
            // takes the whole gate with it.
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
    fn working_diff_ignores_configuration_that_reshapes_its_output() {
        let dir = init_repo();
        make_git_hostile(dir.path());
        fs::write(
            dir.path().join("requirements.txt"),
            "ruff==0.4.0\nrequestz==2.31.0\n",
        )
        .expect("write");

        let diff = working_diff(dir.path()).expect("diff succeeds");

        assert!(
            diff.contains("+++ b/requirements.txt\n"),
            "diff was: {diff:?}"
        );
        // The pre-image line too. Nothing reads it today, so without this the
        // `--src-prefix` half of the defence is unpinned and would be dropped
        // by anyone tidying the invocation — and then the day something does
        // read it, it is `w/…` or `src/…` again.
        assert!(
            diff.contains("--- a/requirements.txt\n"),
            "diff was: {diff:?}"
        );
        assert!(diff.contains("+requestz==2.31.0"), "diff was: {diff:?}");
        // Colour is the one that leaves no path at all: an escape sequence in
        // front of `diff --git` stops the header being recognised.
        assert!(!diff.contains('\u{1b}'), "diff carried colour: {diff:?}");
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
    fn working_diff_reports_root_relative_paths_from_any_directory() {
        let dir = init_repo();
        make_git_hostile(dir.path());
        let nested = dir.path().join("packages").join("api");
        fs::create_dir_all(&nested).expect("create dirs");
        fs::write(
            dir.path().join("requirements.txt"),
            "ruff==0.4.0\nrequestz==2.31.0\n",
        )
        .expect("write");

        let diff = working_diff(&nested).expect("diff succeeds");

        assert!(
            diff.contains("+++ b/requirements.txt\n"),
            "a manifest outside the directory asked about went missing: {diff:?}"
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
