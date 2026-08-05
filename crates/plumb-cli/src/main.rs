// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Plumb Authors

mod check;
mod git;
mod registry;

use clap::{Parser, Subcommand};
use std::path::Path;
use std::process::ExitCode;

/// Exit code for a run that could not happen.
///
/// Deliberately outside the 0/1/2 contract. CI has to be able to tell "the
/// gate found nothing" from "the gate did not run", and a broken gate that
/// exits 0 is one nobody notices for months. 70 is `EX_SOFTWARE` from
/// sysexits(3).
const EXIT_INTERNAL_ERROR: u8 = 70;

/// Exit code for a command line that could not be understood.
///
/// Outside the contract for the same reason, and it has to be stated rather
/// than left to clap: clap's own default for a usage error is 2, which is the
/// code for "the gate ran and warned". 64 is `EX_USAGE` from sysexits(3).
const EXIT_USAGE: u8 = 64;

#[derive(Parser)]
#[command(
    name = "plumb",
    version,
    about = "Structural gates for AI-generated code"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run gates against the working diff.
    Check {
        /// Check the working tree and the index against HEAD. Currently the
        /// only supported scope.
        ///
        /// A change that is already committed is outside this scope: there is
        /// nothing left to compare, so the run checks nothing and exits 0. On
        /// a fresh clone or a CI checkout the tree is clean and that is the
        /// whole run. Checking a commit needs `plumb check --repo`, which is
        /// not implemented yet.
        ///
        /// Required rather than assumed: a flag that is accepted and ignored
        /// means the day a second scope lands, every existing invocation
        /// silently changes meaning.
        #[arg(long, required = true)]
        diff: bool,
        // This read "unresolved names are reported as unknown" until someone
        // ran it: nothing is reported, on either stream. The README carried
        // the same sentence and was corrected first, which is how the tool
        // came to contradict its own documentation — and `--help` is the
        // surface a user reads, so it was the one that mattered.
        /// Never contact a registry; a name the cache cannot answer for yields
        /// no finding at all, so a cold cache reports nothing and exits 0.
        ///
        /// An unreachable registry is not evidence that a package does not
        /// exist, so it can never be a violation. The cost is that silence
        /// answers both "every name resolved" and "nothing could be looked
        /// up", and only knowing this flag was passed tells the two apart.
        #[arg(long)]
        offline: bool,
        /// Emit JSON instead of text.
        #[arg(long)]
        json: bool,
    },
}

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => return reported(error),
    };

    match cli.command {
        Commands::Check {
            diff,
            offline,
            json,
        } => {
            debug_assert!(diff, "clap must not admit a check with no scope");
            check_diff(offline, json)
        }
    }
}

/// Runs the diff gate against the repository the working directory is in.
fn check_diff(offline: bool, json: bool) -> ExitCode {
    let repo = match git::repo_root(Path::new(".")) {
        Ok(repo) => repo,
        Err(error) => return failed(&error),
    };

    report_cache_inside_the_repository(&repo);

    // One registry for the whole run. It holds the cache and the latch that
    // stops a dead network being retried once per dependency, and both exist
    // to keep the gate fast enough that nobody turns it off.
    //
    // The cache path comes from `registry::cache_path`, which puts it outside
    // the repository. It used to be `<repo>/.plumb/cache/registry.json`, where
    // committing one file disabled the gate for thirty days.
    let registry = registry::PyPiRegistry::new(offline, registry::cache_path(&repo));

    match check::run(&repo, &registry, json) {
        Ok(outcome) => {
            let check::Outcome {
                report,
                code,
                unreadable,
            } = outcome;
            write_report(&report);
            report_unreadable_manifests(&unreadable);
            report_stale_packages(&registry);
            // `exit_code` returns 0, 1 or 2. The fallback is unreachable, and
            // is an internal error rather than a 0 so that a gate which
            // somehow produced a code it cannot report never reports success.
            ExitCode::from(u8::try_from(code).unwrap_or(EXIT_INTERNAL_ERROR))
        }
        Err(error) => failed(&error),
    }
}

