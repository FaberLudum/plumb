// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Plumb Authors

//! Invoking git. Every subprocess the gate needs lives here, never in
//! `plumb-core`, which stays free of the outside world.

use anyhow::{Context, Result, bail};
use std::path::Path;
use std::process::Command;

/// Returns `git diff HEAD` for the given repository.
///
/// Output is decoded lossily, so it can contain U+FFFD wherever the diff
/// carried bytes that are not valid UTF-8. See the body for why.
// `allow` and not `expect`: the tests below use this, so the lint fires only
// for the binary target and an expectation would go unfulfilled in the other.
#[allow(dead_code, reason = "wired into the gate in a later task")]
pub fn working_diff(repo: &Path) -> Result<String> {
    let output = Command::new("git")
        .args(["diff", "HEAD"])
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
// See [`working_diff`] for why this is `allow` and not `expect`.
#[allow(dead_code, reason = "wired into the gate in a later task")]
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

        // Assert on the replacement character, not on the path. A diff whose
        // hunk is suppressed — which a global `core.attributesFile` marking
        // `*.txt binary` produces, since `working_diff` honours ambient config
        // while the fixture does not — still contains the path, so a path
        // assertion would pass without the lossy decoding being exercised.
        assert!(diff.contains('\u{FFFD}'), "diff was: {diff}");
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
