//! Cross-cutting state primitives.
//!
//! Currently houses the advisory file lock used to serialise mutations
//! to `.macot/` (see [`lock::MacotLock`]). Future state-layer helpers
//! (e.g. checkpoint files, transaction logs) belong here too.

pub mod lock;
