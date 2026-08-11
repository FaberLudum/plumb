// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Plumb Authors

//! Resolving distribution names against PyPI.
//!
//! This is the only part of the binary that touches the network, and it makes
//! exactly one class of request: a lookup of one distribution name. Nothing
//! else is ever sent.

use plumb_core::manifest::Ecosystem;
use serde::{Deserialize, Serialize};
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Environment variable naming the cache file outright.
///
/// Two uses, and both are real. A CI job that wants the cache to survive
/// between runs has to be able to say where it is; and the end-to-end tests
/// have to point the binary somewhere isolated, because they must never reach
/// the network and must never touch the developer's own cache.
///
/// It is not a hole in what [`cache_path`] fixes. That finding was about a
/// file **committed into the repository**, which arrives with the change under
/// inspection and needs nobody's consent. An environment variable is set by
/// whoever runs the gate, which is the same person who could pass `--offline`
/// or not run it at all.
pub const CACHE_FILE_ENV: &str = "PLUMB_CACHE_FILE";

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
/// what [`PyPiRegistry::stale_packages`] is for.
const EXISTS_TTL: Duration = Duration::from_secs(30 * 24 * 60 * 60);

/// How long one lookup may take before it reports [`Existence::Unknown`].
///
/// ureq applies no timeout to a bodyless GET by default — global, resolve and
/// connect are all unset — so this is the only thing bounding a lookup. It
/// covers the whole call, resolution and connection included.
const LOOKUP_TIMEOUT: Duration = Duration::from_secs(10);

/// Where the registry cache for `repo` lives.
///
/// **Outside the repository, always.** The cache is an unauthenticated trust
/// store: [`PyPiRegistry::new`] reads it with no provenance check, and a fresh
/// entry short-circuits the network before any lookup happens. Keeping it at
/// `<repo>/.plumb/cache/registry.json` put that store in the writable,
/// committable area of whoever proposes a change, so one committed file turned
/// the blocking gate off for [`EXISTS_TTL`] — thirty days. The rule this tool
/// enforces is cited to an attack paper; a trust store the attacker can write
/// is the wrong shape for it.
///
/// Refusing a *tracked* cache file would have been the narrower fix, and it
/// was rejected: it leaves the store in the repository, so it still has to be
/// gitignored by every consumer, still travels in a tarball, and still answers
/// for the gate when it is merely untracked-and-planted rather than committed.
/// Moving it removes the class instead of one member of it.
///
/// Keyed per repository rather than shared. The data — does this name resolve
/// on PyPI — is global truth today, so a single shared file would be warmer
/// and this is a real cost. It is paid for two reasons: concurrent runs in
/// different repositories cannot lose each other's entries, and an `Exists` is
/// only global while there is one index. The README's own false-positive list
/// already names packages from a private index, so a per-repository registry
/// is a change this project expects to make, and a cache keyed today does not
/// have to be invalidated when it does.
///
/// `None` means this run must not cache at all — see [`cache_root`].
pub fn cache_path(repo: &Path) -> Option<PathBuf> {
    match std::env::var_os(CACHE_FILE_ENV) {
        Some(explicit) if !explicit.is_empty() => Some(PathBuf::from(explicit)),
        _ => default_cache_path(repo),
    }
}

/// [`cache_path`] with the environment override skipped.
///
/// Split out so the tests can exercise the resolution without mutating process
/// environment, which is shared with every other test in the binary and is
/// `unsafe` in edition 2024 for exactly that reason.
fn default_cache_path(repo: &Path) -> Option<PathBuf> {
    Some(
        cache_root()?
            .join("plumb")
            .join("registry")
            .join(format!("{}.json", cache_key(repo))),
    )
}

/// The OS directory for cached data this machine may delete at will.
///
/// `%LOCALAPPDATA%` on Windows, `$XDG_CACHE_HOME` then `~/.cache` elsewhere.
/// Resolved from the environment rather than through a crate: `dirs` would
/// need a row in `docs/dependencies.md` claiming the standard library is not
/// enough, and for these three variables it plainly is.
///
/// Absolute paths only. A relative `XDG_CACHE_HOME` is undefined per the
/// specification and would resolve against the working directory, which for
/// this binary is somewhere inside the repository — the one place the cache
/// must not be.
///
/// `None` when none of them is set, and **there is deliberately no fallback to
/// the temporary directory**. That fallback was here and was wrong: on a
/// container or CI runner with no `HOME` — which is real, not hypothetical —
/// it puts an unauthenticated trust store at a fully predictable path in a
/// world-writable directory, where any other local user can pre-create it with
/// `exists: true` entries and disable the gate for thirty days. That is the
/// same shape as the in-repository cache this move exists to remove, reached
/// through a different door.
///
/// A cold cache costs a round trip per name. A poisoned one costs the gate. So
/// when no trusted directory can be named, this run simply does not cache;
/// [`CACHE_FILE_ENV`] is there for anyone who wants to name one.
fn cache_root() -> Option<PathBuf> {
    if cfg!(windows) {
        absolute_env("LOCALAPPDATA")
    } else {
        absolute_env("XDG_CACHE_HOME")
            .or_else(|| absolute_env("HOME").map(|home| home.join(".cache")))
    }
}

/// An environment variable read as an absolute path, or nothing.
fn absolute_env(name: &str) -> Option<PathBuf> {
    let value = PathBuf::from(std::env::var_os(name)?);
    value.is_absolute().then_some(value)
}

/// A filename standing for one repository.
///
/// The directory name is kept in front so the cache directory can be read by a
/// human deciding what to delete; the digest is what actually distinguishes
/// two checkouts of the same project.
///
/// The input is the root as `git rev-parse --show-toplevel` spells it, which
/// is stable across runs and across the subdirectory the gate is invoked from.
/// If it ever were not — a differently spelled but equivalent path — the cost
/// is a cold cache and one round trip, never a wrong answer, because a cache
/// miss can only produce `Unknown` and `Unknown` never blocks.
fn cache_key(repo: &Path) -> String {
    let digest = fnv1a_64(repo.to_string_lossy().as_bytes());
    let label: String = repo
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .map(|c| c.to_ascii_lowercase())
        .take(32)
        .collect();

    if label.is_empty() {
        format!("{digest:016x}")
    } else {
        format!("{label}-{digest:016x}")
    }
}

/// FNV-1a, 64-bit.
///
/// Written out rather than taken from `std::hash::DefaultHasher`, whose output
/// is explicitly not guaranteed stable across Rust releases — a compiler
/// upgrade would silently rename every cache file. Nothing here is a security
/// property: this only has to separate two directory names, so a short,
/// specified, reproducible function is exactly the right amount of machinery.
fn fnv1a_64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Where the cache used to live, inside the repository.
///
/// Kept only so a run can tell the user the file is there and is no longer
/// read. It is deliberately never opened: migrating it would import exactly
/// the unverifiable, possibly-committed entries the move exists to refuse, and
/// a migration that launders a poisoned cache into the trusted store is worse
/// than no migration. Ignoring it silently was the other option, and it leaves
/// a stale file in every existing checkout with nothing said about it.
pub fn legacy_cache_path(repo: &Path) -> PathBuf {
    repo.join(".plumb").join("cache").join("registry.json")
}

/// Whether a package could be found in its registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Existence {
    Exists,
    Missing,
    /// The registry could not be consulted. Never a violation.
    Unknown,
}

