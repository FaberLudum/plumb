// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Plumb Authors

mod adapters_cmd;
mod check;
mod git;
mod init;
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
        /// Check the working tree against HEAD. Currently the only supported
        /// scope.
        ///
        /// A change that is already committed is outside this scope: there is
        /// nothing left to compare, so the run checks nothing and exits 0. On
        /// a fresh clone or a CI checkout the tree is clean and that is the
        /// whole run. Checking a commit needs `plumb check --repo`, which is
        /// not implemented yet.
        ///
        /// Staged content is not checked either. A path is listed only when
        /// the working tree differs from HEAD, so an addition that is staged
        /// and then reverted on disk is invisible, and a manifest staged one
        /// way and edited another is checked on the working-tree side rather
        /// than the side about to be committed.
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
    /// Write the .plumb/ working agreement for this repository.
    Init {
        /// Accept every default without asking.
        #[arg(long)]
        defaults: bool,
    },
    /// Regenerate agent instruction files from .plumb/.
    Adapters {
        /// Overwrite target files that plumb did not generate.
        #[arg(long)]
        force: bool,
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
        Commands::Init { defaults } => {
            let repo = match git::repo_root(Path::new(".")) {
                Ok(repo) => repo,
                Err(error) => return failed(&error),
            };
            let stdin = std::io::stdin();
            match init::run(&repo, defaults, stdin.lock(), std::io::stdout()) {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => failed(&error),
            }
        }
        Commands::Adapters { force } => {
            let repo = match git::repo_root(Path::new(".")) {
                Ok(repo) => repo,
                Err(error) => return failed(&error),
            };
            match adapters_cmd::run(&repo, force, std::io::stdout()) {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => failed(&error),
            }
        }
    }
}

/// Runs the diff gate against the repository the working directory is in.
fn check_diff(offline: bool, json: bool) -> ExitCode {
    let repo = match git::repo_root(Path::new(".")) {
        Ok(repo) => repo,
        Err(error) => return failed(&error),
    };

    // The cache path comes from `registry::cache_path`, which puts it outside
    // the repository. It used to be `<repo>/.plumb/cache/registry.json`, where
    // committing one file disabled the gate for thirty days.
    let cache = registry::cache_path(&repo);
    report_on_the_cache(&repo, cache.as_deref());

    // One registry for the whole run. It holds the cache and the latch that
    // stops a dead network being retried once per dependency, and both exist
    // to keep the gate fast enough that nobody turns it off.
    let registry = registry::PyPiRegistry::new(offline, cache);

    match check::run(&repo, &registry, json) {
        Ok(outcome) => {
            let check::Outcome { report, code } = outcome;
            write_report(&report);
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

/// Says whatever is true about where this run's cache is.
///
/// Three things can be worth saying, and they are mutually exclusive on
/// purpose, because the first version of this said the second one
/// unconditionally: with `PLUMB_CACHE_FILE` pointed at the old in-repository
/// path, the binary printed "no longer read and can be deleted" **while
/// reading it and answering from it**. A gate that makes a false statement
/// about its own trust store is the defect class this project is named for, so
/// the containment check comes first and the reassurance is only printed when
/// it is earned.
///
/// All on stderr and none of it touches the exit code, like every other note
/// this binary makes: nothing here is a rule violation, and stdout carries a
/// report `--json` promises is parseable.
fn report_on_the_cache(repo: &Path, cache: Option<&Path>) {
    match cache {
        // The dangerous configuration, reachable only by setting
        // `PLUMB_CACHE_FILE` at something inside the checkout. Not refused,
        // because the variable is set by whoever runs the gate and refusing
        // would break a deliberate if unwise setup — but never silent, because
        // it reinstates exactly the hole the cache was moved out of.
        Some(path) if inside(path, repo) => eprintln!(
            "plumb: the registry cache in use ({}) is inside the repository; a \
             cache file that travels with a change can assert that a package \
             exists and turn this gate off",
            path.display()
        ),
        // No trusted directory could be named, so this run does not cache.
        // Worth one line: the next run pays for it again, and a user who wants
        // a cache can name one.
        None => eprintln!(
            "plumb: no cache directory could be resolved, so this run remembers \
             nothing; set {} to name one",
            registry::CACHE_FILE_ENV
        ),
        // The ordinary case. Mention the leftover file only here, where the
        // claim that it is unread is actually true.
        Some(_) => {
            let legacy = registry::legacy_cache_path(repo);
            if legacy.exists() {
                eprintln!(
                    "plumb: {} is no longer read and can be deleted; the \
                     registry cache now lives outside the repository, because a \
                     cache file inside one can be committed and a committed \
                     cache answers for the gate",
                    legacy.display()
                );
            }
        }
    }
}

/// Whether `path` lies inside `repo`.
///
/// Canonicalised on both sides so that two spellings of the same directory — a
/// symlinked temporary directory, a short path on Windows — do not read as
/// different places and let an in-repository cache past the check. The cache
/// file itself need not exist yet, so the deepest ancestor that does is what
/// gets canonicalised.
///
/// A failure to canonicalise falls back to comparing the paths as written,
/// which can only under-report; the fallback is a best effort on a warning, not
/// a security boundary.
fn inside(path: &Path, repo: &Path) -> bool {
    let resolved = path
        .ancestors()
        .find_map(|ancestor| std::fs::canonicalize(ancestor).ok());
    match (resolved, std::fs::canonicalize(repo)) {
        (Some(path), Ok(repo)) => path.starts_with(repo),
        _ => path.starts_with(repo),
    }
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
