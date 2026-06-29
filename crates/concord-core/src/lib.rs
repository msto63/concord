//! `concord-core` — the typed coordination state model and atomic on-disk
//! transitions behind the Concord CLI (WP12 M1).
//!
//! Design stance (WP12 leitplanken):
//!  - **Drop-in parity first.** The on-disk layout (`sessions/<id>`, `leases/<area>/`,
//!    `intents.jsonl`, `merge.lock/`, the prose channel) is byte-compatible with
//!    `bin/coord.sh`; the Rust binary and the shell can coexist mid-migration.
//!  - **Typed, atomic, ownership-enforced.** Transitions go through [`store::Store`],
//!    which makes field writes atomic (temp + rename) and reports ownership instead
//!    of silently succeeding — the structural fixes the shell cannot make.
//!  - **New capabilities are opt-in and gated**, not bolted onto the parity path
//!    (see [`store::OverlapPolicy`]).

pub mod clock;
pub mod directive;
pub mod error;
pub mod ipc;
pub mod message;
pub mod model;
pub mod paths;
pub mod slug;
pub mod store;

pub use error::{ConcordError, Result};
pub use model::{LedgerEntry, Lease, MergeLock, Session};
pub use paths::Paths;
pub use store::{
    ClaimOutcome, HoldStatus, MergeLockOutcome, MergeUnlockOutcome, OverlapPolicy, ReleaseOutcome,
    StatusReport, Store,
};