/// A source of truth for whether a package name resolves in a given
/// ecosystem.
///
/// Object safe on purpose: `check.rs` holds `&dyn Registry` for the whole run
/// so tests can inject a stub instead of reaching the network.
///
/// Only [`RegistryRouter`] implements this trait. [`PyPiRegistry`] and
/// [`NpmRegistry`] each expose an ecosystem-less inherent `exists(name)`
/// instead: each already knows which registry it asks, so the parameter
/// would be dead weight on both of them. The router is the one thing that
/// needs it, to decide which of the two to ask — see its doc comment.
pub trait Registry {
    fn exists(&self, name: &str, ecosystem: Ecosystem) -> Existence;
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

/// Remembered answers, keyed by [`pypi_cache_key`] or [`npm_cache_key`].
///
/// One map holds both ecosystems' answers, because [`PyPiRegistry`] and
/// [`NpmRegistry`] share one on-disk file — see [`persist_entry`] for how
/// they write to it without erasing each other's entries.
///
/// The pre-timestamp schema stored a bare bool. It does not deserialize into
/// this, so an old file is discarded and rebuilt — see [`PyPiRegistry::new`].
#[derive(Debug, Default, Serialize, Deserialize)]
struct Cache {
    known: BTreeMap<String, Entry>,
}

/// The map key an answer about `name` is stored under for PyPI.
///
/// Prefixed so one cache file can hold npm's answers too without either
/// ecosystem's names colliding with the other's — see [`npm_cache_key`]. A
/// bare key with no prefix is what every entry looked like before npm
/// existed, so [`PyPiRegistry::exists_via`] still reads one when a prefixed
/// entry is not there: a cache file from before this change stays valid
/// without anyone migrating it. New answers are written only under the
/// prefixed form, so the legacy shape never spreads to new entries.
fn pypi_cache_key(name: &str) -> String {
    format!("pypi:{name}")
}

/// The map key an answer about `name` is stored under for npm.
///
/// No legacy bare form: a bare key predates npm's existence entirely, so it
/// can only ever have meant PyPI — see [`pypi_cache_key`].
fn npm_cache_key(name: &str) -> String {
    format!("npm:{name}")
}

/// Reads the cache at `cache_path`, if there is a readable one.
///
/// Shared by [`PyPiRegistry::new`], [`NpmRegistry::new`] and
/// [`persist_entry`]'s read-modify-write. A missing, unreadable or malformed
/// file all read as empty rather than an error — the cache is an
/// optimisation, and refusing to start over a corrupt one would turn a
/// scratch file into a hard failure of the gate. A file written by the
/// pre-timestamp schema takes the same path: it fails to deserialize, so it
/// is discarded and rebuilt rather than trusted with no way to tell how old
/// its answers are.
fn load_cache(cache_path: Option<&Path>) -> Cache {
    cache_path
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str::<Cache>(&text).ok())
        .unwrap_or_default()
}

/// Merges one answer into the cache file on disk.
///
/// Read-modify-write rather than a blind overwrite of a whole in-memory
/// snapshot, because [`PyPiRegistry`] and [`NpmRegistry`] write to the same
/// file. A snapshot taken at construction and written back wholesale would
/// let whichever registry persists second erase whatever the other
/// persisted in between, within the very same run — a Python and an npm
/// manifest both changing is the ordinary case this has to survive, not an
/// edge case. Reading fresh immediately before writing one entry keeps that
/// window as small as it can be without adding a second process's worth of
/// locking to a single-threaded, single-process CLI run.
///
/// A failed write costs the next run a round trip and nothing else, so there
/// is no outcome worth interrupting the gate for.
fn persist_entry(cache_path: &Path, key: &str, entry: Entry) {
    let mut cache = load_cache(Some(cache_path));
    cache.known.insert(key.to_string(), entry);
    let Ok(text) = serde_json::to_string(&cache) else {
        return;
    };
    if let Some(parent) = cache_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(cache_path, text);
}

/// What an expired entry is worth when the registry will not be consulted.
///
/// Shared by [`PyPiRegistry::exists_via`] and [`NpmRegistry::exists_via`]:
/// the asymmetry is about what `Exists` and `Missing` cost, not about either
/// registry, so there is nothing ecosystem-specific left to say here that
/// [`PyPiRegistry`]'s own doc comment does not already say in full — offline
/// there is no way to refresh, so the choice is between the old answer and
/// no answer, and a stale `Exists` only fails open while a stale `Missing`
/// would block a build on evidence nobody can check.
fn honour_offline(cached: Option<Entry>) -> Existence {
    match cached {
        Some(entry) if entry.exists => Existence::Exists,
        _ => Existence::Unknown,
    }
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
    /// `None` when no trusted cache location could be named, in which case
    /// this run answers from the network alone — see [`cache_root`].
    cache_path: Option<PathBuf>,
    cache: RefCell<Cache>,
    /// Held rather than built per lookup, so a run of N names reuses one
    /// connection pool and one TLS handshake instead of opening N.
    agent: ureq::Agent,
    /// Set once the network proves unreachable, and never cleared.
    ///
    /// In memory only. It describes this process's network for the length of
    /// this run and must not outlive it, so it is deliberately not persisted.
    unreachable: Cell<bool>,
    /// The names this run answered from entries past their lifetime.
    ///
    /// Names rather than a count, because the same package named in two
    /// manifests is asked about twice and is still one package whose answer
    /// could not be confirmed. Collected rather than flagged so the report can
    /// say how much of the run rested on old data. Like the latch, this
    /// describes this run and is deliberately not persisted.
    honoured_stale: RefCell<BTreeSet<String>>,
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
    /// `cache_path` is `None` when no trusted location could be named. Such a
    /// run keeps its answers in memory only: every lookup still happens, and
    /// nothing is read from or written to disk.
    pub fn new(offline: bool, cache_path: Option<PathBuf>) -> Self {
        let cache = load_cache(cache_path.as_deref());

        Self {
            offline,
            cache_path,
            cache: RefCell::new(cache),
            agent: lookup_agent(),
            unreachable: Cell::new(false),
            honoured_stale: RefCell::new(BTreeSet::new()),
        }
    }

    /// How many distinct packages this run answered from cache entries past
    /// their lifetime.
    ///
    /// Distinct packages, not lookups: a name declared in two manifests is
    /// asked about twice, and the reader is being told how much of their
    /// dependency set went unconfirmed, not how many times this process
    /// consulted a map.
    ///
    /// Only the offline path can produce one: online an expired entry is
    /// re-queried, and a latched run refuses it rather than honouring it. See
    /// [`honour_offline`] for why offline honours it at all, and
    /// [`EXISTS_TTL`] for why that concession needs saying out loud.
    ///
    /// Read once after a run rather than pushed through [`Registry`]: the
    /// trait has to stay trivially stubbable, and a method a stub must answer
    /// meaningfully is a method every stub gets wrong.
    pub fn stale_packages(&self) -> usize {
        self.honoured_stale.borrow().len()
    }

    /// Answers from the cache, then from `lookup`, then not at all.
    ///
    /// The lookup is a parameter so the request itself can be replaced in
    /// tests. [`Registry::exists`] passes the real one.
    ///
    /// Order matters. A fresh cache entry answers first, so a warm cache still
    /// answers on a dead network. Offline stops there, keeping whatever an
    /// expired entry is still worth — see [`honour_offline`].
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
        // The prefixed key first, then the legacy bare one — a file written
        // before npm existed has no prefix at all, and every entry in it was
        // a PyPI answer, so it still answers here rather than being silently
        // discarded the day this key format changed. The bare-key fallback
        // is gated on `name` itself carrying no `:`: a prefixed key can
        // never be a legitimate legacy bare name (legacy files predate
        // prefixes entirely), so a queried name that contains one must not
        // be allowed to match another ecosystem's namespace by accident —
        // `plumb-core` happens to keep `:` out of every name it extracts
        // today, but that is an invariant of a different crate, asserted
        // nowhere in this one, and this guard must not rest on it.
        let cached = {
            let cache = self.cache.borrow();
            cache
                .known
                .get(&pypi_cache_key(name))
                .or_else(|| {
                    if name.contains(':') {
                        None
                    } else {
                        cache.known.get(name)
                    }
                })
                .copied()
        };
        if let Some(entry) = cached
            && entry.is_fresh(now_unix())
        {
            return entry.existence();
        }

        if self.offline {
            let answer = honour_offline(cached);
            // Reaching here means the entry was not fresh, so any answer at
            // all is an answer taken from data past its lifetime. Counting the
            // `Unknown`s too would report staleness for a cache miss, which is
            // an absence rather than an old answer.
            if answer != Existence::Unknown {
                self.honoured_stale.borrow_mut().insert(name.to_string());
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

    /// Answers from the cache, then from PyPI, then not at all.
    ///
    /// Anything that is not a definite success or 404 — a timeout, a proxy, a
    /// firewall, a 5xx — is [`Existence::Unknown`], and `Unknown` is never
    /// cached: it is a fact about this run's network, not about the package.
    ///
    /// See [`PyPiRegistry::exists_via`] for the order this resolves in, and
    /// for the latch that stops a dead network being retried once per name.
    pub fn exists(&self, name: &str) -> Existence {
        self.exists_via(name, |name| self.lookup(name))
    }

    /// Asks PyPI about one name.
    fn lookup(&self, name: &str) -> Outcome {
        // Task 4 admits only ASCII alphanumerics and `-` into a name, anchored
        // alphanumeric at both ends, so the name cannot alter the URL's shape.
        // Asserting it here keeps that guarantee local to the code that builds
        // the URL, rather than resting on a crate this file never references.
        // Scoped to PyPI's own charset: an npm name may carry `@`, `/`, `_`
        // and `.`, none of which this assertion admits, so it must never run
        // against one — see [`is_valid_npm_name`] for npm's own validator.
        debug_assert!(
            is_valid_pypi_name(name),
            "name reached the registry unvalidated: {name:?}"
        );
        let url = format!("https://pypi.org/pypi/{name}/json");
        classify(self.agent.get(&url).call().map(|_| ()))
    }

    /// Records a definite answer and writes it into the shared cache file.
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
        let key = pypi_cache_key(name);
        self.cache.borrow_mut().known.insert(key.clone(), entry);
        if let Some(cache_path) = self.cache_path.as_deref() {
            persist_entry(cache_path, &key, entry);
        }
    }
}

/// Whether `name` is safe to interpolate into a PyPI lookup URL unescaped.
///
/// ASCII alphanumeric and `-` only, anchored alphanumeric at both ends — PEP
/// 503's own character set after normalisation. A pure predicate rather than
/// inline in [`PyPiRegistry::lookup`] so the character set can be pinned
/// without a request, the same reasoning [`npm_url`] and
/// [`is_valid_npm_name`] follow for npm.
fn is_valid_pypi_name(name: &str) -> bool {
    name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
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

/// Builds the npm registry URL for `name`.
///
/// A scoped name's `/` is encoded as `%2F` — npm's own documented way to
/// address `@scope/name` as a single path segment rather than two — and
/// nothing else is touched, so an unscoped name passes through unchanged.
/// Pure and separate from [`NpmRegistry::lookup`] so the mapping can be
/// pinned without a request; the endpoint shape itself was confirmed with a
/// manual `curl` outside the test suite, quoted in this task's report.
fn npm_url(name: &str) -> String {
    format!("https://registry.npmjs.org/{}", name.replace('/', "%2F"))
}

/// Whether `name` is a syntactically ordinary npm package name.
///
/// ASCII alphanumerics, in either case, plus `@`, `/`, `-`, `_` and `.` —
/// npm's real, documented name charset, wider than PyPI's and including `/`
/// in a scoped name's unencoded form (see [`npm_url`] for how that `/`
/// reaches the wire). [`is_valid_pypi_name`]'s narrower rule must never run
/// against one of these; this is the ecosystem-specific validator that runs
/// instead.
///
/// Both cases, not lowercase only. npm still serves legal packages
/// published before it required a lowercase name —
/// `registry.npmjs.org/JSONStream` answers 200 today — and a validator that
/// admitted only lowercase refused a name npm itself accepts. That was a
/// real bug in an earlier version of this function: it tripped
/// [`NpmRegistry::lookup`]'s `debug_assert` in a debug build on every such
/// name, taking the gate down with a panic (exit 101, outside the exit
/// contract) over a package that had done nothing wrong.
///
/// This is also the gate [`NpmRegistry::exists`] checks before it does
/// anything else, so once a name has passed it, every character remaining
/// is already a safe, literal URL path-segment character — [`npm_url`]
/// only ever has to encode the `/` of a scoped name, never anything this
/// function would have rejected.
fn is_valid_npm_name(name: &str) -> bool {
    name.chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '@' | '/'))
}

