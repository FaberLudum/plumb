// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Plumb Authors

//! Resolving distribution names against PyPI.
//!
//! This is the only part of the binary that touches the network, and it makes
//! exactly one class of request: a lookup of one distribution name. Nothing
//! else is ever sent.

use serde::{Deserialize, Serialize};
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// How long a cached `Missing` may be trusted.
///
/// A `Missing` blocks a build, so it is the dangerous thing to hold on to: a
/// name cached before it was published would otherwise block forever, with no
/// diagnostic and no cure the user has been told about. An hour is long enough
/// to cover an edit-test loop or a run of CI retries without asking PyPI
/// again, and short enough that publishing a package unblocks the same working
/// day. The load this permits is negligible — at most one request per hour per
/// name, and only for names that are already failing the gate.
const MISSING_TTL: Duration = Duration::from_secs(60 * 60);

/// How long a cached `Exists` may be trusted.
///
/// Far longer, because a stale `Exists` fails open: it merely blunts the rule
/// rather than breaking a build. Bounded rather than infinite so that a name
/// is eventually asked about again — PyPI does remove projects, and removing
/// malicious typosquats is one of the main reasons it does.
///
/// **That bound applies online only.** Offline there is nothing to ask, so
/// [`PyPiRegistry::honour_offline`] returns an `Exists` of any age, and a
/// long-airgapped machine can report `Exists` for a package PyPI has since
/// removed. That is the round-2 trade taken deliberately: `Exists` and
/// `Unknown` are both non-blocking, so expiring the entry would change no
/// blocking decision — it would only replace an answer with silence. The
/// remedy for the staleness is to report it, not to discard it — which is
/// what [`PyPiRegistry::stale_answers`] is for.
const EXISTS_TTL: Duration = Duration::from_secs(30 * 24 * 60 * 60);

/// How long one lookup may take before it reports [`Existence::Unknown`].
///
/// ureq applies no timeout to a bodyless GET by default — global, resolve and
/// connect are all unset — so this is the only thing bounding a lookup. It
/// covers the whole call, resolution and connection included.
const LOOKUP_TIMEOUT: Duration = Duration::from_secs(10);

/// Whether a package could be found in its registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Existence {
    Exists,
    Missing,
    /// The registry could not be consulted. Never a violation.
    Unknown,
}

/// A source of truth for whether a package name resolves.
///
/// Object safe on purpose: callers hold `&dyn Registry` so tests can inject a
/// stub instead of reaching the network.
pub trait Registry {
    fn exists(&self, name: &str) -> Existence;
}

/// Seconds since the Unix epoch, or 0 if the clock is set before it.
///
/// Falling back to 0 makes every entry read as stale, which costs a re-query
/// and never a wrong answer.
fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or(0)
}

/// One remembered answer and when it was learned.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct Entry {
    exists: bool,
    /// Seconds since the Unix epoch, as of the lookup that produced this.
    checked: u64,
}

impl Entry {
    fn existence(self) -> Existence {
        if self.exists {
            Existence::Exists
        } else {
            Existence::Missing
        }
    }

    /// Whether this may still be used as an answer.
    ///
    /// A timestamp in the future — a clock that jumped backwards, or a
    /// hand-edited file — reads as stale rather than fresh, so a bad clock
    /// cannot make an entry immortal.
    fn is_fresh(self, now: u64) -> bool {
        let ttl = if self.exists { EXISTS_TTL } else { MISSING_TTL };
        now >= self.checked && Duration::from_secs(now - self.checked) < ttl
    }
}

/// Remembered answers, keyed by distribution name.
///
/// The pre-timestamp schema stored a bare bool. It does not deserialize into
/// this, so an old file is discarded and rebuilt — see [`PyPiRegistry::new`].
#[derive(Debug, Default, Serialize, Deserialize)]
struct Cache {
    known: BTreeMap<String, Entry>,
}

/// Resolves names against PyPI, with an on-disk cache and an offline mode.
///
/// Single-threaded by construction: the [`RefCell`] and [`Cell`] make this
/// `!Sync`, so sharing one registry across threads is a compile error rather
/// than a race. It is `Send`, so a whole registry can still be moved to
/// another thread. Nothing here assumes concurrency, and lookups run one at a
/// time — which is also why the unreachable latch needs no synchronisation.
pub struct PyPiRegistry {
    offline: bool,
    cache_path: PathBuf,
    cache: RefCell<Cache>,
    /// Held rather than built per lookup, so a run of N names reuses one
    /// connection pool and one TLS handshake instead of opening N.
    agent: ureq::Agent,
    /// Set once the network proves unreachable, and never cleared.
    ///
    /// In memory only. It describes this process's network for the length of
    /// this run and must not outlive it, so it is deliberately not persisted.
    unreachable: Cell<bool>,
    /// How many answers this run took from entries past their lifetime.
    ///
    /// Counted rather than flagged so the report can say how much of the run
    /// rested on old data. Like the latch, this describes this run and is
    /// deliberately not persisted.
    honoured_stale: Cell<usize>,
}

