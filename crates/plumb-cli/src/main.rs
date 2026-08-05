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
        /// Check the working diff. Currently the only supported scope.
        ///
        /// Required rather than assumed: a flag that is accepted and ignored
        /// means the day a second scope lands, every existing invocation
        /// silently changes meaning.
        #[arg(long, required = true)]
        diff: bool,
        /// Never contact a registry; unresolved names are reported as unknown.
        #[arg(long)]
        offline: bool,
        /// Emit JSON instead of text.
        #[arg(long)]
        json: bool,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();

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

    // One registry for the whole run. It holds the cache and the latch that
    // stops a dead network being retried once per dependency, and both exist
    // to keep the gate fast enough that nobody turns it off.
    let registry = registry::PyPiRegistry::new(
        offline,
        repo.join(".plumb").join("cache").join("registry.json"),
    );

    match check::run(&repo, &registry, json) {
        Ok((output, code)) => {
            // Exactly one trailing newline either way: `render_text` ends with
            // one and `render_json` does not.
            println!("{}", output.trim_end_matches('\n'));
            report_stale_answers(&registry);
            // `exit_code` returns 0, 1 or 2. The fallback is unreachable, and
            // is an internal error rather than a 0 so that a gate which
            // somehow produced a code it cannot report never reports success.
            ExitCode::from(u8::try_from(code).unwrap_or(EXIT_INTERNAL_ERROR))
        }
        Err(error) => failed(&error),
    }
}

/// Says so when the run answered from cache entries past their lifetime.
///
/// Offline, an `Exists` is honoured at any age — see
/// [`registry::PyPiRegistry::stale_answers`]. That trade is defensible only
/// because the run reports it, so this is part of the gate's output, not a
/// nicety.
///
/// On stderr, and with no effect on the exit code. Stale data here can only
/// have produced a non-blocking answer, so it is not a finding; and stdout
/// carries a report that `--json` promises is parseable.
fn report_stale_answers(registry: &registry::PyPiRegistry) {
    let stale = registry.stale_answers();
    if stale == 0 {
        return;
    }
    let package = if stale == 1 { "package" } else { "packages" };
    eprintln!(
        "plumb: {stale} {package} answered from stale cache data; \
         run without --offline to confirm against the registry"
    );
}

/// Reports a run that could not happen.
fn failed(error: &anyhow::Error) -> ExitCode {
    eprintln!("plumb: {error:#}");
    ExitCode::from(EXIT_INTERNAL_ERROR)
}
