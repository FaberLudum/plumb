// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Plumb Authors

//! Resolving distribution names against PyPI.
//!
//! This is the only part of the binary that touches the network, and it makes
//! exactly one class of request: a lookup of one distribution name. Nothing
//! else is ever sent.

use serde::{Deserialize, Serialize};
use std::cell::RefCell;
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
/// rather than breaking a build. Not unbounded, though — PyPI does remove
/// projects, and removing malicious typosquats is one of the main reasons it
/// does, so a name must eventually be asked about again.
const EXISTS_TTL: Duration = Duration::from_secs(30 * 24 * 60 * 60);

/// How long one lookup may take before it reports [`Existence::Unknown`].
///
/// ureq applies no timeout to a bodyless GET by default — global, resolve and
/// connect are all unset — so this is the only thing bounding a lookup. It
/// covers the whole call, resolution and connection included.
const LOOKUP_TIMEOUT: Duration = Duration::from_secs(10);

/// Whether a package could be found in its registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code, reason = "wired into the gate in a later task")]
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
#[allow(dead_code, reason = "wired into the gate in a later task")]
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
/// Single-threaded by construction: the [`RefCell`] makes this `!Sync`, so
/// sharing one registry across threads is a compile error rather than a race.
/// It is `Send`, so a whole registry can still be moved to another thread.
/// Nothing here assumes concurrency, and lookups run one at a time.
#[allow(dead_code, reason = "wired into the gate in a later task")]
pub struct PyPiRegistry {
    offline: bool,
    cache_path: PathBuf,
    cache: RefCell<Cache>,
    /// Held rather than built per lookup, so a run of N names reuses one
    /// connection pool and one TLS handshake instead of opening N.
    agent: ureq::Agent,
}

#[allow(dead_code, reason = "wired into the gate in a later task")]
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
        }
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

/// Turns one lookup result into an outcome.
///
/// Only a success and a 404 say anything about the package. Everything else —
/// a 5xx, a 403 from a proxy, a dead host, a socket error, a timeout — is a
/// failure to ask, and reports [`Existence::Unknown`].
///
/// `Ok` is any non-error status ureq settles on, redirects already followed,
/// since it treats 4xx and 5xx as errors by default.
///
/// Takes `Result<(), Error>` rather than the response so the mapping can be
/// tested without a request; the body is never read either way.
fn classify(result: Result<(), ureq::Error>) -> Existence {
    match result {
        Ok(()) => Existence::Exists,
        Err(ureq::Error::StatusCode(404)) => Existence::Missing,
        Err(_) => Existence::Unknown,
    }
}

impl Registry for PyPiRegistry {
    /// Answers from the cache, then from PyPI, then not at all.
    ///
    /// Anything that is not a definite 200 or 404 — a timeout, a proxy, a
    /// firewall, a 5xx — is [`Existence::Unknown`], and `Unknown` is never
    /// cached: it is a fact about this run's network, not about the package.
    fn exists(&self, name: &str) -> Existence {
        if let Some(entry) = self.cache.borrow().known.get(name).copied()
            && entry.is_fresh(now_unix())
        {
            return entry.existence();
        }

        if self.offline {
            return Existence::Unknown;
        }

        let url = format!("https://pypi.org/pypi/{name}/json");
        let outcome = classify(self.agent.get(&url).call().map(|_| ()));

        self.record(name, outcome);
        outcome
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

        // Offline, so an expired entry cannot be re-queried and the only
        // honest answer is Unknown -- which never blocks.
        let registry = PyPiRegistry::new(true, cache_path);

        assert_eq!(registry.exists("requestz"), Existence::Unknown);
    }

    #[test]
    fn an_expired_exists_is_not_returned_either() {
        let dir = tempfile::tempdir().expect("temp dir");
        let cache_path = dir.path().join("registry.json");
        let long_ago = 120 * 24 * 60 * 60;
        std::fs::write(&cache_path, cache_json(&[("ruff", true, long_ago)])).expect("write cache");

        let registry = PyPiRegistry::new(true, cache_path);

        assert_eq!(registry.exists("ruff"), Existence::Unknown);
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
        assert_eq!(classify(Ok(())), Existence::Exists);
        assert_eq!(
            classify(Err(ureq::Error::StatusCode(404))),
            Existence::Missing
        );
        assert_eq!(
            classify(Err(ureq::Error::StatusCode(500))),
            Existence::Unknown
        );
        assert_eq!(
            classify(Err(ureq::Error::StatusCode(403))),
            Existence::Unknown
        );
        assert_eq!(classify(Err(ureq::Error::HostNotFound)), Existence::Unknown);
        assert_eq!(
            classify(Err(ureq::Error::Io(std::io::Error::from(
                std::io::ErrorKind::ConnectionRefused
            )))),
            Existence::Unknown
        );
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
            classify(Err(ureq::Error::Timeout(ureq::Timeout::Global))),
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