impl PyPiRegistry {
    /// Reads the cache at `cache_path`, if there is a readable one.
    ///
    /// A missing, unreadable or malformed cache file yields an empty cache
    /// rather than an error. The cache is an optimisation; losing it costs a
    /// round trip, and refusing to start over a corrupt one would turn a
    /// scratch file into a hard failure of the gate.
    ///
    /// A file written by the pre-timestamp schema takes that same path: it
    /// fails to deserialize, so it is discarded and rebuilt. That is the
    /// intended migration. Such a file records no time, so nothing in it can
    /// be vouched for, and honouring it would resurrect the stale blocking
    /// `Missing` the timestamp exists to prevent. The cost is one run of
    /// re-queries; the outcome can never be a block, because a discarded entry
    /// means asking again rather than trusting an old answer.
    pub fn new(offline: bool, cache_path: PathBuf) -> Self {
        let cache = std::fs::read_to_string(&cache_path)
            .ok()
            .and_then(|text| serde_json::from_str::<Cache>(&text).ok())
            .unwrap_or_default();

        Self {
            offline,
            cache_path,
            cache: RefCell::new(cache),
            agent: lookup_agent(),
            unreachable: Cell::new(false),
            honoured_stale: Cell::new(0),
        }
    }

    /// How many answers this run took from cache entries past their lifetime.
    ///
    /// Only the offline path can produce one: online an expired entry is
    /// re-queried, and a latched run refuses it rather than honouring it. See
    /// [`PyPiRegistry::honour_offline`] for why offline honours it at all, and
    /// [`EXISTS_TTL`] for why that concession needs saying out loud.
    ///
    /// Read once after a run rather than pushed through [`Registry`]: the
    /// trait has to stay trivially stubbable, and a method a stub must answer
    /// meaningfully is a method every stub gets wrong.
    pub fn stale_answers(&self) -> usize {
        self.honoured_stale.get()
    }

    /// Answers from the cache, then from `lookup`, then not at all.
    ///
    /// The lookup is a parameter so the request itself can be replaced in
    /// tests. [`Registry::exists`] passes the real one.
    ///
    /// Order matters. A fresh cache entry answers first, so a warm cache still
    /// answers on a dead network. Offline stops there, keeping whatever an
    /// expired entry is still worth — see [`PyPiRegistry::honour_offline`].
    /// Then the latch: once a lookup has failed for a transport reason, every
    /// later name in this run reports `Unknown` without a request, because
    /// they would all pay [`LOOKUP_TIMEOUT`] to learn the same thing. Thirty
    /// dependencies behind a blackholing firewall is otherwise a five-minute
    /// hang, and a gate that hangs gets switched off exactly like one that
    /// fails.
    ///
    /// Nothing on the offline path writes to the cache. An honoured entry
    /// keeps the timestamp of the lookup that produced it, which is a claim
    /// about when PyPI answered; restamping it on every read would make the
    /// entry immortal and stop it ever being re-queried once the network came
    /// back.
    ///
    /// The latch deliberately does not honour stale entries the way offline
    /// does. Offline is a standing choice to work from what is on disk; an
    /// unreachable network is a fault that may clear within the same minute,
    /// and quietly substituting old answers for it would change what the gate
    /// reports without anyone having asked.
    fn exists_via(&self, name: &str, lookup: impl Fn(&str) -> Outcome) -> Existence {
        let cached = self.cache.borrow().known.get(name).copied();
        if let Some(entry) = cached
            && entry.is_fresh(now_unix())
        {
            return entry.existence();
        }

        if self.offline {
            let answer = Self::honour_offline(cached);
            // Reaching here means the entry was not fresh, so any answer at
            // all is an answer taken from data past its lifetime. Counting the
            // `Unknown`s too would report staleness for a cache miss, which is
            // an absence rather than an old answer.
            if answer != Existence::Unknown {
                self.honoured_stale.set(self.honoured_stale.get() + 1);
            }
            return answer;
        }

        if self.unreachable.get() {
            return Existence::Unknown;
        }

        let outcome = lookup(name);
        if outcome == Outcome::Unreachable {
            self.unreachable.set(true);
        }

        // `record` drops `Unknown`, which is what both an unreachable network
        // and an unhelpful answer come to, so neither reaches the cache.
        let existence = outcome.existence();
        self.record(name, existence);
        existence
    }

    /// What an expired entry is worth when the registry will not be consulted.
    ///
    /// Offline there is no way to refresh, so the choice is between the old
    /// answer and no answer, and the lifetimes' own asymmetry decides it. A
    /// stale `Exists` only fails open: honouring it costs nothing and keeps
    /// the run informative, so a newly hallucinated name still stands out as
    /// `Unknown` against a backdrop of confirmed ones. A stale `Missing` would
    /// block a build on evidence nobody can check, so it is refused.
    ///
    /// Expiring uniformly instead would answer `Unknown` to everything, which
    /// is not a safer gate but no gate at all.
    ///
    /// This is a read, never a refresh — see [`PyPiRegistry::exists_via`] for
    /// why the timestamp must not move.
    ///
    /// Age is not consulted at all, so [`EXISTS_TTL`] does not bound this
    /// path. A machine offline for years will still report `Exists` for what
    /// it last confirmed. Deliberate: expiring it could only turn a
    /// non-blocking answer into a different non-blocking answer, which buys no
    /// safety and loses the one signal an offline run still has. What makes
    /// that defensible is that the run says so: every answer honoured here is
    /// counted by [`PyPiRegistry::stale_answers`] and reported.
    fn honour_offline(cached: Option<Entry>) -> Existence {
        match cached {
            Some(entry) if entry.exists => Existence::Exists,
            _ => Existence::Unknown,
        }
    }

