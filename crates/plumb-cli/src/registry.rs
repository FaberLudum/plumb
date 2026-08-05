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

#[derive(Debug, Default, Serialize, Deserialize)]
struct Cache {
    known: BTreeMap<String, bool>,
}

/// Resolves names against PyPI, with an on-disk cache and an offline mode.
#[allow(dead_code, reason = "wired into the gate in a later task")]
pub struct PyPiRegistry {
    offline: bool,
    cache_path: PathBuf,
    cache: RefCell<Cache>,
}

#[allow(dead_code, reason = "wired into the gate in a later task")]
impl PyPiRegistry {
    /// Reads the cache at `cache_path`, if there is a readable one.
    ///
    /// A missing, unreadable or malformed cache file yields an empty cache
    /// rather than an error. The cache is an optimisation; losing it costs a
    /// round trip, and refusing to start over a corrupt one would turn a
    /// scratch file into a hard failure of the gate.
    pub fn new(offline: bool, cache_path: PathBuf) -> Self {
        let cache = std::fs::read_to_string(&cache_path)
            .ok()
            .and_then(|text| serde_json::from_str::<Cache>(&text).ok())
            .unwrap_or_default();

        Self {
            offline,
            cache_path,
            cache: RefCell::new(cache),
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
        self.cache
            .borrow_mut()
            .known
            .insert(name.to_string(), outcome == Existence::Exists);
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

/// Turns one lookup result into an outcome.
///
/// Only a 200 and a 404 say anything about the package. Everything else — a
/// 5xx, a 403 from a proxy, a dead host, a socket error, a timeout — is a
/// failure to ask, and reports [`Existence::Unknown`].
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
        if let Some(&known) = self.cache.borrow().known.get(name) {
            return if known {
                Existence::Exists
            } else {
                Existence::Missing
            };
        }

        if self.offline {
            return Existence::Unknown;
        }

        let url = format!("https://pypi.org/pypi/{name}/json");
        let outcome = classify(ureq::get(&url).call().map(|_| ()));

        self.record(name, outcome);
        outcome
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        std::fs::write(&cache_path, r#"{"known":{"ruff":true,"requestz":false}}"#)
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
        std::fs::write(&cache_path, r#"{"known":{"ruff":true}}"#).expect("write cache");

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
        first.cache.borrow_mut().known.insert("ruff".into(), true);
        first
            .cache
            .borrow_mut()
            .known
            .insert("requestz".into(), false);
        first.persist();

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

    /// `Unknown` says something about this run's network, not about the
    /// package, so recording it would freeze one outage into every later run.
    #[test]
    fn recording_stores_definite_answers_and_ignores_unknown() {
        let dir = tempfile::tempdir().expect("temp dir");
        let cache_path = dir.path().join("registry.json");
        let registry = PyPiRegistry::new(false, cache_path.clone());

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

    /// Callers hold the registry as `&dyn Registry` so they can inject a stub,
    /// which only compiles while the trait stays object safe. A method taking
    /// or returning `Self`, or a generic parameter, would break that silently
    /// for every caller at once.
    #[test]
    fn the_registry_trait_is_object_safe() {
        let dir = tempfile::tempdir().expect("temp dir");
        let cache_path = dir.path().join("registry.json");
        std::fs::write(&cache_path, r#"{"known":{"ruff":true}}"#).expect("write cache");

        let registry = PyPiRegistry::new(true, cache_path);
        let erased: &dyn Registry = &registry;

        assert_eq!(erased.exists("ruff"), Existence::Exists);
    }
}
