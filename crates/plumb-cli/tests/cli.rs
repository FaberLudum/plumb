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

/// A tempdir holding a repository at `repo/` and this test's cache beside it.
///
/// The repository is a subdirectory rather than the tempdir root so that
/// [`cache_file`] can sit next to it instead of inside it. A cache inside a
/// repository is precisely what
/// `a_cache_file_inside_the_repository_is_never_consulted` forbids, and a
/// fixture modelling the forbidden shape is one somebody copies.
///
/// No `* diff` attribute. It used to be needed to stop a developer's global
/// attributes suppressing the hunk — which also meant no test could observe
/// that failure, because the fixture forbade it. `HOSTILE_GIT_CONFIG` now
/// provokes it instead, by the two global routes that beat an in-repository
/// attribute anyway.
fn repo_with_requirements(initial: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temp dir");
    let repo = repo(dir.path());
    std::fs::create_dir_all(&repo).expect("create repo dir");
    run_git(&repo, &["init", "-b", "main"]);
    run_git(&repo, &["config", "user.email", "test@example.com"]);
    run_git(&repo, &["config", "user.name", "Test"]);
    std::fs::write(repo.join("requirements.txt"), initial).expect("write");
    run_git(&repo, &["add", "-A"]);
    run_git(&repo, &["commit", "-m", "initial"]);
    dir
}

/// The repository inside a fixture tempdir.
fn repo(dir: &Path) -> std::path::PathBuf {
    dir.join("repo")
}

/// A path inside the repository of a fixture tempdir.
fn in_repo(dir: &Path, name: &str) -> std::path::PathBuf {
    repo(dir).join(name)
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or(0)
}

/// The cache file a test points the binary at.
///
/// Outside the repository, and outside the developer's own cache directory.
/// Both halves matter. The binary now resolves its cache under
/// `%LOCALAPPDATA%` or `~/.cache`, so a test that said nothing would read and
/// write the cache of whoever is running the suite — seeding one machine's
/// answers into another test's run, and leaving entries behind afterwards.
/// Naming the file explicitly makes each test's cache exactly what that test
/// wrote and nothing else, which is also what keeps every run off the network.
fn cache_file(dir: &Path) -> std::path::PathBuf {
    dir.join("plumb-cache.json")
}