/// Resolves names against npm, with the same on-disk cache, offline mode and
/// unreachable-network latch as [`PyPiRegistry`] — see that type's doc
/// comment and its fields' comments for the reasoning behind all of them,
/// none of which differs here. What differs is narrow and is all this type
/// states of its own: the endpoint ([`npm_url`]), the cache key prefix
/// ([`npm_cache_key`], with no legacy bare-key fallback — a bare key predates
/// npm entirely), and the character set a name must satisfy before it
/// reaches the URL ([`is_valid_npm_name`]).
///
/// A second struct rather than a shared generic one, on the plan's own
/// instruction: a little duplication beats a premature abstraction, and the
/// third ecosystem is what should pay for factoring this out, not the
/// second.
pub struct NpmRegistry {
    offline: bool,
    cache_path: Option<PathBuf>,
    cache: RefCell<Cache>,
    agent: ureq::Agent,
    unreachable: Cell<bool>,
    honoured_stale: RefCell<BTreeSet<String>>,
}

impl NpmRegistry {
    /// See [`PyPiRegistry::new`]; identical, against the shared cache file.
    pub fn new(offline: bool, cache_path: Option<PathBuf>) -> Self {
        let cache = load_cache(cache_path.as_deref());

        Self {
            offline,
            cache_path,
            cache: RefCell::new(cache),
            agent: lookup_agent(),
            unreachable: Cell::new(false),
            honoured_stale: RefCell::new(BTreeSet::new()),
        }
    }

    /// See [`PyPiRegistry::stale_packages`].
    pub fn stale_packages(&self) -> usize {
        self.honoured_stale.borrow().len()
    }

