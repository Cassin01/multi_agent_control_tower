//! Name pool for dynamic expert add.
//!
//! Provides automatic name selection from a curated literary pool, with a
//! deterministic fallback that synthesises a unique name from the expert ID
//! when the pool is exhausted or every entry is already in use.
//!
//! See design doc §3.4 for the rationale on the pool composition.

// Items in this module are consumed by `ExpertAddService` (Phase 8) and
// the CLI/TUI surfaces (Phases 10–11). Until those land, suppress
// dead-code lints from the bin target to keep `make lint` green.
#![allow(dead_code)]

use std::collections::HashSet;

use crate::models::ExpertId;

/// Curated literary name pool. Existing 4 names from `Config::default()` are
/// preserved at the head of the list and the rest are drawn from the same
/// novel for stylistic consistency.
pub const LITERARY_NAMES: &[&str] = &[
    // Existing 4 names — must remain unchanged for backwards compatibility.
    "Alyosha",
    "Ilyusha",
    "Grigory",
    "Katya",
    // Extension drawn from the same novel.
    "Dmitri",
    "Ivan",
    "Smerdyakov",
    "Fyodor",
    "Zosima",
    "Lise",
    "Varvara",
    "Marfa",
];

/// Selector for an unused expert name.
#[derive(Debug, Clone, Copy)]
pub struct NamePool {
    pool: &'static [&'static str],
}

impl NamePool {
    /// Construct a pool backed by the default literary list.
    pub const fn new() -> Self {
        Self {
            pool: LITERARY_NAMES,
        }
    }

    /// Return the first name in the pool that is not present in `used`.
    ///
    /// Returns `None` when every pool entry is already taken; the caller is
    /// expected to fall back to [`NamePool::fallback`] in that case.
    pub fn pick_unused(&self, used: &HashSet<&str>) -> Option<&'static str> {
        self.pool.iter().copied().find(|name| !used.contains(name))
    }

    /// Synthesise a fallback name from the expert ID.
    ///
    /// The result satisfies the naming regex `^[A-Za-z][A-Za-z0-9_-]*$` and
    /// is wide enough to avoid disrupting the tower column layout.
    pub fn fallback(&self, id: ExpertId) -> String {
        format!("Expert{id:02}")
    }
}

impl Default for NamePool {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use regex::Regex;

    fn name_regex() -> Regex {
        Regex::new(r"^[A-Za-z][A-Za-z0-9_-]*$").expect("valid regex")
    }

    #[test]
    fn pick_unused_returns_first_free_literary_name() {
        let pool = NamePool::new();
        let used: HashSet<&str> = HashSet::new();
        let picked = pool.pick_unused(&used);
        assert_eq!(
            picked,
            Some("Alyosha"),
            "pick_unused: empty `used` should return the first pool entry"
        );
    }

    #[test]
    fn pick_unused_skips_taken_names() {
        let pool = NamePool::new();
        let mut used: HashSet<&str> = HashSet::new();
        used.insert("Alyosha");
        used.insert("Ilyusha");
        let picked = pool.pick_unused(&used);
        assert_eq!(
            picked,
            Some("Grigory"),
            "pick_unused: should skip names present in `used`"
        );
    }

    #[test]
    fn pick_unused_returns_none_when_pool_exhausted() {
        let pool = NamePool::new();
        let used: HashSet<&str> = LITERARY_NAMES.iter().copied().collect();
        let picked = pool.pick_unused(&used);
        assert!(
            picked.is_none(),
            "pick_unused: full `used` set must yield None"
        );
    }

    #[test]
    fn fallback_matches_two_digit_pattern() {
        let pool = NamePool::new();
        assert_eq!(
            pool.fallback(0),
            "Expert00",
            "fallback: id 0 should pad to two digits"
        );
        assert_eq!(
            pool.fallback(7),
            "Expert07",
            "fallback: id 7 should pad to two digits"
        );
        assert_eq!(
            pool.fallback(42),
            "Expert42",
            "fallback: id 42 should keep two-digit shape"
        );
        assert_eq!(
            pool.fallback(1234),
            "Expert1234",
            "fallback: ids beyond two digits should grow naturally"
        );
    }

    proptest! {
        // Property: NamePool always returns a valid name (literary or fallback).
        // **Validates: Requirements 3.4, 5.1 (naming regex)**
        #[test]
        fn name_pool_always_yields_valid_unused_name(
            used_names in prop::collection::vec("[A-Za-z][A-Za-z0-9_-]{0,15}", 0..32),
            id in 0u32..1_000_000,
        ) {
            let regex = name_regex();
            let used: HashSet<&str> = used_names.iter().map(String::as_str).collect();
            let pool = NamePool::new();

            let picked: String = match pool.pick_unused(&used) {
                Some(literary) => literary.to_string(),
                None => pool.fallback(id),
            };

            prop_assert!(
                regex.is_match(&picked),
                "selected name {picked:?} must satisfy naming regex"
            );

            // pick_unused never collides with `used`. The fallback may collide
            // when `used` happens to contain "Expert{id:02}", but the service
            // layer reconciles that by retrying with the next ID; the unit
            // here only guarantees regex validity for the fallback path.
            if pool.pick_unused(&used).is_some() {
                prop_assert!(
                    !used.contains(picked.as_str()),
                    "pick_unused returned a name present in used set: {picked:?}"
                );
            }
        }
    }
}