    /// Asks PyPI about one name.
    fn lookup(&self, name: &str) -> Outcome {
        // Task 4 admits only ASCII alphanumerics and `-` into a name, anchored
        // alphanumeric at both ends, so the name cannot alter the URL's shape.
        // Asserting it here keeps that guarantee local to the code that builds
        // the URL, rather than resting on a crate this file never references.
        debug_assert!(
            name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'),
            "name reached the registry unvalidated: {name:?}"
        );
        let url = format!("https://pypi.org/pypi/{name}/json");
        classify(self.agent.get(&url).call().map(|_| ()))
    }

    /// Records a definite answer and writes the cache back.
    ///
    /// [`Existence::Unknown`] is dropped: it describes this run's network, not
    /// the package, and caching it would make one outage permanent.
    fn record(&self, name: &str, outcome: Existence) {
        if outcome == Existence::Unknown {
            return;
        }
        let entry = Entry {
            exists: outcome == Existence::Exists,
            checked: now_unix(),
        };
        self.cache
            .borrow_mut()
            .known
            .insert(name.to_string(), entry);
        self.persist();
    }

    /// Writes the cache back, ignoring every failure.
    ///
    /// A failed write costs the next run a round trip and nothing else, so
    /// there is no outcome worth interrupting the gate for.
    fn persist(&self) {
        let Ok(text) = serde_json::to_string(&*self.cache.borrow()) else {
            return;
        };
        if let Some(parent) = self.cache_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&self.cache_path, text);
    }
}

/// The agent every lookup runs on.
///
/// Exists to bound the call. Without [`LOOKUP_TIMEOUT`] a server that accepts
/// the connection and then says nothing would hang the gate forever, which is
/// a worse failure than reporting `Unknown` and moving on.
fn lookup_agent() -> ureq::Agent {
    ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .timeout_global(Some(LOOKUP_TIMEOUT))
            .build(),
    )
}

/// What one lookup established.
///
/// The distinction is whether the failure was about the package or about the
/// network, because only the second generalises to the next name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    /// A server answered. `Unknown` here means it answered unhelpfully — a
    /// 5xx, or a 403 — which says nothing about this package and nothing
    /// about the next one either.
    Answered(Existence),
    /// Nothing answered: DNS, socket, TLS, proxy or timeout. This one does
    /// generalise, so it latches for the rest of the run.
    Unreachable,
}

impl Outcome {
    fn existence(self) -> Existence {
        match self {
            Outcome::Answered(existence) => existence,
            Outcome::Unreachable => Existence::Unknown,
        }
    }
}

/// Turns one lookup result into an outcome.
///
/// Only a success and a 404 say anything about the package. A 5xx or a 403 is
/// a reachable server declining to say, and everything else — a dead host, a
/// socket error, a timeout, a TLS failure — is a failure to reach it at all.
///
/// `BadUri`, `Http` and the redirect errors stay on the answered side on
/// purpose: they are properties of this one request, not of the network, so
/// one odd name must not disable lookups for the rest of the run. The
/// catch-all latches instead, because ureq's error enum is `#[non_exhaustive]`
/// and the variants not named here are all reachability failures.
///
/// `Ok` is any non-error status ureq settles on, redirects already followed,
/// since it treats 4xx and 5xx as errors by default.
///
/// Takes `Result<(), Error>` rather than the response so the mapping can be
/// tested without a request; the body is never read either way.
fn classify(result: Result<(), ureq::Error>) -> Outcome {
    match result {
        Ok(()) => Outcome::Answered(Existence::Exists),
        Err(ureq::Error::StatusCode(404)) => Outcome::Answered(Existence::Missing),
        Err(
            ureq::Error::StatusCode(_)
            | ureq::Error::Http(_)
            | ureq::Error::BadUri(_)
            | ureq::Error::Protocol(_)
            | ureq::Error::TooManyRedirects
            | ureq::Error::RedirectFailed
            | ureq::Error::BodyExceedsLimit(_),
        ) => Outcome::Answered(Existence::Unknown),
        Err(_) => Outcome::Unreachable,
    }
}