    /// See [`PyPiRegistry::exists_via`]; identical but for the cache key,
    /// which has no legacy bare-key fallback — see [`npm_cache_key`].
    fn exists_via(&self, name: &str, lookup: impl Fn(&str) -> Outcome) -> Existence {
        let cached = self.cache.borrow().known.get(&npm_cache_key(name)).copied();
        if let Some(entry) = cached
            && entry.is_fresh(now_unix())
        {
            return entry.existence();
        }

        if self.offline {
            let answer = honour_offline(cached);
            if answer != Existence::Unknown {
                self.honoured_stale.borrow_mut().insert(name.to_string());
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

        let existence = outcome.existence();
        self.record(name, existence);
        existence
    }

    /// Answers from the cache, then from npm, then not at all. See
    /// [`PyPiRegistry::exists`].
    ///
    /// A name [`is_valid_npm_name`] refuses is answered `Missing`
    /// immediately — no cache read, no offline fallback, no request — and
    /// that runs first, ahead of everything [`exists_via`](Self::exists_via)
    /// does, so it applies the same online or offline. A name npm's own
    /// naming rules do not admit cannot be a name npm serves: that is an
    /// objective, syntactic fact about the input, established from the name
    /// alone, not a guess about the network, which is what keeps blocking on
    /// it Tier A-safe.
    ///
    /// This is the fix for the release-build half of this task's finding.
    /// [`npm_url`] escapes only a scoped name's `/`; the code this replaced
    /// built the lookup URL from a name that had never been checked at all
    /// in release, where `debug_assert` compiles out. A name carrying a URL
    /// metacharacter — `react?bogus`, live-demonstrated by the reviewer —
    /// reached npm as `registry.npmjs.org/react?bogus`, which npm's router
    /// reads as `/react` plus a query string it ignores: a 200 for a
    /// package that does not exist, no finding, exit 0, and
    /// `npm:react?bogus` cached `exists:true` for up to [`EXISTS_TTL`].
    /// Rejecting the name here, before any of that, closes the false
    /// negative and the cache poisoning in the same place the charset is
    /// already checked for [`NpmRegistry::lookup`]'s `debug_assert`.
    pub fn exists(&self, name: &str) -> Existence {
        if !is_valid_npm_name(name) {
            return Existence::Missing;
        }
        self.exists_via(name, |name| self.lookup(name))
    }

    /// Asks npm about one name.
    fn lookup(&self, name: &str) -> Outcome {
        // `NpmRegistry::exists` already refuses a name this check would
        // reject, before `lookup` is ever reached, so this can no longer
        // fire in ordinary operation. Kept anyway as a debug-only tripwire
        // consistent with that same, corrected charset: a future caller of
        // `lookup` that bypasses `exists` — a refactor, a new code path —
        // trips it immediately in a debug build instead of silently sending
        // an unchecked name to the wire. Scoped to npm's own, wider charset
        // — see `is_valid_npm_name` — so a name PyPI's narrower rule would
        // refuse is not, by itself, evidence of a bug here.
        debug_assert!(
            is_valid_npm_name(name),
            "npm name reached the registry unvalidated: {name:?}"
        );
        classify(self.agent.get(&npm_url(name)).call().map(|_| ()))
    }

    /// Records a definite answer and writes it into the shared cache file.
    /// See [`PyPiRegistry::record`].
    fn record(&self, name: &str, outcome: Existence) {
        if outcome == Existence::Unknown {
            return;
        }
        let entry = Entry {
            exists: outcome == Existence::Exists,
            checked: now_unix(),
        };
        let key = npm_cache_key(name);
        self.cache.borrow_mut().known.insert(key.clone(), entry);
        if let Some(cache_path) = self.cache_path.as_deref() {
            persist_entry(cache_path, &key, entry);
        }
    }
}

/// The map key an answer about `name` is stored under for NuGet.
///
/// NuGet package IDs are case-insensitive, so the key lowercases `name`
/// first: `"Newtonsoft.Json"` and `"newtonsoft.json"` must share the one
/// cache entry rather than silently duplicating it under two keys that
/// never see each other. No legacy bare form, for the same reason as
/// [`npm_cache_key`]: a bare key predates NuGet's existence entirely.
fn nuget_cache_key(name: &str) -> String {
    format!("nuget:{}", name.to_ascii_lowercase())
}

/// Builds the nuget.org flat-container lookup URL for `name`.
///
/// NuGet package IDs are case-insensitive and the flat-container index is
/// served at the lowercased path only, so `name` is lowercased before it is
/// interpolated. Pure and separate from [`NuGetRegistry::lookup`] so the
/// mapping can be pinned without a request, the same reasoning [`npm_url`]
/// follows for npm.
///
/// `v3-flatcontainer`, not `v3-flat2`. The design spec and this file both
/// carried `v3-flat2` from Task 2 through the whole of Task 3's automated
/// test suite unnoticed, because every test that exercises `nuget_url`
/// pins the *string this function builds*, never a real response — the
/// stub transport in `exists_via`'s own tests never touches the network
/// either. Nothing caught it until this task's live smoke test ran the
/// built binary against nuget.org for real: `Newtonsoft.Json`, a package
/// that has shipped for over a decade, came back 404 and blocked the
/// build, in the same run that correctly blocked a genuinely nonexistent
/// name — because *every* lookup 404s against the wrong path, real or
/// hallucinated alike. Verified directly with `curl` outside the test
/// suite: `v3-flat2/newtonsoft.json/index.json` is a 404
/// `ResourceNotFound`; `v3-flatcontainer/newtonsoft.json/index.json` is a
/// 200 listing its published versions. This is exactly the failure mode
/// `CLAUDE.md`'s "check the current API before writing code against it"
/// rule exists for — a URL path recalled from memory rather than looked
/// up, and a mocked test suite is structurally unable to catch it because
/// the mock and the code under test share the same wrong assumption.
fn nuget_url(name: &str) -> String {
    format!(
        "https://api.nuget.org/v3-flatcontainer/{}/index.json",
        name.to_ascii_lowercase()
    )
}

/// Whether `name` is a syntactically ordinary NuGet package ID.
///
/// Non-empty, ASCII alphanumerics plus `.`, `_` and `-` only — narrower than
/// [`is_valid_npm_name`] (no `@` or `/`), and unlike [`is_valid_pypi_name`]
/// it explicitly rejects the empty string: the flat-container URL for an
/// empty id collapses to `.../v3-flatcontainer//index.json`, a different,
/// well-formed request nobody asked for, not a lookup for "no package".
///
/// The first and last character must also be ASCII alphanumeric. A charset
/// check alone is not enough here: `.` is otherwise an ordinary character,
/// so it admitted `.` and `..` whole — RFC 3986 dot-segments — and
/// `nuget_url("..")` emitted `.../v3-flatcontainer/../index.json`. The pinned http
/// client does not normalise a dot-segment out of the path client-side, so
/// that request reached the wire as written and server-side normalisation
/// alone decided what answered, with the result cached under `nuget:..` as
/// a Tier A verdict regardless of what it was. Anchoring both ends on
/// alphanumeric kills `.` and `..` structurally — neither has an
/// alphanumeric character at all — along with the wider family of
/// leading/trailing `.` or `-`, which matches nuget.org's real id shape:
/// every id it actually serves starts and ends alphanumeric.
///
/// This is the gate [`NuGetRegistry::exists`] checks before it does
/// anything else, exactly as [`is_valid_npm_name`] gates
/// [`NpmRegistry::exists`]: an illegal id interpolated into the lookup URL
/// unescaped could resolve to something else entirely and poison the cache
/// under a name nobody searched for — the same class of bug
/// [`is_valid_npm_name`]'s own doc comment describes for npm, and the same
/// class the dot-segment case above turned out to be one member of.
fn is_valid_nuget_id(name: &str) -> bool {
    let charset_ok = name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    let ends_alphanumeric = name
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphanumeric())
        && name
            .chars()
            .next_back()
            .is_some_and(|c| c.is_ascii_alphanumeric());
    charset_ok && ends_alphanumeric
}

/// Resolves package ids against nuget.org's flat-container index, with the
/// same on-disk cache, offline mode and unreachable-network latch as
/// [`PyPiRegistry`] and [`NpmRegistry`] — see [`PyPiRegistry`]'s doc comment
/// for the reasoning behind all of them, none of which differs here. What
/// differs is narrow and is all this type states of its own: the endpoint
/// ([`nuget_url`]), the cache key prefix ([`nuget_cache_key`], lowercased
/// for NuGet's case-insensitive ids and with no legacy bare-key fallback),
/// and the character set an id must satisfy before it reaches the URL
/// ([`is_valid_nuget_id`]).
pub struct NuGetRegistry {
    offline: bool,
    cache_path: Option<PathBuf>,
    cache: RefCell<Cache>,
    agent: ureq::Agent,
    unreachable: Cell<bool>,
    honoured_stale: RefCell<BTreeSet<String>>,
}

impl NuGetRegistry {
    /// See [`PyPiRegistry::new`]; identical, against the shared cache file.
    pub fn new(offline: bool, cache_path: Option<PathBuf>) -> Self {
        let cache = load_cache(cache_path.as_deref());

        Self {
            offline,
            cache_path,
            cache: RefCell::new(cache),
            agent: lookup_agent(),
            unreachable: Cell::new(false),
            honoured_stale: RefCell::new(BTreeSet::new()),
        }
    }

    /// See [`PyPiRegistry::stale_packages`].
    pub fn stale_packages(&self) -> usize {
        self.honoured_stale.borrow().len()
    }

