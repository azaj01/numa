//! A two-layer domain set: entries seeded from `numa.toml` (reloaded, never
//! persisted) and entries added at runtime (persisted to a JSON file, never
//! written back into the TOML). For features where the UI adds/removes
//! individual domains durably alongside a config-declared bulk list — the
//! rebind allowlist and the blocking allow/block lists. Mirrors
//! `ServiceStore`'s config-vs-user split.

use std::collections::HashSet;
use std::path::PathBuf;

use crate::blocklist::{find_in_set, normalize};
use crate::persist::{load_json_vec, save_json};

#[derive(Default)]
pub struct PersistedDomainList {
    config: HashSet<String>, // from numa.toml; reloaded each boot, never saved
    user: HashSet<String>,   // runtime-added; persisted to `persist_path`
    persist_path: PathBuf,
}

impl PersistedDomainList {
    /// `filename` is resolved under the platform config dir, e.g.
    /// `rebind-allow.json`.
    pub fn new(filename: &str) -> Self {
        PersistedDomainList {
            config: HashSet::new(),
            user: HashSet::new(),
            persist_path: crate::config_dir().join(filename),
        }
    }

    /// Seed a config-declared entry (not written to disk).
    pub fn insert_from_config(&mut self, domain: &str) {
        self.config.insert(normalize(domain));
    }

    /// Add a runtime entry and persist. No-op if config already covers it
    /// exactly or it is already present.
    pub fn insert(&mut self, domain: &str) {
        let d = normalize(domain);
        if !self.config.contains(&d) && self.user.insert(d) {
            self.save();
        }
    }

    /// Remove a runtime entry, persisting on change. Config entries are
    /// file-owned and cannot be removed here; returns false for them.
    pub fn remove(&mut self, domain: &str) -> bool {
        if self.user.remove(&normalize(domain)) {
            self.save();
            true
        } else {
            false
        }
    }

    /// Exact-or-parent suffix match against either layer: `example.com`
    /// matches `nas.example.com` but never `evilexample.com`.
    pub fn matches(&self, qname: &str) -> bool {
        let n = normalize(qname);
        find_in_set(&n, &self.config).is_some() || find_in_set(&n, &self.user).is_some()
    }

    /// Whether the exact (normalized) domain came from config — lets the UI
    /// mark which entries are durable vs runtime-removable.
    pub fn is_config(&self, domain: &str) -> bool {
        self.config.contains(&normalize(domain))
    }

    /// All entries (config ∪ user), sorted, for listing.
    pub fn entries(&self) -> Vec<String> {
        let mut v: Vec<String> = self.config.union(&self.user).cloned().collect();
        v.sort();
        v
    }

    /// Load persisted runtime entries. Call once at startup, after seeding
    /// config entries (so config takes precedence on overlap).
    pub fn load_persisted(&mut self) {
        for domain in load_json_vec::<String>(&self.persist_path) {
            let d = normalize(&domain);
            if !self.config.contains(&d) {
                self.user.insert(d);
            }
        }
    }

    fn save(&self) {
        let mut entries: Vec<&String> = self.user.iter().collect();
        entries.sort();
        save_json(&self.persist_path, &entries);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Store whose persisted writes go to /dev/null (mirrors `service_store`
    /// tests) — exercises the save path without touching the real config dir.
    fn test_list() -> PersistedDomainList {
        PersistedDomainList {
            config: HashSet::new(),
            user: HashSet::new(),
            persist_path: PathBuf::from("/dev/null"),
        }
    }

    #[test]
    fn config_and_user_layers_both_match() {
        let mut l = test_list();
        l.insert_from_config("config.example");
        l.insert("user.example");
        assert!(l.matches("config.example"));
        assert!(l.matches("user.example"));
        assert!(!l.matches("other.example"));
    }

    #[test]
    fn suffix_match_covers_subdomains_not_lookalikes() {
        let mut l = test_list();
        l.insert_from_config("example.com");
        assert!(l.matches("nas.example.com"));
        assert!(l.matches("example.com"));
        assert!(!l.matches("evilexample.com"));
    }

    #[test]
    fn normalizes_case_and_trailing_dot() {
        let mut l = test_list();
        l.insert("NAS.Example.COM.");
        assert!(l.matches("nas.example.com"));
        assert_eq!(l.entries(), vec!["nas.example.com"]);
    }

    #[test]
    fn remove_drops_user_entry_but_not_config() {
        let mut l = test_list();
        l.insert_from_config("keep.example");
        l.insert("drop.example");
        assert!(l.remove("drop.example"));
        assert!(!l.matches("drop.example"));
        // Config entry is file-owned: remove is a no-op and reports false.
        assert!(!l.remove("keep.example"));
        assert!(l.matches("keep.example"));
    }

    #[test]
    fn insert_skips_domain_already_in_config() {
        let mut l = test_list();
        l.insert_from_config("dup.example");
        l.insert("dup.example");
        // No duplicate user copy; still a single listed entry, still config-owned.
        assert_eq!(l.entries(), vec!["dup.example"]);
        assert!(l.is_config("dup.example"));
    }

    #[test]
    fn entries_are_sorted_union() {
        let mut l = test_list();
        l.insert_from_config("b.example");
        l.insert("a.example");
        l.insert("c.example");
        assert_eq!(l.entries(), vec!["a.example", "b.example", "c.example"]);
    }

    #[test]
    fn is_config_distinguishes_layers() {
        let mut l = test_list();
        l.insert_from_config("seed.example");
        l.insert("runtime.example");
        assert!(l.is_config("seed.example"));
        assert!(!l.is_config("runtime.example"));
    }
}