/// Seeds the cache the binary reads, so a run can reach a definite answer
/// without a request. `age` is how many seconds ago the entry was checked.
///
/// `dir` is the fixture tempdir, so the file lands beside the checkout rather
/// than inside it — see [`repo_with_requirements`].
fn seed_cache(dir: &Path, name: &str, exists: bool, age: u64) {
    let checked = now_unix().saturating_sub(age);
    write_cache(
        dir,
        &format!(r#""{name}":{{"exists":{exists},"checked":{checked}}}"#),
    );
}

/// Writes a whole cache body from pre-rendered entries.
fn write_cache(dir: &Path, entries: &str) {
    std::fs::write(cache_file(dir), format!(r#"{{"known":{{{entries}}}}}"#)).expect("write cache");
}

/// The binary, run inside a fixture's repository and pointed at that
/// fixture's cache.
fn plumb_in(dir: &Path) -> Command {
    let mut command = Command::cargo_bin("plumb").expect("binary builds");
    command
        .current_dir(repo(dir))
        .env(CACHE_FILE_ENV, cache_file(dir));
    command
}

/// The binary with no repository and no cache of its own — for `--help` and
/// `--version`, which read neither.
fn plumb() -> Command {
    Command::cargo_bin("plumb").expect("binary builds")
}

/// Kept in step with `registry::CACHE_FILE_ENV` by
/// `the_environment_variable_the_tests_use_is_the_one_the_binary_reads`.
const CACHE_FILE_ENV: &str = "PLUMB_CACHE_FILE";

/// Every setting found to change the shape of `git diff` output, written as a
/// global config the way it reaches a real developer.
///
/// All of these are legal, documented and the sort of thing people put in
/// dotfiles. None is an attack; `color.ui = always` is just someone who likes
/// coloured diffs, and `core.attributesFile` is where a developer keeps the
/// attributes they want in every repository.
///
/// Two of them do more than reshape the output: `core.bigFileThreshold` and an
/// attributes file marking paths `-diff` make git replace the hunk with
/// `Binary files a/… and b/… differ`, which carries no `+++` line at all.
/// Those are the ones no flag could ever have reached, and they belong here,
/// at the end-to-end level, in their global spelling — the whole point of the
/// finding was that **none of this needs the repository's cooperation**.
///
/// `{attributes}` is substituted with a real path by [`hostile_git_config`].
const HOSTILE_GIT_CONFIG: &str = "\
[color]
\tui = always
\tdiff = always
[diff]
\tmnemonicPrefix = true
\tnoprefix = true
\tsrcPrefix = src/
\tdstPrefix = dst/
\trelative = true
\texternal = /nonexistent/external-diff
\tcolorMoved = zebra
\twsErrorHighlight = all
[core]
\tquotePath = true
\tbigFileThreshold = 1
\tattributesFile = {attributes}
";

/// Writes [`HOSTILE_GIT_CONFIG`] and returns the path to give `GIT_CONFIG_GLOBAL`.
fn hostile_git_config(dir: &Path) -> std::path::PathBuf {
    let attributes = dir.join("hostile-gitattributes");
    std::fs::write(&attributes, "* -diff\n").expect("write attributes");

    // Forward slashes: a backslash is an escape character inside a git config
    // value, so a Windows path spelled with them does not resolve — and a
    // hostile setting that quietly fails to apply is a test proving nothing.
    let spelled = attributes.display().to_string().replace('\\', "/");
    let path = dir.join("hostile-gitconfig");
    std::fs::write(&path, HOSTILE_GIT_CONFIG.replace("{attributes}", &spelled))
        .expect("write config");
    path
}

/// Every test here isolates its cache by setting [`CACHE_FILE_ENV`], so if the
/// binary ever read a different variable the isolation would vanish silently:
/// runs would fall back to the developer's own cache directory, answer from
/// whatever happened to be in it, and — with no entry — reach the network,
/// which no test in this file may do. Nothing would fail; results would just
/// start depending on the machine.
///
/// An integration test cannot import from a binary crate, so this reads the
/// declaration out of the source, the way
/// `every_finding_names_a_section_of_the_taxonomy` reads the taxonomy.
#[test]
fn the_environment_variable_the_tests_use_is_the_one_the_binary_reads() {
    let source = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("registry.rs"),
    )
    .expect("the registry source is readable");

    assert!(
        source.contains(&format!(
            r#"pub const CACHE_FILE_ENV: &str = "{CACHE_FILE_ENV}";"#
        )),
        "the binary no longer reads {CACHE_FILE_ENV}, so these tests isolate nothing"
    );
}

/// A cache file sitting inside the repository must never answer for the gate.
///
/// The rule this tool enforces is cited to an attack paper, and the cache is
/// an unauthenticated trust store: `PyPiRegistry::new` reads it with no
/// provenance check at all, and a fresh entry short-circuits the network
/// before any lookup happens. Keeping that store inside the repository put it
/// in the writable, committable area of whoever is proposing the change — so
/// one committed file turned the blocking gate off, for `EXISTS_TTL`, which is
/// thirty days.
///
/// Reproduced before this test existed, on a repository whose HEAD carries
/// `.plumb/cache/registry.json` asserting a nonexistent package exists:
/// `plumb: no findings`, exit 0. Deleting that one file: exit 1.
///
/// `.gitignore` already listed `.plumb/cache/`, so the authors expected it
/// untracked — but nothing enforced it, and an expectation that nothing
/// enforces is not a defence. The two caches here disagree on purpose, so the
/// verdict says which one was read rather than merely that a verdict was
/// reached.
#[test]
fn a_cache_file_inside_the_repository_is_never_consulted() {
    let dir = repo_with_requirements("ruff==0.4.0\n");

    // The poison, committed on its own so that HEAD still carries the
    // manifest without `requestz` — otherwise the change under inspection
    // adds nothing and the run would report nothing for an unrelated reason.
    let planted = in_repo(dir.path(), ".plumb").join("cache");
    std::fs::create_dir_all(&planted).expect("create cache dir");
    std::fs::write(
        planted.join("registry.json"),
        format!(
            r#"{{"known":{{"requestz":{{"exists":true,"checked":{}}}}}}}"#,
            now_unix()
        ),
    )
    .expect("write cache");
    run_git(&repo(dir.path()), &["add", "-A", "-f"]);
    run_git(&repo(dir.path()), &["commit", "-m", "commit a cache file"]);
    std::fs::write(
        in_repo(dir.path(), "requirements.txt"),
        "ruff==0.4.0\nrequestz==2.31.0\n",
    )
    .expect("write");

    // The real cache, outside the repository, which knows better.
    seed_cache(dir.path(), "requestz", false, 0);

    plumb_in(dir.path())
        .args(["check", "--diff", "--offline"])
        .assert()
        .code(1)
        .stdout(predicates::str::contains("requestz"));
}

/// The leftover file is not read, but it is not passed over in silence
/// either: every existing checkout has one, and a file that looks like it is
/// doing something and is not is the shape of defect this project is about.
///
/// Migrating it was the other option and was rejected — it would import
/// exactly the unverifiable entries the move exists to refuse. So the run says
/// the file is dead and says to delete it.
///
/// On stderr, and the verdict is unchanged: nothing here is a rule violation.
#[test]
fn a_leftover_cache_inside_the_repository_is_reported_not_read() {
    let dir = repo_with_requirements("ruff==0.4.0\n");
    let planted = in_repo(dir.path(), ".plumb").join("cache");
    std::fs::create_dir_all(&planted).expect("create cache dir");
    std::fs::write(planted.join("registry.json"), r#"{"known":{}}"#).expect("write cache");

    plumb_in(dir.path())
        .args(["check", "--diff", "--offline"])
        .assert()
        .success()
        .stderr(
            predicates::str::contains("no longer read").and(predicates::str::contains(".plumb")),
        );
}

/// One byte that is not UTF-8 must not silently uncheck a whole manifest.
///
/// `read_to_string` fails on invalid UTF-8, and the result was
/// `.unwrap_or_default()` — so the after-side read as empty, no name was
/// looked up, and the run exited 0. Verified on a one-byte difference:
/// `# cafe pinned` blocks, `# caf\xe9 pinned` reports nothing. A Latin-1
/// comment in a `requirements.txt` is ordinary, and the package below it is
/// not checked at all.
///
/// A file this process could not decode is not evidence that a package does
/// not exist, so there is no Tier A violation — but it is a coverage finding,
/// Tier B, in the report on stdout. Exit 2 rather than 0: a run that could
/// not establish anything about a manifest must not hand back the code that
/// means "checked and clean".
#[test]
fn a_manifest_that_is_not_utf8_is_reported_rather_than_skipped() {
    let dir = repo_with_requirements("ruff==0.4.0\n");
    // `\xe9` is `é` in Latin-1 and is not valid UTF-8 on its own.
    std::fs::write(
        in_repo(dir.path(), "requirements.txt"),
        b"# caf\xe9 pinned\nruff==0.4.0\nrequestz==2.31.0\n",
    )
    .expect("write");
    seed_cache(dir.path(), "requestz", false, 0);

    plumb_in(dir.path())
        .args(["check", "--diff", "--offline"])
        .assert()
        .code(2)
        .stdout(
            predicates::str::contains("requirements.txt").and(predicates::str::contains("UTF-8")),
        );
}

/// The binary must not claim a file is unread while it is reading it.
///
/// `PLUMB_CACHE_FILE` can be pointed at the old in-repository path, and the
/// first version of the note printed "no longer read and can be deleted"
/// unconditionally — so the run reassured the user about the exact file it was
/// answering from. A gate that makes a false statement about its own trust
/// store is this project's own defect class.
///
/// Both halves are asserted. The false reassurance must be gone, and the
/// truthful warning — that the cache in use is inside the repository — must be
/// there, because dropping the sentence and saying nothing would also pass a
/// test that only checked for absence.
#[test]
fn a_cache_in_use_inside_the_repository_is_not_called_unread() {
    let dir = repo_with_requirements("ruff==0.4.0\n");
    let planted = in_repo(dir.path(), ".plumb").join("cache");
    std::fs::create_dir_all(&planted).expect("create cache dir");
    std::fs::write(planted.join("registry.json"), r#"{"known":{}}"#).expect("write cache");

    plumb_in(dir.path())
        .args(["check", "--diff", "--offline"])
        .env(CACHE_FILE_ENV, planted.join("registry.json"))
        .assert()
        .success()
        .stderr(
            predicates::str::contains("no longer read")
                .not()
                .and(predicates::str::contains("inside the repository")),
        );
}

/// The mirror: no leftover file, no note. A warning every run prints is one
/// every reader learns to skip.
#[test]
fn a_repository_without_the_old_cache_says_nothing_about_it() {
    let dir = repo_with_requirements("ruff==0.4.0\n");

    plumb_in(dir.path())
        .args(["check", "--diff", "--offline"])
        .assert()
        .success()
        .stderr(predicates::str::contains("no longer read").not());
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

    plumb_in(dir.path())
        .args(["check", "--diff", "--offline"])
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
        in_repo(dir.path(), "requirements.txt"),
        "ruff==0.4.0\nrequestz==2.31.0\n",
    )
    .expect("write");
    seed_cache(dir.path(), "requestz", false, 0);

    plumb_in(dir.path())
        .args(["check", "--diff", "--offline"])
        .assert()
        .code(1)
        .stdout(predicates::str::contains("requestz"));
}

/// The whole binary, from a working diff carrying an undeclared cross-module
/// import to exit code 2. `--offline` is exercised for the same reason as the
/// package-existence test above: this rule never touches a registry, so the
/// flag is here to prove that, not because the finding needs it.
///
/// The source file is committed *without* the import: the boundary gate
/// reports only violations a change introduces, so a violation already
/// present at HEAD — which committing it alongside the architecture in one
/// commit would have produced — would be legacy and suppressed rather than
/// reported. Introducing it in the working-tree edit is what makes this the
/// change that adds it.
#[test]
fn an_undeclared_import_warns_end_to_end() {
    let dir = repo_with_requirements("ruff==0.4.0\n");
    let plumb_dir = in_repo(dir.path(), ".plumb");
    std::fs::create_dir_all(&plumb_dir).expect("create .plumb");
    std::fs::write(
        plumb_dir.join("architecture.md"),
        "## crates/core\n\nMay depend on: (none).\n\n## crates/cli\n\nMay depend on: core.\n",
    )
    .expect("write architecture");
    let src = in_repo(dir.path(), "crates/core/src");
    std::fs::create_dir_all(&src).expect("create src");
    std::fs::write(src.join("lib.rs"), "// empty\n").expect("write source");
    run_git(&repo(dir.path()), &["add", "-A"]);
    run_git(&repo(dir.path()), &["commit", "-m", "add architecture"]);
    std::fs::write(src.join("lib.rs"), "use cli::main;\n").expect("edit source");

    plumb_in(dir.path())
        .args(["check", "--diff", "--offline"])
        .assert()
        .code(2)
        .stdout(
            predicates::str::contains("boundary").and(predicates::str::contains("crates/core")),
        );
}

/// `git diff` prints paths relative to the repository root, so the gate has to
/// resolve the root rather than trust the working directory. Without that this
/// run reads no manifest, finds nothing, and exits 0 — passing silently, which
/// is the failure mode nobody notices.
#[test]
fn check_works_from_a_subdirectory_of_the_repository() {
    let dir = repo_with_requirements("ruff==0.4.0\n");
    std::fs::write(
        in_repo(dir.path(), "requirements.txt"),
        "ruff==0.4.0\nrequestz==2.31.0\n",
    )
    .expect("write");
    seed_cache(dir.path(), "requestz", false, 0);
    let nested = in_repo(dir.path(), "packages").join("api");
    std::fs::create_dir_all(&nested).expect("create dirs");

    plumb_in(dir.path())
        .args(["check", "--diff", "--offline"])
        .current_dir(&nested)
        .assert()
        .code(1)
        .stdout(predicates::str::contains("requestz"));
}

/// The gate must survive the git config its user actually has.
///
/// Every setting in [`HOSTILE_GIT_CONFIG`] changes the shape of `git diff`
/// output, and each of them alone was enough to make the gate report a clean
/// run over a manifest it never opened: a mnemonic or custom prefix sends the
/// path somewhere that does not exist on disk, colour puts an escape sequence
/// in front of the header so no file is recorded at all, and an external diff
/// replaces the output entirely.
///
/// A gate that switches itself off because the user likes coloured diffs is
/// worse than no gate, because it looks like it is running. So the settings go
/// in through `GIT_CONFIG_GLOBAL`, which is how they reach a developer, and
/// the run still has to exit 1.
///
/// `GIT_EXTERNAL_DIFF` is set too: it is the environment's spelling of
/// `diff.external`, and it would otherwise be a route around a config-only
/// defence.
#[test]
fn check_survives_a_hostile_but_ordinary_git_config() {
    let dir = repo_with_requirements("ruff==0.4.0\n");
    std::fs::write(
        in_repo(dir.path(), "requirements.txt"),
        "ruff==0.4.0\nrequestz==2.31.0\n",
    )
    .expect("write");
    seed_cache(dir.path(), "requestz", false, 0);

    plumb_in(dir.path())
        .args(["check", "--diff", "--offline"])
        .env("GIT_CONFIG_GLOBAL", hostile_git_config(dir.path()))
        .env("GIT_EXTERNAL_DIFF", "/nonexistent/external-diff")
        .assert()
        .code(1)
        .stdout(predicates::str::contains("requestz"));
}

/// The same config must not turn a clean run into a blocking one either.
///
/// The pair matters: a gate hardened by making everything look changed would
/// pass the test above and be useless. Here `requestz` is *removed* by the
/// change, so it must never be looked up — an implementation treating
/// everything in either version as an addition reports it and fails here.
///
/// The mutation used to be written to `<tempdir>/requirements.txt`, outside
/// the repository: the fixture moved the checkout into `<tempdir>/repo/` and
/// this one call site was missed. The tree was therefore clean —
/// `git status --porcelain` and `git diff --name-only HEAD` both empty — so
/// the hostile config had no diff to reshape, no manifest was opened, and
/// `no findings` was guaranteed for every implementation. Confirmed by
/// mutation: with `deps::added` returning the union instead of the difference,
/// this test fails as written and passed as it stood.
///
/// A guard that cannot fail is worse than no guard, because the suite counts
/// it as coverage.
#[test]
fn a_hostile_git_config_does_not_manufacture_findings() {
    let dir = repo_with_requirements("ruff==0.4.0\nrequestz==2.31.0\n");
    std::fs::write(in_repo(dir.path(), "requirements.txt"), "ruff==0.4.1\n").expect("write");
    seed_cache(dir.path(), "requestz", false, 0);

    plumb_in(dir.path())
        .args(["check", "--diff", "--offline"])
        .env("GIT_CONFIG_GLOBAL", hostile_git_config(dir.path()))
        .env("GIT_EXTERNAL_DIFF", "/nonexistent/external-diff")
        .assert()
        .success()
        .stdout(predicates::str::contains("no findings"));
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
        in_repo(dir.path(), "requirements.txt"),
        "ruff==0.4.0\nhttpx==0.27.0\n",
    )
    .expect("write");
    // Far past every lifetime in the registry, so it can only be honoured.
    seed_cache(dir.path(), "httpx", true, 400 * 24 * 60 * 60);

    plumb_in(dir.path())
        .args(["check", "--diff", "--offline", "--json"])
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
        in_repo(dir.path(), "requirements.txt"),
        "ruff==0.4.0\nhttpx==0.27.0\n",
    )
    .expect("write");
    seed_cache(dir.path(), "httpx", true, 0);

    plumb_in(dir.path())
        .args(["check", "--diff", "--offline"])
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
        let output = plumb_in(dir.path())
            .args(&args)
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

/// `plumb check --diff | head -1` must not abort with a Rust panic.
///
/// A consumer that stops reading is ordinary in a pipeline, and `println!`
/// panics when the write fails. Exit 101 is outside the contract so it cannot
/// be misread as a verdict, but a gate that dies on a pipe is a gate people
/// stop putting in pipes.
///
/// Forced rather than hoped for: the report has to be larger than the pipe
/// buffer, or the child finishes writing before the reader goes away and the
/// failure never happens. Two thousand findings is a few hundred kilobytes,
/// comfortably past it. The read of one line before closing mirrors what
/// `head -1` does.
#[test]
fn a_consumer_that_stops_reading_does_not_crash_the_run() {
    let count = 2000;
    let names: Vec<String> = (0..count).map(|i| format!("pkg-{i:04}")).collect();

    let dir = repo_with_requirements("ruff==0.4.0\n");
    let mut manifest = String::from("ruff==0.4.0\n");
    for name in &names {
        manifest.push_str(name);
        manifest.push('\n');
    }
    std::fs::write(in_repo(dir.path(), "requirements.txt"), &manifest).expect("write");

    // Every name seeded as a fresh `Missing`, so the run blocks on all of them
    // without a request.
    let checked = now_unix();
    let entries: Vec<String> = names
        .iter()
        .map(|name| format!(r#""{name}":{{"exists":false,"checked":{checked}}}"#))
        .collect();
    write_cache(dir.path(), &entries.join(","));

    // The raw `Command`, not `assert_cmd`'s: this test needs the child handle
    // so it can close the pipe while the child is still writing.
    let mut child = std::process::Command::new(assert_cmd::cargo::cargo_bin("plumb"))
        .args(["check", "--diff", "--offline"])
        .current_dir(repo(dir.path()))
        .env(CACHE_FILE_ENV, cache_file(dir.path()))
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("the binary runs");

    {
        let stdout = child.stdout.take().expect("stdout was piped");
        let mut reader = std::io::BufReader::new(stdout);
        let mut line = String::new();
        std::io::BufRead::read_line(&mut reader, &mut line).expect("one line");
        assert!(line.contains("pkg-"), "first line was: {line:?}");
        // Dropping the reader closes this end of the pipe, which is what
        // `head` does once it has what it wants.
    }

    let finished = child.wait_with_output().expect("the child exits");
    let status = finished.status;

    assert_ne!(
        status.code(),
        Some(101),
        "the run panicked when its reader went away"
    );
    assert_eq!(
        status.code(),
        Some(1),
        "the verdict is still the verdict; the reader leaving does not change it"
    );
    // A reader that stopped reading made a decision; complaining about it puts
    // a line on the stderr of every `plumb … | head` in the world.
    let stderr = String::from_utf8_lossy(&finished.stderr);
    assert!(
        !stderr.contains("could not write"),
        "a broken pipe was reported as a failure: {stderr:?}"
    );
}

/// `--diff` is the only scope this version implements, and it is required
/// rather than assumed. A flag that is accepted and ignored means the day a
/// second scope lands, every existing invocation silently changes meaning.
#[test]
fn check_requires_a_scope() {
    let dir = repo_with_requirements("ruff==0.4.0\n");

    plumb_in(dir.path())
        .args(["check", "--offline"])
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

#[test]
fn init_with_defaults_writes_the_working_agreement() {
    let dir = repo_with_requirements("ruff==0.4.0\n");

    plumb_in(dir.path())
        .args(["init", "--defaults"])
        .assert()
        .success()
        .stdout(predicates::str::contains(".plumb/architecture.md"));

    for file in [
        ".plumb/architecture.md",
        ".plumb/conventions.md",
        ".plumb/invariants.md",
        ".plumb/index.md",
    ] {
        assert!(in_repo(dir.path(), file).exists(), "{file} was not written");
    }
    assert!(in_repo(dir.path(), ".plumb/decisions").is_dir());
}

#[test]
fn init_refuses_a_repository_that_already_has_an_agreement() {
    let dir = repo_with_requirements("ruff==0.4.0\n");
    std::fs::create_dir_all(in_repo(dir.path(), ".plumb")).expect("create .plumb");

    plumb_in(dir.path())
        .args(["init", "--defaults"])
        .assert()
        .code(70)
        .stderr(predicates::str::contains(".plumb"));
}

#[test]
fn init_reads_answers_from_stdin() {
    let dir = repo_with_requirements("ruff==0.4.0\n");

    plumb_in(dir.path())
        .arg("init")
        .write_stdin("demo\napi, storage\n\n\n")
        .assert()
        .success();

    let architecture =
        std::fs::read_to_string(in_repo(dir.path(), ".plumb/architecture.md")).expect("read");
    assert!(architecture.contains("## api"), "{architecture}");
    assert!(architecture.contains("## storage"), "{architecture}");
}

/// `read_line` returning `Ok(0)` means stdin hit EOF, not "the user pressed
/// enter on an empty line" — but `prompt` used to treat both the same way
/// and quietly accept every default. A non-interactive invocation of `init`
/// without `--defaults` (piped from `/dev/null`, or a CI step with no stdin
/// at all) must not silently bootstrap the whole working agreement; it
/// should fail and say why.
#[test]
fn init_refuses_eof_on_stdin_without_defaults() {
    let dir = repo_with_requirements("ruff==0.4.0\n");

    plumb_in(dir.path())
        .arg("init")
        .write_stdin("")
        .assert()
        .code(70)
        .stderr(predicates::str::contains("--defaults"));

    assert!(
        !in_repo(dir.path(), ".plumb").exists(),
        ".plumb was created despite the run failing"
    );
}

#[test]
fn adapters_generates_every_instruction_file() {
    let dir = repo_with_requirements("ruff==0.4.0\n");
    plumb_in(dir.path())
        .args(["init", "--defaults"])
        .assert()
        .success();

    plumb_in(dir.path()).arg("adapters").assert().success();

    for file in [
        "CLAUDE.md",
        "AGENTS.md",
        ".cursorrules",
        ".github/copilot-instructions.md",
    ] {
        let content = std::fs::read_to_string(in_repo(dir.path(), file))
            .unwrap_or_else(|_| panic!("{file} missing"));
        assert!(
            content.contains("generated by `plumb adapters`"),
            "{file} lacks the marker"
        );
        assert!(content.contains("Before writing"), "{file} lacks protocols");
    }
}

#[test]
fn adapters_without_an_agreement_points_at_init() {
    let dir = repo_with_requirements("ruff==0.4.0\n");

    plumb_in(dir.path())
        .arg("adapters")
        .assert()
        .code(70)
        .stderr(predicates::str::contains("plumb init"));
}

#[test]
fn adapters_refuses_to_overwrite_a_hand_written_file() {
    let dir = repo_with_requirements("ruff==0.4.0\n");
    plumb_in(dir.path())
        .args(["init", "--defaults"])
        .assert()
        .success();
    std::fs::write(in_repo(dir.path(), "CLAUDE.md"), "hand-written notes\n").expect("write");

    plumb_in(dir.path())
        .arg("adapters")
        .assert()
        .code(70)
        .stderr(predicates::str::contains("CLAUDE.md").and(predicates::str::contains("--force")));

    let content = std::fs::read_to_string(in_repo(dir.path(), "CLAUDE.md")).expect("read");
    assert_eq!(content, "hand-written notes\n", "the file was clobbered");

    plumb_in(dir.path())
        .args(["adapters", "--force"])
        .assert()
        .success();
    let content = std::fs::read_to_string(in_repo(dir.path(), "CLAUDE.md")).expect("read");
    assert!(content.contains("generated by `plumb adapters`"));
}

/// `adapters` used to title generated files with `repo.file_name()`, which is
/// the checkout directory name, not the project name `plumb init` actually
/// captured into `.plumb/architecture.md`. The fixture repository directory
/// is named `repo` (see [`repo`]), so a generated file titled with the
/// directory name reads "repo — working agreement" instead of the answered
/// name — exactly the mismatch this project's own generated CLAUDE.md had
/// against its architecture.md before the fix.
#[test]
fn adapters_titles_generated_files_with_the_name_from_init_not_the_directory_name() {
    let dir = repo_with_requirements("ruff==0.4.0\n");
    plumb_in(dir.path())
        .arg("init")
        .write_stdin("demo-project\napi, storage\n\n\n")
        .assert()
        .success();

    plumb_in(dir.path()).arg("adapters").assert().success();

    let content = std::fs::read_to_string(in_repo(dir.path(), "CLAUDE.md")).expect("read");
    assert!(
        content.contains("# demo-project — working agreement"),
        "{content}"
    );
}

/// The write loop used to `create_dir_all` each target's parent one target
/// at a time, so a directory component that exists as a regular file (here
/// `.github`, blocking `.github/copilot-instructions.md`) only surfaced
/// after earlier targets — CLAUDE.md, AGENTS.md, .cursorrules — had already
/// been written, leaving a half-regenerated repository behind a nonzero exit
/// code. Every target's writability must be checked before any target is
/// written.
#[test]
fn adapters_refuses_when_a_target_directory_component_is_a_file() {
    let dir = repo_with_requirements("ruff==0.4.0\n");
    plumb_in(dir.path())
        .args(["init", "--defaults"])
        .assert()
        .success();
    std::fs::write(in_repo(dir.path(), ".github"), "not a directory\n").expect("write");

    plumb_in(dir.path()).arg("adapters").assert().code(70);

    assert!(
        !in_repo(dir.path(), "CLAUDE.md").exists(),
        "CLAUDE.md was written despite the run failing"
    );
}

/// `std::fs::read_to_string` fails on a target that is not valid UTF-8, and
/// `if let Ok(existing) = ...` used to treat that failure the same as "the
/// file does not exist" — silently overwriting a UTF-16 file, or one this
/// process cannot even read, with no `--force` in sight. The guard must
/// distinguish "genuinely absent" from "present but unreadable" and refuse
/// the latter without touching it.
#[test]
fn adapters_refuses_a_target_that_is_not_valid_utf8() {
    let dir = repo_with_requirements("ruff==0.4.0\n");
    plumb_in(dir.path())
        .args(["init", "--defaults"])
        .assert()
        .success();
    // UTF-16LE BOM followed by "hi" and a lone low surrogate — not valid
    // UTF-8 anywhere, and with no 0x0A byte so the whole thing is "line one".
    let non_utf8: &[u8] = &[0xFF, 0xFE, b'h', 0x00, b'i', 0x00, 0x3D, 0xD8];
    std::fs::write(in_repo(dir.path(), "CLAUDE.md"), non_utf8).expect("write");

    plumb_in(dir.path())
        .arg("adapters")
        .assert()
        .code(70)
        .stderr(predicates::str::contains("CLAUDE.md"));

    let content = std::fs::read(in_repo(dir.path(), "CLAUDE.md")).expect("read");
    assert_eq!(content, non_utf8, "the file was clobbered");
}

/// A first line that merely talks about generated siblings is not the marker
/// line plumb itself writes, so it must be refused exactly like any other
/// hand-written content — `first_line.contains(MARKER)` used to accept it.
#[test]
fn adapters_refuses_a_file_that_only_mentions_the_marker_in_prose() {
    let dir = repo_with_requirements("ruff==0.4.0\n");
    plumb_in(dir.path())
        .args(["init", "--defaults"])
        .assert()
        .success();
    std::fs::write(
        in_repo(dir.path(), "CLAUDE.md"),
        "Note: siblings are generated by `plumb adapters`\n",
    )
    .expect("write");

    plumb_in(dir.path())
        .arg("adapters")
        .assert()
        .code(70)
        .stderr(predicates::str::contains("CLAUDE.md").and(predicates::str::contains("--force")));

    let content = std::fs::read_to_string(in_repo(dir.path(), "CLAUDE.md")).expect("read");
    assert_eq!(
        content, "Note: siblings are generated by `plumb adapters`\n",
        "the file was clobbered"
    );
}

/// `.plumb/` and the generated instruction files drift apart when someone
/// edits the artefacts and forgets to regenerate. `check --diff` is meant to
/// catch that, end to end through `init`, `adapters` and `check` as a real
/// user would run them.
#[test]
fn drift_between_plumb_and_generated_files_warns_end_to_end() {
    let dir = repo_with_requirements("ruff==0.4.0\n");
    plumb_in(dir.path())
        .args(["init", "--defaults"])
        .assert()
        .success();
    plumb_in(dir.path()).arg("adapters").assert().success();
    run_git(&repo(dir.path()), &["add", "-A"]);
    run_git(&repo(dir.path()), &["commit", "-m", "adopt plumb"]);
    let invariants = in_repo(dir.path(), ".plumb/invariants.md");
    let mut text = std::fs::read_to_string(&invariants).expect("read");
    text.push_str("\n## New invariant\n\nAdded after generation.\n");
    std::fs::write(&invariants, text).expect("write");

    plumb_in(dir.path())
        .args(["check", "--diff", "--offline"])
        .assert()
        .code(2)
        .stdout(predicates::str::contains("adapter-drift"));
}

#[test]
fn index_scans_tracked_sources_into_plumb_index() {
    let dir = repo_with_requirements("ruff==0.4.0\n");
    plumb_in(dir.path())
        .args(["init", "--defaults"])
        .assert()
        .success();
    let src = in_repo(dir.path(), "src");
    std::fs::create_dir_all(&src).expect("mkdir");
    std::fs::write(src.join("lib.rs"), "pub fn visible() {}\nfn hidden() {}\n").expect("write");
    run_git(&repo(dir.path()), &["add", "-A"]);
    run_git(&repo(dir.path()), &["commit", "-m", "sources"]);

    plumb_in(dir.path())
        .arg("index")
        .assert()
        .success()
        .stdout(predicates::str::contains(".plumb/index.md"));

    let index = std::fs::read_to_string(in_repo(dir.path(), ".plumb/index.md")).expect("read");
    assert!(index.contains("## src/lib.rs"), "{index}");
    assert!(index.contains("- fn `visible`"), "{index}");
    assert!(!index.contains("hidden"), "{index}");
    assert!(index.contains("generated by `plumb index`"), "{index}");
}

#[test]
fn index_without_an_agreement_points_at_init() {
    let dir = repo_with_requirements("ruff==0.4.0\n");

    plumb_in(dir.path())
        .arg("index")
        .assert()
        .code(70)
        .stderr(predicates::str::contains("plumb init"));
}

/// `.plumb/index.md` drifts the same way the generated instruction files do:
/// someone scans it once with `plumb index`, then the public surface grows
/// and nobody re-runs the scan. `check --diff` catches that too, end to end
/// through `init`, `index` and `check` as a real user would run them.
#[test]
fn a_stale_index_warns_end_to_end() {
    let dir = repo_with_requirements("ruff==0.4.0\n");
    plumb_in(dir.path())
        .args(["init", "--defaults"])
        .assert()
        .success();
    let src = in_repo(dir.path(), "src");
    std::fs::create_dir_all(&src).expect("mkdir");
    std::fs::write(src.join("lib.rs"), "pub fn first() {}\n").expect("write");
    run_git(&repo(dir.path()), &["add", "-A"]);
    run_git(&repo(dir.path()), &["commit", "-m", "sources"]);
    plumb_in(dir.path()).arg("index").assert().success();
    run_git(&repo(dir.path()), &["add", "-A"]);
    run_git(&repo(dir.path()), &["commit", "-m", "scan"]);
    std::fs::write(
        src.join("lib.rs"),
        "pub fn first() {}\npub fn second() {}\n",
    )
    .expect("write");

    plumb_in(dir.path())
        .args(["check", "--diff", "--offline"])
        .assert()
        .code(2)
        .stdout(predicates::str::contains("index-stale"));
}

/// Volume alone, with no `.plumb/architecture.md` present: coupling has
/// nothing to resolve against, but the report still runs and shows the
/// commit's line counts.
#[test]
fn report_shows_volume_per_commit_without_an_architecture() {
    let dir = repo_with_requirements("ruff==0.4.0\n");
    std::fs::write(in_repo(dir.path(), "notes.txt"), "one\ntwo\nthree\n").expect("write");
    run_git(&repo(dir.path()), &["add", "-A"]);
    run_git(&repo(dir.path()), &["commit", "-m", "add notes"]);

    plumb_in(dir.path())
        .args(["report", "--commits", "10"])
        .assert()
        .success()
        .stdout(
            predicates::str::contains("add notes")
                .and(predicates::str::contains("+3"))
                .and(predicates::str::contains("decay signature: 0 of")),
        );
}

/// A commit that both grows in volume and adds a new cross-module edge is
/// the decay pattern from taxonomy §4.5, and the report must name it.
///
/// The declared architecture permits nothing for `crates/core` — irrelevant
/// here: `report` measures edges, it does not judge them. The boundary gate
/// is a different feature; the edge exists whether or not it is permitted.
#[test]
fn report_counts_a_new_cross_module_edge_as_rising_coupling() {
    let dir = repo_with_requirements("ruff==0.4.0\n");
    let plumb_dir = in_repo(dir.path(), ".plumb");
    std::fs::create_dir_all(&plumb_dir).expect("mkdir");
    std::fs::write(
        plumb_dir.join("architecture.md"),
        "## crates/core\n\nMay depend on: cli.\n\n## crates/cli\n\nMay depend on: (none).\n",
    )
    .expect("write");
    let src = in_repo(dir.path(), "crates/core/src");
    std::fs::create_dir_all(&src).expect("mkdir");
    std::fs::write(src.join("lib.rs"), "// nothing yet\n").expect("write");
    run_git(&repo(dir.path()), &["add", "-A"]);
    run_git(&repo(dir.path()), &["commit", "-m", "scaffold"]);
    // The decay-shaped commit: volume up, and a new cross-module edge.
    std::fs::write(
        src.join("lib.rs"),
        "use cli::main;\npub fn one() {}\npub fn two() {}\n",
    )
    .expect("write");
    run_git(&repo(dir.path()), &["add", "-A"]);
    run_git(&repo(dir.path()), &["commit", "-m", "reach across"]);

    plumb_in(dir.path())
        .args(["report", "--commits", "1"])
        .assert()
        .success()
        .stdout(
            predicates::str::contains("reach across")
                .and(predicates::str::contains("decay signature: 1 of 1")),
        );
}

#[test]
fn report_json_is_parseable() {
    let dir = repo_with_requirements("ruff==0.4.0\n");

    let output = plumb_in(dir.path())
        .args(["report", "--commits", "5", "--json"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout).into_owned();
    let value: serde_json::Value = serde_json::from_str(stdout.trim()).expect("parses");
    assert!(value["commits"].is_array(), "{value}");
}