    /// See [`PyPiRegistry::exists_via`]; identical but for the cache key,
    /// which lowercases `name` and has no legacy bare-key fallback — see
    /// [`nuget_cache_key`].
    fn exists_via(&self, name: &str, lookup: impl Fn(&str) -> Outcome) -> Existence {
        let cached = self
            .cache
            .borrow()
            .known
            .get(&nuget_cache_key(name))
            .copied();
        if let Some(entry) = cached
            && entry.is_fresh(now_unix())
        {
            return entry.existence();
        }

        if self.offline {
            let answer = honour_offline(cached);
            if answer != Existence::Unknown {
                self.honoured_stale.borrow_mut().insert(name.to_string());
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

        let existence = outcome.existence();
        self.record(name, existence);
        existence
    }

    /// Answers from the cache, then from nuget.org, then not at all. See
    /// [`PyPiRegistry::exists`].
    ///
    /// A name [`is_valid_nuget_id`] refuses is answered `Missing`
    /// immediately — no cache read, no offline fallback, no request — and
    /// that runs first, ahead of everything
    /// [`exists_via`](Self::exists_via) does, exactly as
    /// [`NpmRegistry::exists`] gates on [`is_valid_npm_name`] first: an
    /// illegal id must never reach the URL or the cache.
    pub fn exists(&self, name: &str) -> Existence {
        if !is_valid_nuget_id(name) {
            return Existence::Missing;
        }
        self.exists_via(name, |name| self.lookup(name))
    }

    /// Asks nuget.org about one package id.
    fn lookup(&self, name: &str) -> Outcome {
        // `NuGetRegistry::exists` already refuses a name this check would
        // reject, before `lookup` is ever reached. Kept as a debug-only
        // tripwire, consistent with [`NpmRegistry::lookup`]'s own guard.
        debug_assert!(
            is_valid_nuget_id(name),
            "nuget id reached the registry unvalidated: {name:?}"
        );
        classify(self.agent.get(&nuget_url(name)).call().map(|_| ()))
    }

    /// Records a definite answer and writes it into the shared cache file.
    /// See [`PyPiRegistry::record`].
    fn record(&self, name: &str, outcome: Existence) {
        if outcome == Existence::Unknown {
            return;
        }
        let entry = Entry {
            exists: outcome == Existence::Exists,
            checked: now_unix(),
        };
        let key = nuget_cache_key(name);
        self.cache.borrow_mut().known.insert(key.clone(), entry);
        if let Some(cache_path) = self.cache_path.as_deref() {
            persist_entry(cache_path, &key, entry);
        }
    }
}

/// Dispatches a lookup to the registry for its ecosystem.
///
/// One [`PyPiRegistry`], one [`NpmRegistry`] and one [`NuGetRegistry`], all
/// constructed against the same `cache_path` — see the cache key prefix
/// functions for how the three share one file without colliding. `check.rs`
/// holds this behind `&dyn Registry` for the whole run, exactly as it held a
/// bare `PyPiRegistry` before npm existed: one router, not one per manifest,
/// because the cache and the unreachable-network latch inside each concrete
/// registry are per-process state that a fresh registry per lookup would
/// throw away.
///
/// Holds the concrete types rather than `Box<dyn Registry>`, so
/// [`RegistryRouter::stale_packages`] can read every registry's
/// `stale_packages()` — a method [`Registry`] deliberately does not carry,
/// per [`PyPiRegistry::stale_packages`]'s own doc comment. A router of trait
/// objects could dispatch lookups but could not report this.
pub struct RegistryRouter {
    pypi: PyPiRegistry,
    npm: NpmRegistry,
    nuget: NuGetRegistry,
}

impl RegistryRouter {
    pub fn new(offline: bool, cache_path: Option<PathBuf>) -> Self {
        Self {
            pypi: PyPiRegistry::new(offline, cache_path.clone()),
            npm: NpmRegistry::new(offline, cache_path.clone()),
            nuget: NuGetRegistry::new(offline, cache_path),
        }
    }

    /// Distinct packages any registry answered from cache entries past
    /// their lifetime, summed across all three. See
    /// [`PyPiRegistry::stale_packages`] for what "stale" means and why
    /// offline honours it at all; the message this feeds only has to be
    /// truthful about the count, not about which ecosystem it came from.
    pub fn stale_packages(&self) -> usize {
        self.pypi.stale_packages() + self.npm.stale_packages() + self.nuget.stale_packages()
    }
}

impl Registry for RegistryRouter {
    fn exists(&self, name: &str, ecosystem: Ecosystem) -> Existence {
        match ecosystem {
            Ecosystem::PyPi => self.pypi.exists(name),
            Ecosystem::Npm => self.npm.exists(name),
            Ecosystem::NuGet => self.nuget.exists(name),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The property the whole move is for: the *default* location must never
    /// be inside the repository being checked — that is the writable,
    /// committable area of whoever proposes the change.
    ///
    /// Scoped to the default deliberately, and the name says so. This calls
    /// `default_cache_path`, which skips [`CACHE_FILE_ENV`], so it proves
    /// nothing about `cache_path` — an earlier version of this comment claimed
    /// "whatever the environment says", which it does not test and which is
    /// not even true, since the variable can name a path anywhere. What holds
    /// the environment path is
    /// `a_cache_file_inside_the_repository_is_never_consulted` in
    /// `tests/cli.rs`, end to end through the real binary.
    ///
    /// Two repository shapes, because the fallbacks differ: an ordinary root,
    /// and one whose name survives no sanitisation at all, which is what
    /// exercises the digest-only branch of [`cache_key`].
    #[test]
    fn the_default_cache_location_is_never_inside_the_repository() {
        for repo in [Path::new("/home/dev/projects/api"), Path::new("/srv/漢字")] {
            let Some(resolved) = default_cache_path(repo) else {
                // Only when no cache directory can be named at all, in which
                // case nothing is written anywhere and the property holds
                // vacuously.
                continue;
            };
            assert!(
                !resolved.starts_with(repo),
                "{} is inside {}",
                resolved.display(),
                repo.display()
            );
            assert!(
                resolved.to_string_lossy().contains("plumb"),
                "{} does not name the tool that owns it",
                resolved.display()
            );
        }
    }

    /// A run that can name no trusted cache directory must not fall back to
    /// somewhere untrusted — it must not cache at all.
    ///
    /// The temporary directory was that fallback and was wrong: a fully
    /// predictable path in a world-writable directory is an unauthenticated
    /// trust store any other local user can pre-create, which is the
    /// in-repository hole reached through a different door. A cold cache costs
    /// a round trip; a poisoned one costs the gate.
    ///
    /// Driven through `PyPiRegistry` rather than the path function, because
    /// "does not cache" is a property of the run: nothing may be read, and
    /// nothing may be written.
    #[test]
    fn a_run_with_no_trusted_cache_directory_writes_nothing() {
        let dir = tempfile::tempdir().expect("temp dir");
        let registry = PyPiRegistry::new(true, None);

        registry.record("ruff", Existence::Exists);
        registry.record("requestz", Existence::Missing);

        assert!(
            std::fs::read_dir(dir.path())
                .expect("read temp dir")
                .next()
                .is_none(),
            "a run with no cache location wrote something anyway"
        );
        // Answers still work in memory for the length of the run.
        assert_eq!(registry.exists("ruff"), Existence::Exists);
    }

    /// Two checkouts must not write over each other. The failure would be
    /// silent — a lost entry costs a round trip, never a wrong answer — but a
    /// per-repository key is also what stops an `Exists` learned against one
    /// index being reused against another, which this project already expects
    /// to need.
    #[test]
    fn two_repositories_do_not_share_a_cache_file() {
        let (Some(one), Some(two)) = (
            default_cache_path(Path::new("/home/dev/one")),
            default_cache_path(Path::new("/home/dev/two")),
        ) else {
            // No cache directory on this machine, so there is nothing to
            // collide. `a_run_with_no_trusted_cache_directory_writes_nothing`
            // covers that case.
            return;
        };

        assert_ne!(one, two);
        assert_eq!(
            one.parent(),
            two.parent(),
            "both belong in the same cache directory"
        );
    }

    /// A key that moved between runs would mean a permanently cold cache, and
    /// a cold cache is a gate that pays a round trip per name every time.
    #[test]
    fn the_same_repository_resolves_to_the_same_cache_file() {
        let repo = Path::new("/home/dev/projects/api");

        assert_eq!(default_cache_path(repo), default_cache_path(repo));
    }

    /// The old location, which is now only ever reported and never read.
    #[test]
    fn the_legacy_path_is_the_one_that_used_to_be_read() {
        assert_eq!(
            legacy_cache_path(Path::new("/home/dev/api")),
            Path::new("/home/dev/api/.plumb/cache/registry.json")
        );
    }

    /// FNV-1a is written out because `DefaultHasher` is not stable across Rust
    /// releases, so this pins it against the published test vectors rather
    /// than against itself.
    #[test]
    fn the_digest_matches_the_published_fnv_vectors() {
        assert_eq!(fnv1a_64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a_64(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a_64(b"foobar"), 0x8594_4171_f739_67e8);
    }

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
        let registry = PyPiRegistry::new(true, Some(cache_path));

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

        let registry = PyPiRegistry::new(true, Some(cache_path));

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

        let registry = PyPiRegistry::new(true, Some(cache_path));

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

        let registry = PyPiRegistry::new(true, Some(cache_path.clone()));
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

        let registry = PyPiRegistry::new(true, Some(cache_path));
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
        let registry = PyPiRegistry::new(false, Some(cache_path));

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
        let registry = PyPiRegistry::new(false, Some(cache_path));
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
        let registry = PyPiRegistry::new(false, Some(cache_path));
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

        let registry = PyPiRegistry::new(true, Some(cache_path));

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

        let registry = PyPiRegistry::new(true, Some(cache_path));

        assert_eq!(registry.exists("ruff"), Existence::Unknown);
        assert_eq!(registry.exists("requestz"), Existence::Unknown);
    }

    /// The legacy-bare-key fallback exists to let a pre-npm cache file keep
    /// answering — see `pypi_cache_key`'s doc comment — and that is the only
    /// thing it may ever do. It must not become a second, unfiltered way to
    /// read the same map: a PyPI query for the literal name `"npm:foo"`
    /// finds no `pypi:npm:foo` entry, falls through to the bare-key lookup,
    /// and — before this fix — matched npm's own `npm:foo` entry outright,
    /// answering a PyPI question with an npm answer.
    ///
    /// This is reachable only if a caller ever asks about a name containing
    /// `:`, which `plumb-core`'s `requirement_name` filters out today (only
    /// ASCII alphanumeric and `-` survive PEP 503 normalisation) — but that
    /// guarantee lives in a different crate and is not asserted here, so
    /// this registry must not depend on it: a local invariant needs a local
    /// guard.
    #[test]
    fn the_legacy_fallback_does_not_read_another_ecosystems_prefixed_key() {
        let dir = tempfile::tempdir().expect("temp dir");
        let cache_path = dir.path().join("registry.json");
        std::fs::write(&cache_path, cache_json(&[("npm:foo", true, 0)])).expect("write cache");

        let registry = PyPiRegistry::new(true, Some(cache_path));

        assert_eq!(
            registry.exists("npm:foo"),
            Existence::Unknown,
            "a pypi query answered from npm's namespace"
        );
    }

    #[test]
    fn a_definite_answer_is_stored_with_the_time_it_was_checked() {
        let dir = tempfile::tempdir().expect("temp dir");
        let cache_path = dir.path().join("registry.json");
        let registry = PyPiRegistry::new(true, Some(cache_path.clone()));

        registry.record("ruff", Existence::Exists);

        let written = std::fs::read_to_string(&cache_path).expect("read cache");
        assert!(written.contains("\"checked\""), "no timestamp: {written}");
        // Round trips as fresh, which is what makes the entry usable at all.
        assert_eq!(
            PyPiRegistry::new(true, Some(cache_path)).exists("ruff"),
            Existence::Exists
        );
    }

    #[test]
    fn offline_mode_never_makes_a_request_and_reports_unknown() {
        let dir = tempfile::tempdir().expect("temp dir");
        let registry = PyPiRegistry::new(true, Some(dir.path().join("registry.json")));

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
        let registry = PyPiRegistry::new(true, Some(cache_path));

        assert_eq!(registry.exists("ruff"), Existence::Exists);
        assert_eq!(registry.exists("requestz"), Existence::Missing);
    }

    #[test]
    fn a_missing_cache_file_is_not_an_error() {
        let dir = tempfile::tempdir().expect("temp dir");
        let registry = PyPiRegistry::new(true, Some(dir.path().join("does-not-exist.json")));

        assert_eq!(registry.exists("ruff"), Existence::Unknown);
    }

    #[test]
    fn a_corrupt_cache_file_is_not_an_error() {
        let dir = tempfile::tempdir().expect("temp dir");
        let cache_path = dir.path().join("registry.json");
        std::fs::write(&cache_path, "not json at all").expect("write cache");

        let registry = PyPiRegistry::new(true, Some(cache_path));

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

        let registry = PyPiRegistry::new(true, Some(cache_path.clone()));
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

        let first = PyPiRegistry::new(true, Some(cache_path.clone()));
        first.record("ruff", Existence::Exists);
        first.record("requestz", Existence::Missing);

        let second = PyPiRegistry::new(true, Some(cache_path));

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
        let registry = PyPiRegistry::new(false, Some(dir.path().join("registry.json")));
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
        let registry = PyPiRegistry::new(false, Some(dir.path().join("registry.json")));
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
        let registry = PyPiRegistry::new(false, Some(cache_path));
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
        let registry = PyPiRegistry::new(false, Some(dir.path().join("registry.json")));
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
        let registry = PyPiRegistry::new(true, Some(cache_path.clone()));

        registry.record("nowhere", Existence::Unknown);
        assert!(
            !cache_path.exists(),
            "an unknown answer created a cache file"
        );

        registry.record("ruff", Existence::Exists);
        registry.record("requestz", Existence::Missing);

        let reloaded = PyPiRegistry::new(true, Some(cache_path));
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
        // `PyPiRegistry` and `NpmRegistry` no longer implement `Registry`
        // themselves — see the trait's doc comment — so it is
        // `RegistryRouter`, the type `check.rs` actually holds as `&dyn
        // Registry` in production, that this must be proven against.
        let dir = tempfile::tempdir().expect("temp dir");
        let cache_path = dir.path().join("registry.json");
        std::fs::write(&cache_path, cache_json(&[("pypi:ruff", true, 0)])).expect("write cache");

        let router = RegistryRouter::new(true, Some(cache_path));
        let erased: &dyn Registry = &router;

        assert_eq!(erased.exists("ruff", Ecosystem::PyPi), Existence::Exists);
    }

    // -- npm and the router -------------------------------------------------
    //
    // `NpmRegistry` mirrors `PyPiRegistry` exactly, so its own tests below
    // are a narrower slice rather than a full duplicate of the ~30 tests
    // above: the shared machinery (TTLs, the latch, `classify`, the lookup
    // timeout) is already pinned once, against `PyPiRegistry`, and pinning
    // it again here would only prove the copy was faithful. What is actually
    // specific to npm — the URL, the character set, the cache key prefix and
    // its lack of a legacy fallback, and the router's dispatch — is what
    // these cover.

    /// Pure and network-free, as the plan requires: `react` is unscoped and
    /// passes through untouched, `@types/node` is scoped and its `/` becomes
    /// `%2F` — npm's own documented way to address it as one path segment.
    #[test]
    fn npm_url_encodes_only_a_scoped_names_slash() {
        assert_eq!(npm_url("react"), "https://registry.npmjs.org/react");
        assert_eq!(
            npm_url("@types/node"),
            "https://registry.npmjs.org/@types%2Fnode"
        );
    }

    /// The character sets are meant to be different, and PyPI's rule must
    /// refuse what only npm allows: `@`, `/`, `_` and `.`, none of which
    /// ASCII-alphanumeric-and-`-` admits. `left-pad` is deliberately not in
    /// this list — every character in it is also legal PyPI syntax, so it
    /// belongs in the "ordinary names satisfy both" check below instead, not
    /// among the names PyPI's rule is supposed to reject.
    #[test]
    fn npm_and_pypi_name_validation_disagree_on_npms_own_characters() {
        for name in ["@types/node", "lodash.merge", "left_pad"] {
            assert!(is_valid_npm_name(name), "npm should admit {name:?}");
            assert!(
                !is_valid_pypi_name(name),
                "pypi's narrower rule must not admit {name:?}"
            );
        }
        // An ordinary hyphenated name is legal syntax in both ecosystems;
        // neither validator has any business rejecting it.
        assert!(is_valid_pypi_name("left-pad"));
        assert!(is_valid_npm_name("left-pad"));
        assert!(is_valid_pypi_name("ruff"));
        assert!(is_valid_npm_name("ruff"), "ordinary names satisfy both");
    }

    /// npm still serves legal packages published before it required a
    /// lowercase name — `registry.npmjs.org/JSONStream` answers 200 today —
    /// so a validator that admits only lowercase refuses a name npm itself
    /// accepts. That was the bug: the previous rule read
    /// `c.is_ascii_lowercase()`, so every legacy uppercase name tripped
    /// `NpmRegistry::lookup`'s `debug_assert` in a debug build, taking the
    /// gate down (exit 101) on a package that had done nothing wrong.
    #[test]
    fn is_valid_npm_name_admits_npms_real_charset() {
        for name in ["JSONStream", "@types/node", "lodash.merge", "left_pad"] {
            assert!(is_valid_npm_name(name), "npm should admit {name:?}");
        }
    }

    /// The other half of the same finding: in release, where `debug_assert`
    /// compiles out, a name outside npm's charset was interpolated into the
    /// lookup URL unescaped — see [`npm_url`] for what it does and does not
    /// encode. `react?bogus` is the reviewer's own live example: it reached
    /// npm as `registry.npmjs.org/react?bogus`, which npm's router reads as
    /// `/react` plus a query string it ignores, so a name that does not
    /// exist answered 200. A charset rejection here — checked as its own
    /// property, independent of [`NpmRegistry::exists`]'s release-path gate
    /// — is the thing that must stop such a name before it ever becomes a
    /// URL.
    #[test]
    fn is_valid_npm_name_rejects_url_metacharacters_and_illegal_characters() {
        for name in ["react?bogus", "a#b", "left pad", "café"] {
            assert!(!is_valid_npm_name(name), "npm should refuse {name:?}");
        }
    }

    /// A definite npm answer is written under the `npm:` prefix, never the
    /// bare name — the bare form means PyPI, and only PyPI, in this shared
    /// file.
    #[test]
    fn an_npm_answer_is_stored_under_the_npm_prefixed_key() {
        let dir = tempfile::tempdir().expect("temp dir");
        let cache_path = dir.path().join("registry.json");
        let registry = NpmRegistry::new(true, Some(cache_path.clone()));

        registry.record("left-pad", Existence::Missing);

        let written = std::fs::read_to_string(&cache_path).expect("read cache");
        assert!(written.contains("\"npm:left-pad\""), "written: {written}");
    }

    /// The other half of the legacy story: a bare key predates npm entirely,
    /// so — unlike `PyPiRegistry`, which falls back to it — npm must not
    /// answer from one.
    #[test]
    fn npm_does_not_fall_back_to_a_legacy_bare_key() {
        let dir = tempfile::tempdir().expect("temp dir");
        let cache_path = dir.path().join("registry.json");
        std::fs::write(&cache_path, cache_json(&[("react", true, 0)])).expect("write cache");

        let registry = NpmRegistry::new(true, Some(cache_path));

        assert_eq!(registry.exists("react"), Existence::Unknown);
    }

    #[test]
    fn npm_offline_mode_never_makes_a_request_and_reports_unknown() {
        let dir = tempfile::tempdir().expect("temp dir");
        let registry = NpmRegistry::new(true, Some(dir.path().join("registry.json")));

        assert_eq!(registry.exists("left-pad"), Existence::Unknown);
    }

    /// The release-path half of the same finding, at the unit level: a name
    /// outside npm's charset must answer `Missing` without ever reaching
    /// [`NpmRegistry::lookup`] — no request built, nothing cached — and it
    /// must do so independent of `--offline`. `offline: false` here is
    /// deliberate, not an oversight: if the charset check did not run before
    /// `exists_via`'s own logic, this call would fall through to `lookup`
    /// and attempt a real connection, which no test in this suite may do.
    /// The charset gate is what has to stop it, not the offline flag.
    #[test]
    fn an_invalid_charset_npm_name_is_missing_without_a_request_or_a_cache_write() {
        let dir = tempfile::tempdir().expect("temp dir");
        let cache_path = dir.path().join("registry.json");
        let registry = NpmRegistry::new(false, Some(cache_path.clone()));

        assert_eq!(registry.exists("react?bogus"), Existence::Missing);
        assert!(
            !cache_path.exists(),
            "an invalid-charset name should never create a cache file"
        );
    }

    /// The exact shape of the reviewer's live poisoned-cache scenario, pinned
    /// as a unit test rather than left correct-by-inspection: before this
    /// fix, `react?bogus` reached npm as `registry.npmjs.org/react?bogus`,
    /// npm's router answered 200 for the truncated `/react`, and the run
    /// cached `npm:react?bogus` as `exists:true` — an entry a *later* run,
    /// on a machine that had never touched the network for this name,
    /// would trust for up to `EXISTS_TTL`.
    ///
    /// This test starts from that already-poisoned state — the entry is
    /// written straight to disk, standing in for one left behind by an
    /// unpatched binary — and proves the charset gate in
    /// `NpmRegistry::exists` runs ahead of the cache read, not just ahead of
    /// the network: an illegal name answers `Missing` even when a fresh,
    /// favourable entry for that exact name is sitting right there, and the
    /// entry is never rewritten either, so a poisoned cache file cannot be
    /// laundered clean by a later run just by asking about it again.
    #[test]
    fn a_poisoned_cache_entry_for_an_illegal_name_is_never_consulted() {
        let dir = tempfile::tempdir().expect("temp dir");
        let cache_path = dir.path().join("registry.json");
        let poisoned = cache_json(&[("npm:react?bogus", true, 0)]);
        std::fs::write(&cache_path, &poisoned).expect("write poisoned cache");

        let registry = NpmRegistry::new(true, Some(cache_path.clone()));
        assert_eq!(registry.exists("react?bogus"), Existence::Missing);

        let after = std::fs::read_to_string(&cache_path).expect("read cache");
        assert_eq!(
            after, poisoned,
            "the poisoned entry must be left exactly as found — neither \
             honoured nor rewritten"
        );
    }

    #[test]
    fn an_npm_cached_answer_is_returned_without_a_request() {
        let dir = tempfile::tempdir().expect("temp dir");
        let cache_path = dir.path().join("registry.json");
        std::fs::write(
            &cache_path,
            cache_json(&[("npm:react", true, 0), ("npm:left-pad-typo", false, 0)]),
        )
        .expect("write cache");

        let registry = NpmRegistry::new(true, Some(cache_path));

        assert_eq!(registry.exists("react"), Existence::Exists);
        assert_eq!(registry.exists("left-pad-typo"), Existence::Missing);
    }

    /// The round trip a real run makes across two invocations of the binary.
    #[test]
    fn a_persisted_npm_cache_is_read_back_by_a_later_registry() {
        let dir = tempfile::tempdir().expect("temp dir");
        let cache_path = dir.path().join("nested").join("registry.json");

        let first = NpmRegistry::new(true, Some(cache_path.clone()));
        first.record("react", Existence::Exists);
        first.record("left-pad-typo", Existence::Missing);

        let second = NpmRegistry::new(true, Some(cache_path));

        assert_eq!(second.exists("react"), Existence::Exists);
        assert_eq!(second.exists("left-pad-typo"), Existence::Missing);
    }

    /// [`Existence::Unknown`] must not be recorded, for npm exactly as for
    /// PyPI — it describes this run's network, not the package.
    #[test]
    fn an_unknown_npm_answer_is_not_written_to_the_cache() {
        let dir = tempfile::tempdir().expect("temp dir");
        let cache_path = dir.path().join("registry.json");
        let registry = NpmRegistry::new(true, Some(cache_path.clone()));

        registry.record("nowhere", Existence::Unknown);

        assert!(
            !cache_path.exists(),
            "an unknown answer created a cache file"
        );
    }

    /// A scoped name round-trips through the cache, `/` and all — the cache
    /// key is the raw name, not the URL-encoded one, so this proves the two
    /// are kept separate rather than one leaking into the other.
    #[test]
    fn a_scoped_npm_name_round_trips_through_the_cache() {
        let dir = tempfile::tempdir().expect("temp dir");
        let cache_path = dir.path().join("registry.json");
        let registry = NpmRegistry::new(true, Some(cache_path.clone()));

        registry.record("@types/node", Existence::Exists);

        let written = std::fs::read_to_string(&cache_path).expect("read cache");
        assert!(
            written.contains("\"npm:@types/node\""),
            "written: {written}"
        );
        let reloaded = NpmRegistry::new(true, Some(cache_path));
        assert_eq!(reloaded.exists("@types/node"), Existence::Exists);
    }

    /// The property the whole shared-file design exists for: an entry
    /// written by one ecosystem must not answer a lookup in the other, and
    /// the router must send each name to the registry that owns it.
    #[test]
    fn the_router_dispatches_by_ecosystem_and_the_two_do_not_cross_answer() {
        let dir = tempfile::tempdir().expect("temp dir");
        let cache_path = dir.path().join("registry.json");
        std::fs::write(
            &cache_path,
            cache_json(&[("pypi:ruff", true, 0), ("npm:left-pad", true, 0)]),
        )
        .expect("write cache");

        let router = RegistryRouter::new(true, Some(cache_path));

        assert_eq!(router.exists("ruff", Ecosystem::PyPi), Existence::Exists);
        assert_eq!(router.exists("left-pad", Ecosystem::Npm), Existence::Exists);
        // Same names, wrong ecosystem: neither registry's cache has an entry
        // under the other's key, so both must come back Unknown rather than
        // reusing the sibling's answer.
        assert_eq!(router.exists("ruff", Ecosystem::Npm), Existence::Unknown);
        assert_eq!(
            router.exists("left-pad", Ecosystem::PyPi),
            Existence::Unknown
        );
    }

    /// `RegistryRouter::stale_packages` sums both registries' counts, so a
    /// run that honoured one stale answer in each ecosystem reports two, not
    /// one and not zero.
    #[test]
    fn the_router_sums_stale_packages_from_both_registries() {
        let dir = tempfile::tempdir().expect("temp dir");
        let cache_path = dir.path().join("registry.json");
        let age = well_past_every_lifetime();
        std::fs::write(
            &cache_path,
            cache_json(&[("pypi:ruff", true, age), ("npm:left-pad", true, age)]),
        )
        .expect("write cache");
        let router = RegistryRouter::new(true, Some(cache_path));

        assert_eq!(router.stale_packages(), 0, "nothing has been asked yet");

        assert_eq!(router.exists("ruff", Ecosystem::PyPi), Existence::Exists);
        assert_eq!(router.exists("left-pad", Ecosystem::Npm), Existence::Exists);

        assert_eq!(router.stale_packages(), 2);
    }

    /// Two manifests in one change, one per ecosystem, must not make either
    /// registry's write erase the other's — the property [`persist_entry`]'s
    /// read-modify-write exists for. Reaches the private fields directly
    /// (this test module is a descendant of `registry`, so it may) to record
    /// through each concrete registry the way two different manifests in the
    /// same run would, one after the other, sharing one router and one file.
    ///
    /// A version that persisted a blind snapshot of `self.cache` — what this
    /// file did before either registry needed to share — fails this: the
    /// second `record` call's registry loaded its snapshot before the first
    /// call wrote anything, so writing that snapshot back would overwrite the
    /// first answer with nothing there at all.
    #[test]
    fn a_write_from_one_registry_does_not_erase_the_others() {
        let dir = tempfile::tempdir().expect("temp dir");
        let cache_path = dir.path().join("registry.json");
        let router = RegistryRouter::new(false, Some(cache_path.clone()));

        router.pypi.record("ruff", Existence::Exists);
        router.npm.record("left-pad", Existence::Exists);

        let reloaded = RegistryRouter::new(true, Some(cache_path));
        assert_eq!(reloaded.exists("ruff", Ecosystem::PyPi), Existence::Exists);
        assert_eq!(
            reloaded.exists("left-pad", Ecosystem::Npm),
            Existence::Exists
        );
    }

    // -- NuGet ----------------------------------------------------------
    //
    // `NuGetRegistry` mirrors `NpmRegistry`, which mirrors `PyPiRegistry`;
    // the shared machinery is already pinned above. This is the task's own
    // test list: the validity guard, case-insensitive cache keys, the
    // lowercased lookup URL, the unreachable-network answer, the
    // flat-container status mapping, and the `nuget:` cache key prefix.

    /// An ID [`is_valid_nuget_id`] refuses must never reach the network or
    /// the cache — the npm lesson: an illegal name interpolated into a
    /// lookup URL can resolve to something else entirely and poison the
    /// cache under a name nobody searched for. `offline: false` is
    /// deliberate, as in npm's own twin test: if the charset guard did not
    /// run first, `exists` would fall through to a real connection attempt,
    /// which no test in this suite may make.
    #[test]
    fn an_illegal_nuget_id_is_missing_without_a_request() {
        let dir = tempfile::tempdir().expect("temp dir");
        let cache_path = dir.path().join("registry.json");
        let registry = NuGetRegistry::new(false, Some(cache_path.clone()));

        for name in ["bad name", "x?y", ""] {
            assert_eq!(registry.exists(name), Existence::Missing, "{name:?}");
        }

        assert!(
            !cache_path.exists(),
            "an illegal id should never create a cache file, let alone a request"
        );
    }

    /// `.` and `..` are RFC 3986 dot-segments: `is_valid_nuget_id`'s old
    /// charset admitted both, since `.` is otherwise a legal character, so
    /// `nuget_url("..")` emitted `.../v3-flatcontainer/../index.json` — a path the
    /// pinned http client does not normalise client-side, leaving
    /// server-side normalisation to decide what actually answers, cached
    /// under `nuget:..` as a Tier A verdict regardless. Leading or trailing
    /// `.`/`-` are the same family: a package ID with a punctuation end is
    /// not one nuget.org would ever issue, so refusing it costs nothing.
    /// `"Newtonsoft.Json"` is the positive control: an ordinary ID with
    /// interior dots must still pass.
    #[test]
    fn a_dot_segment_id_is_missing_without_a_request() {
        let dir = tempfile::tempdir().expect("temp dir");
        let cache_path = dir.path().join("registry.json");
        let registry = NuGetRegistry::new(false, Some(cache_path.clone()));

        for name in ["..", ".", ".leading", "trailing.", "-leading", "trailing-"] {
            assert_eq!(registry.exists(name), Existence::Missing, "{name:?}");
        }
        assert!(
            !cache_path.exists(),
            "a dot-segment or edge-punctuated id should never create a cache file"
        );

        assert!(is_valid_nuget_id("Newtonsoft.Json"), "positive control");
    }

    /// NuGet package IDs are case-insensitive, so two spellings of the same
    /// ID must share one cache entry. Offline, so the only way this can
    /// answer at all is from the cache.
    #[test]
    fn nuget_ids_are_case_insensitive_in_the_cache() {
        let dir = tempfile::tempdir().expect("temp dir");
        let cache_path = dir.path().join("registry.json");
        std::fs::write(
            &cache_path,
            cache_json(&[("nuget:newtonsoft.json", true, 0)]),
        )
        .expect("write cache");

        let registry = NuGetRegistry::new(true, Some(cache_path));

        assert_eq!(registry.exists("Newtonsoft.Json"), Existence::Exists);
    }

    /// Pure and network-free, as the plan requires: the flat-container index
    /// is served at the lowercased path only, so the URL builder must
    /// lowercase the id itself rather than relying on every caller to.
    ///
    /// `v3-flatcontainer`, confirmed against the real service with `curl`
    /// during this task's live smoke test — see `nuget_url`'s doc comment
    /// for what the wrong path (`v3-flat2`, this test's own value until
    /// then) cost: every lookup 404ing, real packages included.
    #[test]
    fn the_lookup_url_lowercases_the_id() {
        assert_eq!(
            nuget_url("OpenAI"),
            "https://api.nuget.org/v3-flatcontainer/openai/index.json"
        );
    }

    /// A transport failure is a fact about this run's network, not about the
    /// package: `Unknown`, and never cached. See
    /// [`PyPiRegistry::exists_via`] for the same property proven against
    /// PyPI.
    #[test]
    fn an_unreachable_registry_is_unknown() {
        let dir = tempfile::tempdir().expect("temp dir");
        let cache_path = dir.path().join("registry.json");
        let registry = NuGetRegistry::new(false, Some(cache_path.clone()));

        assert_eq!(
            registry.exists_via("Newtonsoft.Json", |_| Outcome::Unreachable),
            Existence::Unknown
        );
        assert!(
            !cache_path.exists(),
            "an unreachable registry cached something"
        );
    }

    /// The flat-container contract: a 200 — [`classify`]'s
    /// `Outcome::Answered(Existence::Exists)` — means the package exists, a
    /// 404 — `Outcome::Answered(Existence::Missing)` — means it does not.
    /// `classify` itself is shared with PyPI and npm and is pinned once,
    /// against real status codes, in `only_a_404_reads_as_missing`; this
    /// proves `NuGetRegistry` propagates what it is given rather than
    /// reinterpreting it.
    #[test]
    fn a_404_is_missing_and_a_200_is_exists() {
        let dir = tempfile::tempdir().expect("temp dir");
        let registry = NuGetRegistry::new(false, Some(dir.path().join("registry.json")));

        assert_eq!(
            registry.exists_via("Missing.Package", |_| Outcome::Answered(Existence::Missing)),
            Existence::Missing
        );
        assert_eq!(
            registry.exists_via("Newtonsoft.Json", |_| Outcome::Answered(Existence::Exists)),
            Existence::Exists
        );
    }

    /// A definite answer is written under the `nuget:` prefix, lowercased —
    /// see [`nuget_cache_key`].
    #[test]
    fn cache_keys_carry_the_nuget_prefix() {
        let dir = tempfile::tempdir().expect("temp dir");
        let cache_path = dir.path().join("registry.json");
        let registry = NuGetRegistry::new(true, Some(cache_path.clone()));

        registry.record("Newtonsoft.Json", Existence::Exists);

        let written = std::fs::read_to_string(&cache_path).expect("read cache");
        assert!(
            written.contains("\"nuget:newtonsoft.json\""),
            "written: {written}"
        );
    }
}
