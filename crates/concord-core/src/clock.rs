//! Wall-clock time as Unix seconds — the parity-exact equivalent of `date +%s`.
//!
//! The shell version calls `date +%s` (integer seconds since the epoch) for every
//! `started`/`heartbeat`/`since`/`t` value. We capture it once per operation and
//! reuse it; that is strictly more consistent than the shell (which re-shells
//! `now()` several times per command) and produces identical on-disk values modulo
//! the unavoidable wall-clock skew between two separate runs.

use std::time::{SystemTime, UNIX_EPOCH};

/// Current Unix time in whole seconds, matching `date +%s`.
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        // Pre-1970 clocks are not a real scenario on the coordination host; fall
        // back to 0 rather than panicking, mirroring the shell's never-fails posture.
        .unwrap_or(0)
}
