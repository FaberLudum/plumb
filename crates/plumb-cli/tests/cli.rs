// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Plumb Authors

//! End-to-end tests for the binary.
//!
//! No test here may reach the network. Every run either passes `--offline` or
//! answers from a seeded cache, so a machine with no route to PyPI and a
//! machine with one produce the same result.

use assert_cmd::Command;
use predicates::prelude::*;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// Runs a git command in `dir` and asserts it succeeded.
///
/// Ambient config is neutralised for the reason `git::tests::run_git` records:
/// a global `commit.gpgsign` with no usable key leaves the fixture with an
/// unborn HEAD, and `git diff HEAD` then fails for a reason that has nothing to
/// do with what is under test.
fn run_git(dir: &Path, args: &[&str]) {
    let no_config = dir.join("no-such-gitconfig");
    let output = std::process::Command::new("git")
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

/// A repository holding one `requirements.txt` at HEAD.
///
/// `* diff` stops a developer's global attributes suppressing the hunk; see
/// `check::tests::repo_with` for why that would make these tests pass by
/// checking nothing.
fn repo_with_requirements(initial: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temp dir");
    run_git(dir.path(), &["init", "-b", "main"]);
    std::fs::write(dir.path().join(".gitattributes"), "* diff\n").expect("write");
    run_git(dir.path(), &["config", "user.email", "test@example.com"]);
    run_git(dir.path(), &["config", "user.name", "Test"]);
    std::fs::write(dir.path().join("requirements.txt"), initial).expect("write");
    run_git(dir.path(), &["add", "-A"]);
    run_git(dir.path(), &["commit", "-m", "initial"]);
    dir
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or(0)
}

/// Seeds the cache the binary reads, so a run can reach a definite answer
/// without a request. `age` is how many seconds ago the entry was checked.
fn seed_cache(dir: &Path, name: &str, exists: bool, age: u64) {
    let cache = dir.join(".plumb").join("cache");
    std::fs::create_dir_all(&cache).expect("create cache dir");
    let checked = now_unix().saturating_sub(age);
    std::fs::write(
        cache.join("registry.json"),
        format!(r#"{{"known":{{"{name}":{{"exists":{exists},"checked":{checked}}}}}}}"#),
    )
    .expect("write cache");
}

fn plumb() -> Command {
    Command::cargo_bin("plumb").expect("binary builds")
}

#[test]
fn reports_its_version() {
    let mut cmd = plumb();
    cmd.arg("--version");
    cmd.assert()
        .success()
        .stdout(predicates::str::contains(format!(
            "plumb {}",
            env!("CARGO_PKG_VERSION")
        )));
}

#[test]
fn check_runs_offline_and_reports_no_findings_on_a_clean_repo() {
    let dir = repo_with_requirements("ruff==0.4.0\n");

    plumb()
        .args(["check", "--diff", "--offline"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicates::str::contains("no findings"));
}

/// The whole binary, from a working diff to exit code 1. The answer comes from
/// a seeded cache entry that is still fresh, so this exercises the blocking
/// path without a request.
#[test]
fn check_exits_one_when_an_added_dependency_does_not_exist() {
    let dir = repo_with_requirements("ruff==0.4.0\n");
    std::fs::write(
        dir.path().join("requirements.txt"),
        "ruff==0.4.0\nrequestz==2.31.0\n",
    )
    .expect("write");
    seed_cache(dir.path(), "requestz", false, 0);

    plumb()
        .args(["check", "--diff", "--offline"])
        .current_dir(dir.path())
        .assert()
        .code(1)
        .stdout(predicates::str::contains("requestz"));
}

/// `git diff` prints paths relative to the repository root, so the gate has to
/// resolve the root rather than trust the working directory. Without that this
/// run reads no manifest, finds nothing, and exits 0 — passing silently, which
/// is the failure mode nobody notices.
#[test]
fn check_works_from_a_subdirectory_of_the_repository() {
    let dir = repo_with_requirements("ruff==0.4.0\n");
    std::fs::write(
        dir.path().join("requirements.txt"),
        "ruff==0.4.0\nrequestz==2.31.0\n",
    )
    .expect("write");
    seed_cache(dir.path(), "requestz", false, 0);
    let nested = dir.path().join("packages").join("api");
    std::fs::create_dir_all(&nested).expect("create dirs");

    plumb()
        .args(["check", "--diff", "--offline"])
        .current_dir(&nested)
        .assert()
        .code(1)
        .stdout(predicates::str::contains("requestz"));
}

/// An offline run answers `Exists` from an entry of unbounded age. That is a
/// deliberate trade, and it is only defensible if the run says it made it.
///
/// The note goes to stderr so that `--json` stays machine-readable, and it
/// must not change the verdict: nothing here is a rule violation.
#[test]
fn an_offline_run_says_when_it_answered_from_stale_cache_data() {
    let dir = repo_with_requirements("ruff==0.4.0\n");
    std::fs::write(
        dir.path().join("requirements.txt"),
        "ruff==0.4.0\nhttpx==0.27.0\n",
    )
    .expect("write");
    // Far past every lifetime in the registry, so it can only be honoured.
    seed_cache(dir.path(), "httpx", true, 400 * 24 * 60 * 60);

    plumb()
        .args(["check", "--diff", "--offline", "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicates::str::contains("[]"))
        .stderr(predicates::str::contains("stale"));
}

/// The mirror of the test above. A run that never honoured an old entry must
/// stay quiet, or the note becomes noise every reader learns to skip.
#[test]
fn a_run_answering_from_fresh_data_says_nothing_about_staleness() {
    let dir = repo_with_requirements("ruff==0.4.0\n");
    std::fs::write(
        dir.path().join("requirements.txt"),
        "ruff==0.4.0\nhttpx==0.27.0\n",
    )
    .expect("write");
    seed_cache(dir.path(), "httpx", true, 0);

    plumb()
        .args(["check", "--diff", "--offline"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stderr(predicates::str::contains("stale").not());
}

/// Both renderings leave stdout ending in exactly one newline. `render_text`
/// supplies its own and `render_json` does not, so the command layer has to
/// normalise them: without one half a shell prompt lands on the closing
/// bracket, without the other every report gains a blank line.
#[test]
fn stdout_ends_with_exactly_one_newline_in_both_renderings() {
    let dir = repo_with_requirements("ruff==0.4.0\n");

    for args in [
        vec!["check", "--diff", "--offline"],
        vec!["check", "--diff", "--offline", "--json"],
    ] {
        let output = plumb()
            .args(&args)
            .current_dir(dir.path())
            .output()
            .expect("the binary runs");
        let stdout = String::from_utf8(output.stdout).expect("utf-8 on stdout");

        assert!(
            stdout.ends_with('\n') && !stdout.ends_with("\n\n"),
            "`plumb {}` wrote {stdout:?}",
            args.join(" ")
        );
    }
}

/// `--diff` is the only scope this version implements, and it is required
/// rather than assumed. A flag that is accepted and ignored means the day a
/// second scope lands, every existing invocation silently changes meaning.
#[test]
fn check_requires_a_scope() {
    let dir = repo_with_requirements("ruff==0.4.0\n");

    plumb()
        .args(["check", "--offline"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicates::str::contains("--diff"));
}

/// Asserts the process exited with a code that is not part of the findings
/// contract, and returns it.
fn assert_did_not_run(args: &[&str], dir: &Path) -> i32 {
    let assert = plumb().args(args).current_dir(dir).assert().failure();
    let code = assert.get_output().status.code().expect("an exit code");
    assert!(
        code != 0 && code != 1 && code != 2,
        "`plumb {}` exited {code}, which reads as a verdict on the code under \
         inspection rather than as a run that never happened",
        args.join(" ")
    );
    code
}

/// Not being in a repository is a failure to run, not a clean run. Exiting 0
/// here would let a misconfigured CI job report a passing gate forever.
#[test]
fn check_fails_outside_a_repository() {
    let dir = tempfile::tempdir().expect("temp dir");

    assert_did_not_run(&["check", "--diff", "--offline"], dir.path());
}

/// A run that never happened must not be spelled like a run that found
/// something. The exit codes are a public contract — 0 clean, 1 blocking, 2
/// warnings only — and clap's own default for a usage error is 2, which tells
/// CI the gate ran and had reservations when it did not run at all.
#[test]
fn a_usage_error_is_never_spelled_like_a_verdict() {
    let dir = repo_with_requirements("ruff==0.4.0\n");

    for args in [
        vec!["check", "--offline"],
        vec!["check", "--diff", "--no-such-flag"],
        vec!["no-such-command"],
        vec![],
    ] {
        assert_did_not_run(&args, dir.path());
    }
}

/// The other side of that: asking for help is not a usage error. Handling
/// clap's errors by hand is what makes this worth pinning — help and version
/// arrive as errors too, and reporting them as failures would break every
/// `plumb --version` in a setup script.
#[test]
fn help_and_version_are_not_errors() {
    for args in [
        vec!["--help"],
        vec!["--version"],
        vec!["check", "--help"],
        vec!["help"],
    ] {
        plumb().args(&args).assert().success();
    }
}