/// Writes the report to stdout, surviving a reader that has gone away.
///
/// `println!` panics when the write fails, and `plumb check --diff | head -1`
/// makes it fail as a matter of course. Exit 101 is outside the contract so it
/// could not be misread as a verdict, but a gate that dies in a pipeline is a
/// gate people take out of their pipelines.
///
/// A broken pipe is a consumer's decision, not a fault, so it is not reported.
/// Any other write failure is, because then the report is lost for a reason the
/// user may be able to do something about. Neither changes the exit code: the
/// verdict was reached before the write, and it stands whether or not anyone
/// was still listening.
///
/// Exactly one trailing newline either way — `render_text` ends with one and
/// `render_json` does not.
fn write_report(output: &str) {
    use std::io::Write;

    let mut stdout = std::io::stdout().lock();
    let written =
        writeln!(stdout, "{}", output.trim_end_matches('\n')).and_then(|()| stdout.flush());

    if let Err(error) = written
        && error.kind() != std::io::ErrorKind::BrokenPipe
    {
        eprintln!("plumb: could not write the report: {error}");
    }
}

/// Says so when a manifest the change touched could not be read.
///
/// Such a file contributes no dependency, so the run reports nothing about it
/// and exits 0 — which is right, because a file this process could not decode
/// is not evidence that a package does not exist. What was wrong was doing
/// that in silence: one Latin-1 byte in a comment used to uncheck a whole
/// manifest with nothing said on either stream.
///
/// One line per file rather than a count, unlike
/// [`report_stale_packages`]. A stale package is a fact about a lookup and the
/// reader can only act on the total; an unreadable manifest is a file the
/// reader can open and fix, so naming it is the whole value of saying anything.
///
/// On stderr, and with no effect on the exit code, for the reasons that hold
/// for every note this binary makes: nothing here is a rule violation, and
/// stdout carries a report `--json` promises is parseable.
fn report_unreadable_manifests(unreadable: &[check::Unreadable]) {
    for manifest in unreadable {
        eprintln!(
            "plumb: {} was not checked: {}",
            manifest.path, manifest.reason
        );
    }
}

/// Says so when the run answered from cache entries past their lifetime.
///
/// Offline, an `Exists` is honoured at any age — see
/// [`registry::PyPiRegistry::stale_packages`]. That trade is defensible only
/// because the run reports it, so this is part of the gate's output, not a
/// nicety.
///
/// On stderr, and with no effect on the exit code. Stale data here can only
/// have produced a non-blocking answer, so it is not a finding; and stdout
/// carries a report that `--json` promises is parseable.
fn report_stale_packages(registry: &registry::PyPiRegistry) {
    let stale = registry.stale_packages();
    if stale == 0 {
        return;
    }
    let package = if stale == 1 { "package" } else { "packages" };
    eprintln!(
        "plumb: {stale} {package} answered from stale cache data; \
         run without --offline to confirm against the registry"
    );
}

/// Says so when the old in-repository cache file is still there.
///
/// It is not read — see [`registry::legacy_cache_path`] for why migrating it
/// would launder exactly the entries the move exists to refuse. But leaving it
/// unmentioned means every existing checkout keeps a file that looks like it
/// is doing something and is not, so the run says it out loud and says what to
/// do about it.
///
/// On stderr and with no effect on the exit code, like every other note this
/// binary makes: nothing here is a rule violation, and stdout carries a report
/// `--json` promises is parseable.
fn report_cache_inside_the_repository(repo: &Path) {
    let legacy = registry::legacy_cache_path(repo);
    if !legacy.exists() {
        return;
    }
    eprintln!(
        "plumb: {} is no longer read and can be deleted; the registry cache \
         now lives outside the repository, because a cache file inside one can \
         be committed and a committed cache answers for the gate",
        legacy.display()
    );
}

/// Reports a run that could not happen.
fn failed(error: &anyhow::Error) -> ExitCode {
    eprintln!("plumb: {error:#}");
    ExitCode::from(EXIT_INTERNAL_ERROR)
}

/// Prints whatever clap wants to say and picks the exit code itself.
///
/// [`clap::Parser::parse`] would do both, but its code for a usage error is 2
/// — the code this project has already promised means "the gate ran and
/// warned". Anything that stops the run before it starts has to land outside
/// the contract instead.
///
/// `--help` and `--version` arrive here as errors too. They are requests that
/// were satisfied, so they print to stdout and exit 0; `use_stderr` is what
/// tells the two apart.
fn reported(error: clap::Error) -> ExitCode {
    // Failing to print leaves nothing worth reporting: the exit code below is
    // the diagnostic at that point.
    let _ = error.print();

    if error.use_stderr() {
        ExitCode::from(EXIT_USAGE)
    } else {
        ExitCode::SUCCESS
    }
}