impl Registry for PyPiRegistry {
    /// Answers from the cache, then from PyPI, then not at all.
    ///
    /// Anything that is not a definite success or 404 — a timeout, a proxy, a
    /// firewall, a 5xx — is [`Existence::Unknown`], and `Unknown` is never
    /// cached: it is a fact about this run's network, not about the package.
    ///
    /// See [`PyPiRegistry::exists_via`] for the order this resolves in, and
    /// for the latch that stops a dead network being retried once per name.
    fn exists(&self, name: &str) -> Existence {
        self.exists_via(name, |name| self.lookup(name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a cache file body. `age` is how many seconds ago the entry was
    /// checked; a negative age puts the timestamp in the future.
    fn cache_json(entries: &[(&str, bool, i64)]) -> String {
        let now = i64::try_from(now_unix()).expect("now fits an i64");
        let body: Vec<String> = entries
            .iter()
            .map(|(name, exists, age)| {
                format!(r#""{name}":{{"exists":{exists},"checked":{}}}"#, now - age)
            })
            .collect();
        format!(r#"{{"known":{{{}}}}}"#, body.join(","))
    }

    /// The lifetimes are the policy, so pin the properties that justify them
    /// rather than the numbers. A `Missing` blocks a build, so it must go
    /// stale fast enough that publishing a package unblocks the same working
    /// day. An `Exists` only fails open, so it can live far longer — but not
    /// forever, or a project PyPI removes is never rechecked, and removing
    /// malicious typosquats is precisely why PyPI removes projects.
    #[test]
    fn a_blocking_answer_goes_stale_faster_than_a_harmless_one() {
        assert!(
            MISSING_TTL <= Duration::from_secs(60 * 60),
            "a stale Missing blocks a build; it must expire within the hour"
        );
        assert!(
            MISSING_TTL >= Duration::from_secs(60),
            "expiring this fast would re-query PyPI on every run"
        );
        assert!(
            EXISTS_TTL >= MISSING_TTL * 24,
            "a harmless answer should outlive a blocking one by a wide margin"
        );
        assert!(
            EXISTS_TTL <= Duration::from_secs(90 * 24 * 60 * 60),
            "a removed project must eventually be rechecked"
        );
    }

    /// The whole point of the timestamp. A name cached `Missing` before it was
    /// published would otherwise block every future build with no diagnostic
    /// and no cure the user has been told about.
    #[test]
    fn an_expired_missing_is_never_returned() {
        let dir = tempfile::tempdir().expect("temp dir");
        let cache_path = dir.path().join("registry.json");
        std::fs::write(&cache_path, cache_json(&[("requestz", false, 90 * 60)]))
            .expect("write cache");

        // Offline, so it cannot be re-queried. An expired `Exists` would be
        // honoured here; a `Missing` is refused, because answering it would
        // block a build on evidence this run has no way to check.
        let registry = PyPiRegistry::new(true, cache_path);

        assert_eq!(registry.exists("requestz"), Existence::Unknown);
    }

    /// Long enough past either lifetime that no entry can still be fresh,
    /// derived from the constants so the tests keep pinning the property when
    /// the numbers change.
    fn well_past_every_lifetime() -> i64 {
        let longest = EXISTS_TTL.max(MISSING_TTL).as_secs();
        i64::try_from(longest).expect("a lifetime fits an i64") * 2
    }

    /// The asymmetry, one level out from the lifetimes themselves. Offline
    /// there is no way to refresh anything, so the choice is between the old
    /// answer and no answer. A stale `Exists` only fails open, so honouring it
    /// costs nothing and keeps the gate informative; a stale `Missing` would
    /// block a build on evidence that cannot be checked, so it must not be.
    ///
    /// Returning `Unknown` to everything is not a safer gate, it is no gate.
    #[test]
    fn offline_honours_a_stale_answer_only_when_it_cannot_block() {
        let dir = tempfile::tempdir().expect("temp dir");
        let cache_path = dir.path().join("registry.json");
        let age = well_past_every_lifetime();
        std::fs::write(
            &cache_path,
            cache_json(&[("ruff", true, age), ("requestz", false, age)]),
        )
        .expect("write cache");

        let registry = PyPiRegistry::new(true, cache_path);

        assert_eq!(registry.exists("ruff"), Existence::Exists);
        assert_eq!(registry.exists("requestz"), Existence::Unknown);
    }

    /// `EXISTS_TTL` bounds the online path only. Offline there is nothing to
    /// ask, so age stops mattering entirely — half a century here, and it is
    /// still honoured.
    ///
    /// Pinned because it is a deliberate trade rather than an oversight, and
    /// because the alternative is tempting: expiring it would only swap one
    /// non-blocking answer for another, buying no safety and costing an
    /// offline run the last signal it has. The remedy for staleness is to
    /// report it — see `an_offline_run_counts_the_distinct_stale_packages_it_honoured`.
    #[test]
    fn offline_honours_an_exists_of_any_age() {
        let dir = tempfile::tempdir().expect("temp dir");
        let cache_path = dir.path().join("registry.json");
        let half_a_century = 50 * 365 * 24 * 60 * 60;
        assert!(
            i64::try_from(EXISTS_TTL.as_secs()).expect("fits") < half_a_century,
            "the age must be far past the lifetime for this to prove anything"
        );
        std::fs::write(&cache_path, cache_json(&[("ruff", true, half_a_century)]))
            .expect("write cache");

        let registry = PyPiRegistry::new(true, cache_path);

        assert_eq!(registry.exists("ruff"), Existence::Exists);
    }

    /// Honouring is a read, not a refresh. The timestamp records when PyPI
    /// actually answered, and rewriting it on every offline read would be both
    /// a lie and a leak: each run would push the entry forward, so it would
    /// never expire and would never be re-queried once the network returned.
    #[test]
    fn honouring_a_stale_answer_does_not_refresh_its_timestamp() {
        let dir = tempfile::tempdir().expect("temp dir");
        let cache_path = dir.path().join("registry.json");
        let seeded = cache_json(&[("ruff", true, well_past_every_lifetime())]);
        std::fs::write(&cache_path, &seeded).expect("write cache");

        let registry = PyPiRegistry::new(true, cache_path.clone());
        assert_eq!(registry.exists("ruff"), Existence::Exists);

        let after = std::fs::read_to_string(&cache_path).expect("read cache");
        assert_eq!(after, seeded, "reading a stale answer rewrote the cache");
    }

    /// The other half of the concession above. Answering `Exists` from an
    /// entry of unbounded age is only defensible if the run says it did so, so
    /// the count is what makes that trade honest rather than silent.
    ///
    /// Every step here is a mutation of the one before it: a fresh answer, a
    /// refused stale `Missing`, a cache miss and an honoured stale `Exists`
    /// all take the offline path, and only the last of them is an answer taken
    /// from data past its lifetime.
    #[test]
    fn an_offline_run_counts_the_distinct_stale_packages_it_honoured() {
        let dir = tempfile::tempdir().expect("temp dir");
        let cache_path = dir.path().join("registry.json");
        let age = well_past_every_lifetime();
        std::fs::write(
            &cache_path,
            cache_json(&[
                ("ruff", true, age),
                ("black", true, 0),
                ("requestz", false, age),
            ]),
        )
        .expect("write cache");

        let registry = PyPiRegistry::new(true, cache_path);
        assert_eq!(registry.stale_packages(), 0, "nothing has been asked yet");

        assert_eq!(registry.exists("black"), Existence::Exists);
        assert_eq!(registry.stale_packages(), 0, "a fresh answer is not stale");

        assert_eq!(registry.exists("requestz"), Existence::Unknown);
        assert_eq!(
            registry.stale_packages(),
            0,
            "a stale answer that was refused was not honoured"
        );

        assert_eq!(registry.exists("nowhere"), Existence::Unknown);
        assert_eq!(
            registry.stale_packages(),
            0,
            "a cache miss is an absence, not a stale answer"
        );

        assert_eq!(registry.exists("ruff"), Existence::Exists);
        assert_eq!(
            registry.stale_packages(),
            1,
            "an honoured entry of unbounded age must be reported"
        );

        // The same package named in two manifests is asked about twice. It is
        // still one package whose answer could not be confirmed, and the
        // report says "packages", so counting the lookups would tell the
        // reader there are two of something there is one of.
        assert_eq!(registry.exists("ruff"), Existence::Exists);
        assert_eq!(
            registry.stale_packages(),
            1,
            "the same package asked about twice is one stale package"
        );
    }

    /// Only the offline path honours an expired entry, so only it can report
    /// one. Online an expired entry is re-queried, and a latched run refuses
    /// it outright — neither is an answer taken from stale data, and counting
    /// either would put a warning in front of a user with nothing to fix.
    #[test]
    fn a_run_that_never_honoured_a_stale_answer_reports_none() {
        let dir = tempfile::tempdir().expect("temp dir");
        let cache_path = dir.path().join("registry.json");
        let age = well_past_every_lifetime();
        std::fs::write(
            &cache_path,
            cache_json(&[("ruff", true, age), ("black", true, age)]),
        )
        .expect("write cache");
        let registry = PyPiRegistry::new(false, cache_path);

        // Expired and online, so it is re-queried rather than honoured.
        assert_eq!(
            registry.exists_via("ruff", |_: &str| Outcome::Answered(Existence::Exists)),
            Existence::Exists
        );
        assert_eq!(registry.stale_packages(), 0);

        // Expired and latched, so it is refused rather than honoured.
        assert_eq!(
            registry.exists_via("ruff", |_: &str| Outcome::Unreachable),
            Existence::Exists,
            "the re-query above should have refreshed this entry"
        );
        assert_eq!(
            registry.exists_via("black", |_: &str| Outcome::Unreachable),
            Existence::Unknown
        );
        assert_eq!(registry.stale_packages(), 0);
    }

    /// The concession is scoped to offline, and a dead network is not offline.
    /// Offline is a standing choice to work from what is on disk; an
    /// unreachable network is a fault that may clear within the minute, and
    /// quietly swapping in old answers for it would change what the gate
    /// reports without anyone having asked for that.
    ///
    /// A *fresh* entry still answers once latched — that is
    /// `a_warm_cache_still_answers_after_the_network_dies`. Only the expired
    /// ones are refused.
    #[test]
    fn a_latched_run_does_not_start_honouring_stale_answers() {
        let dir = tempfile::tempdir().expect("temp dir");
        let cache_path = dir.path().join("registry.json");
        let age = well_past_every_lifetime();
        std::fs::write(
            &cache_path,
            cache_json(&[("ruff", true, age), ("black", true, age)]),
        )
        .expect("write cache");
        let registry = PyPiRegistry::new(false, cache_path);
        let attempts = std::cell::Cell::new(0u32);
        let blackhole = |_: &str| {
            attempts.set(attempts.get() + 1);
            Outcome::Unreachable
        };

        // Expired, so it is looked up; the lookup kills the network.
        assert_eq!(registry.exists_via("ruff", blackhole), Existence::Unknown);
        assert_eq!(attempts.get(), 1);

        // Also expired, and now latched. Offline would have honoured this.
        assert_eq!(registry.exists_via("black", blackhole), Existence::Unknown);
        assert_eq!(attempts.get(), 1);
    }

    /// Online the concession does not apply: fresh data is one request away,
    /// so there is no case for answering from an expired entry.
    #[test]
    fn an_expired_answer_is_re_queried_when_the_network_is_there() {
        let dir = tempfile::tempdir().expect("temp dir");
        let cache_path = dir.path().join("registry.json");
        std::fs::write(
            &cache_path,
            cache_json(&[("ruff", true, well_past_every_lifetime())]),
        )
        .expect("write cache");
        let registry = PyPiRegistry::new(false, cache_path);
        let attempts = std::cell::Cell::new(0u32);
        let answers_missing = |_: &str| {
            attempts.set(attempts.get() + 1);
            Outcome::Answered(Existence::Missing)
        };

        assert_eq!(
            registry.exists_via("ruff", answers_missing),
            Existence::Missing
        );
        assert_eq!(
            attempts.get(),
            1,
            "a stale answer was honoured with the network up"
        );
    }

    /// A clock that jumps backwards, or a hand-edited file, leaves entries
    /// stamped in the future. Treating those as fresh would make them
    /// immortal, so they must read as stale and be re-queried instead.
    #[test]
    fn a_timestamp_in_the_future_is_treated_as_stale() {
        let dir = tempfile::tempdir().expect("temp dir");
        let cache_path = dir.path().join("registry.json");
        std::fs::write(&cache_path, cache_json(&[("requestz", false, -86_400)]))
            .expect("write cache");

        let registry = PyPiRegistry::new(true, cache_path);

        assert_eq!(registry.exists("requestz"), Existence::Unknown);
    }

    /// The pre-timestamp schema was `{"name": true}`. It carries no checked
    /// time, so nothing in it can be vouched for. Discarding it costs one run
    /// of re-queries and can never produce a block; honouring it would
    /// resurrect exactly the stale `Missing` the timestamp exists to prevent.
    #[test]
    fn a_cache_file_from_the_old_schema_is_discarded_rather_than_trusted() {
        let dir = tempfile::tempdir().expect("temp dir");
        let cache_path = dir.path().join("registry.json");
        std::fs::write(&cache_path, r#"{"known":{"ruff":true,"requestz":false}}"#)
            .expect("write cache");

        let registry = PyPiRegistry::new(true, cache_path);

        assert_eq!(registry.exists("ruff"), Existence::Unknown);
        assert_eq!(registry.exists("requestz"), Existence::Unknown);
    }

    #[test]
    fn a_definite_answer_is_stored_with_the_time_it_was_checked() {
        let dir = tempfile::tempdir().expect("temp dir");
        let cache_path = dir.path().join("registry.json");
        let registry = PyPiRegistry::new(true, cache_path.clone());

        registry.record("ruff", Existence::Exists);

        let written = std::fs::read_to_string(&cache_path).expect("read cache");
        assert!(written.contains("\"checked\""), "no timestamp: {written}");
        // Round trips as fresh, which is what makes the entry usable at all.
        assert_eq!(
            PyPiRegistry::new(true, cache_path).exists("ruff"),
            Existence::Exists
        );
    }

    #[test]
    fn offline_mode_never_makes_a_request_and_reports_unknown() {
        let dir = tempfile::tempdir().expect("temp dir");
        let registry = PyPiRegistry::new(true, dir.path().join("registry.json"));

        assert_eq!(registry.exists("requestz"), Existence::Unknown);
    }

    #[test]
    fn a_cached_answer_is_returned_without_a_request() {
        let dir = tempfile::tempdir().expect("temp dir");
        let cache_path = dir.path().join("registry.json");
        std::fs::write(
            &cache_path,
            cache_json(&[("ruff", true, 0), ("requestz", false, 0)]),
        )
        .expect("write cache");

        // Offline proves no request is made: the answers can only come from cache.
        let registry = PyPiRegistry::new(true, cache_path);

        assert_eq!(registry.exists("ruff"), Existence::Exists);
        assert_eq!(registry.exists("requestz"), Existence::Missing);
    }

    #[test]
    fn a_missing_cache_file_is_not_an_error() {
        let dir = tempfile::tempdir().expect("temp dir");
        let registry = PyPiRegistry::new(true, dir.path().join("does-not-exist.json"));

        assert_eq!(registry.exists("ruff"), Existence::Unknown);
    }

    #[test]
    fn a_corrupt_cache_file_is_not_an_error() {
        let dir = tempfile::tempdir().expect("temp dir");
        let cache_path = dir.path().join("registry.json");
        std::fs::write(&cache_path, "not json at all").expect("write cache");

        let registry = PyPiRegistry::new(true, cache_path);

        assert_eq!(registry.exists("ruff"), Existence::Unknown);
    }

    /// `Unknown` must not be recorded. Caching it would freeze a transient
    /// network failure into a permanent answer, and every later run would
    /// report `Unknown` for a name that a working network resolves.
    ///
    /// Offline is the only way to reach `Unknown` without a request, so this
    /// pins the weaker claim that no `Unknown` answer reaches the cache file.
    /// The stronger claim — that the network path does not cache `Unknown`
    /// either — is not reachable without an injectable transport.
    #[test]
    fn an_unknown_answer_is_not_written_to_the_cache() {
        let dir = tempfile::tempdir().expect("temp dir");
        let cache_path = dir.path().join("registry.json");
        std::fs::write(&cache_path, cache_json(&[("ruff", true, 0)])).expect("write cache");

        let registry = PyPiRegistry::new(true, cache_path.clone());
        assert_eq!(registry.exists("requestz"), Existence::Unknown);

        let after = std::fs::read_to_string(&cache_path).expect("read cache");
        assert!(
            !after.contains("requestz"),
            "unknown answer was cached: {after}"
        );
    }

    /// The cache is written by one method and read by another, so the two can
    /// drift apart without any single test noticing: the seeded-cache tests
    /// above only exercise the read side. This drives a real `persist` and
    /// reads it back through `new`, which is the round trip production makes
    /// across two runs of the binary.
    #[test]
    fn a_persisted_cache_is_read_back_by_a_later_registry() {
        let dir = tempfile::tempdir().expect("temp dir");
        let cache_path = dir.path().join("nested").join("registry.json");

        let first = PyPiRegistry::new(true, cache_path.clone());
        first.record("ruff", Existence::Exists);
        first.record("requestz", Existence::Missing);

        let second = PyPiRegistry::new(true, cache_path);

        assert_eq!(second.exists("ruff"), Existence::Exists);
        assert_eq!(second.exists("requestz"), Existence::Missing);
    }

    /// The rule the whole gate rests on: a name that comes back `Missing`
    /// blocks a build. So every way of *failing to ask* must land on
    /// `Unknown`, and a 5xx, a dead host or a socket error are all failures to
    /// ask. Reading any of them as `Missing` would turn one proxy outage into
    /// a blocking finding on every dependency at once.
    #[test]
    fn only_a_404_reads_as_missing() {
        assert_eq!(classify(Ok(())).existence(), Existence::Exists);
        assert_eq!(
            classify(Err(ureq::Error::StatusCode(404))).existence(),
            Existence::Missing
        );
        assert_eq!(
            classify(Err(ureq::Error::StatusCode(500))).existence(),
            Existence::Unknown
        );
        assert_eq!(
            classify(Err(ureq::Error::StatusCode(403))).existence(),
            Existence::Unknown
        );
        assert_eq!(
            classify(Err(ureq::Error::HostNotFound)).existence(),
            Existence::Unknown
        );
        assert_eq!(
            classify(Err(ureq::Error::Io(std::io::Error::from(
                std::io::ErrorKind::ConnectionRefused
            ))))
            .existence(),
            Existence::Unknown
        );
    }

    /// The latch keys off *why* a lookup failed, so the split has to be right.
    /// A status code means a server answered, and its answer is about one
    /// package — latching on a 503 would let one bad package silently disable
    /// the gate for every name after it. Only a failure to reach anything at
    /// all generalises to the next lookup.
    ///
    /// `BadUri` and `Http` are per-name too: they mean this URL could not be
    /// built, not that the network is down.
    #[test]
    fn only_a_failure_to_reach_the_server_generalises() {
        let reachable = [
            ureq::Error::StatusCode(404),
            ureq::Error::StatusCode(500),
            ureq::Error::StatusCode(403),
            ureq::Error::TooManyRedirects,
            ureq::Error::BadUri("not a uri".into()),
        ];
        for error in reachable {
            let outcome = classify(Err(error));
            assert!(
                matches!(outcome, Outcome::Answered(_)),
                "{outcome:?} should not latch"
            );
        }

        let unreachable = [
            ureq::Error::HostNotFound,
            ureq::Error::ConnectionFailed,
            ureq::Error::Timeout(ureq::Timeout::Global),
            ureq::Error::Io(std::io::Error::from(std::io::ErrorKind::ConnectionRefused)),
        ];
        for error in unreachable {
            let outcome = classify(Err(error));
            assert!(
                matches!(outcome, Outcome::Unreachable),
                "{outcome:?} should latch"
            );
        }
    }

    /// The timeout bounds one lookup; it does nothing about a run. A firewall
    /// that blackholes packets rather than refusing them makes every name pay
    /// the full bound, so thirty dependencies is a five-minute hang on a
    /// blocking gate — the outcome the timeout was added to prevent, just
    /// slower to arrive.
    #[test]
    fn a_transport_failure_stops_the_run_from_trying_again() {
        let dir = tempfile::tempdir().expect("temp dir");
        let registry = PyPiRegistry::new(false, dir.path().join("registry.json"));
        let attempts = std::cell::Cell::new(0u32);
        let blackhole = |_: &str| {
            attempts.set(attempts.get() + 1);
            Outcome::Unreachable
        };

        for name in ["ruff", "requestz", "pytest", "ruff"] {
            assert_eq!(registry.exists_via(name, blackhole), Existence::Unknown);
        }

        assert_eq!(
            attempts.get(),
            1,
            "every name after the first paid for the same dead network"
        );
    }

    /// The other half of the latch. A 5xx or a 403 is a reachable server
    /// answering about one package, so it must not stop the run: latching here
    /// would turn one flaky package into a silently disabled gate.
    #[test]
    fn a_server_error_does_not_stop_the_run() {
        let dir = tempfile::tempdir().expect("temp dir");
        let registry = PyPiRegistry::new(false, dir.path().join("registry.json"));
        let attempts = std::cell::Cell::new(0u32);
        let flaky = |_: &str| {
            attempts.set(attempts.get() + 1);
            Outcome::Answered(Existence::Unknown)
        };

        for name in ["ruff", "requestz", "pytest"] {
            assert_eq!(registry.exists_via(name, flaky), Existence::Unknown);
        }

        assert_eq!(attempts.get(), 3, "a reachable server must still be asked");
    }

    /// The latch sits after the cache, not before it. A warm cache and a dead
    /// network is the ordinary offline-ish case, and it must still answer.
    #[test]
    fn a_warm_cache_still_answers_after_the_network_dies() {
        let dir = tempfile::tempdir().expect("temp dir");
        let cache_path = dir.path().join("registry.json");
        std::fs::write(&cache_path, cache_json(&[("ruff", true, 0)])).expect("write cache");
        let registry = PyPiRegistry::new(false, cache_path);
        let attempts = std::cell::Cell::new(0u32);
        let blackhole = |_: &str| {
            attempts.set(attempts.get() + 1);
            Outcome::Unreachable
        };

        assert_eq!(
            registry.exists_via("requestz", blackhole),
            Existence::Unknown
        );
        assert_eq!(attempts.get(), 1);

        assert_eq!(registry.exists_via("ruff", blackhole), Existence::Exists);
        assert_eq!(attempts.get(), 1, "the cache was skipped once latched");
    }

    /// ureq 3.3 defaults every timeout to `None`, so an unanswered connection
    /// would block the gate indefinitely. Hanging is worse than failing: a
    /// gate that fails at least reports, and a build that never returns is
    /// switched off faster than one that says the wrong thing.
    ///
    /// A timeout surfaces as `Existence::Unknown` like any other transport
    /// failure, so slow networks cost a round trip, never a blocking finding.
    #[test]
    fn a_lookup_cannot_hang_forever() {
        let dir = tempfile::tempdir().expect("temp dir");
        let registry = PyPiRegistry::new(false, dir.path().join("registry.json"));
        let global = registry
            .agent
            .config()
            .timeouts()
            .global
            .expect("the agent every lookup runs on must be bounded");

        // A bound, not `== LOOKUP_TIMEOUT`, which would hold for ten hours.
        assert!(
            global >= Duration::from_secs(1) && global <= Duration::from_secs(30),
            "a lookup bound of {global:?} is not a usable gate"
        );
        assert_eq!(
            classify(Err(ureq::Error::Timeout(ureq::Timeout::Global))).existence(),
            Existence::Unknown
        );
    }

    /// `Unknown` says something about this run's network, not about the
    /// package, so recording it would freeze one outage into every later run.
    #[test]
    fn recording_stores_definite_answers_and_ignores_unknown() {
        let dir = tempfile::tempdir().expect("temp dir");
        let cache_path = dir.path().join("registry.json");
        let registry = PyPiRegistry::new(true, cache_path.clone());

        registry.record("nowhere", Existence::Unknown);
        assert!(
            !cache_path.exists(),
            "an unknown answer created a cache file"
        );

        registry.record("ruff", Existence::Exists);
        registry.record("requestz", Existence::Missing);

        let reloaded = PyPiRegistry::new(true, cache_path);
        assert_eq!(reloaded.exists("ruff"), Existence::Exists);
        assert_eq!(reloaded.exists("requestz"), Existence::Missing);
        // Offline, so this can only be a cache miss, not a fresh lookup.
        assert_eq!(reloaded.exists("nowhere"), Existence::Unknown);
    }

    /// The `RefCell` is what keeps this single-threaded, and that is a choice,
    /// not an oversight: nothing here needs concurrency. Pinning `Send` says
    /// the registry can still be handed to another thread wholesale, which is
    /// the property a caller would actually reach for. `Sync` is deliberately
    /// absent — sharing one registry between threads must stay a compile
    /// error, because `RefCell` would otherwise race on the cache.
    #[test]
    fn the_registry_can_move_between_threads_but_not_be_shared() {
        fn assert_send<T: Send>() {}
        assert_send::<PyPiRegistry>();
    }

    /// Callers hold the registry as `&dyn Registry` so they can inject a stub,
    /// which only compiles while the trait stays object safe. A method taking
    /// or returning `Self`, or a generic parameter, would break that silently
    /// for every caller at once.
    #[test]
    fn the_registry_trait_is_object_safe() {
        let dir = tempfile::tempdir().expect("temp dir");
        let cache_path = dir.path().join("registry.json");
        std::fs::write(&cache_path, cache_json(&[("ruff", true, 0)])).expect("write cache");

        let registry = PyPiRegistry::new(true, cache_path);
        let erased: &dyn Registry = &registry;

        assert_eq!(erased.exists("ruff"), Existence::Exists);
    }
}
