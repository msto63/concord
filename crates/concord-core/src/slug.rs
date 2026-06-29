//! Area-name slugging and overlap detection.
//!
//! Parity: the shell `slug()` is `tr '/ ' '__'` — every `/` and space becomes `_`.
//! A lease lives in a directory named by the slug, so the slug is the on-disk key.
//!
//! Two structural weaknesses of the shell scheme, surfaced for the typed port:
//!
//!  1. **Slug collision.** `a/b` and `a b` slug to the same `a_b`, so they share one
//!     lease dir — a silent aliasing the shell cannot see. We keep `slug()` byte-exact
//!     for drop-in parity, but expose [`overlaps`] on the *original* area strings so
//!     the claim path can reason about real overlap rather than slug identity.
//!
//!  2. **No path-prefix overlap check.** The shell treats `kernel/src/embedded` and
//!     `kernel/src/embedded/usbd` as unrelated keys, so two sessions can hold a parent
//!     and child of the same subtree at once — exactly the class of collision Concord
//!     exists to prevent (WP12 §6). [`overlaps`] catches it.

/// Slug an area name to its on-disk lease-directory key.
///
/// Byte-exact with the shell `tr '/ ' '__'`: only `/` and ASCII space are remapped,
/// each to a single `_`. No collapsing, no trimming, no lowercasing.
pub fn slug(area: &str) -> String {
    area.chars()
        .map(|c| if c == '/' || c == ' ' { '_' } else { c })
        .collect()
}

/// Do two area paths overlap — i.e. is one equal to or a path-prefix of the other?
///
/// This is the structural claim guard the shell lacks. Comparison is on the raw
/// area strings split into `/`-separated segments, so `kernel/src/embedded` overlaps
/// `kernel/src/embedded/usbd` (parent ⊃ child) and itself, but NOT `kernel/src/embed`
/// (segment-wise, not substring-wise — `embed` is not the segment `embedded`).
///
/// Spaces are not treated as separators here (only `/`), matching how areas are
/// written by callers; the slug collision in (1) is a separate concern handled by
/// comparing raw strings before slugging.
pub fn overlaps(a: &str, b: &str) -> bool {
    let sa: Vec<&str> = segments(a);
    let sb: Vec<&str> = segments(b);
    let n = sa.len().min(sb.len());
    // Equal up to the shorter length ⇒ one is a prefix (or equal) of the other.
    sa[..n] == sb[..n]
}

/// Split an area into non-empty `/`-separated segments, ignoring leading/trailing
/// and duplicate slashes so `a//b/` and `a/b` compare equal.
fn segments(area: &str) -> Vec<&str> {
    area.split('/').filter(|s| !s.is_empty()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_matches_tr_semantics() {
        assert_eq!(slug("kernel/src/main.rs"), "kernel_src_main.rs");
        assert_eq!(slug("merge #411"), "merge_#411");
        assert_eq!(slug("a/b c/d"), "a_b_c_d");
        // The documented collision: distinct areas, identical slug.
        assert_eq!(slug("a/b"), slug("a b"));
    }

    #[test]
    fn overlap_detects_prefix_and_self() {
        assert!(overlaps("kernel/src/embedded", "kernel/src/embedded/usbd"));
        assert!(overlaps("kernel/src/embedded/usbd", "kernel/src/embedded"));
        assert!(overlaps("a/b/c", "a/b/c"));
    }

    #[test]
    fn overlap_rejects_siblings_and_substrings() {
        assert!(!overlaps("kernel/src/embedded", "kernel/src/embed"));
        assert!(!overlaps("a/b/c", "a/b/d"));
        assert!(!overlaps("user/usbd", "user/basisd"));
    }

    #[test]
    fn overlap_ignores_redundant_slashes() {
        assert!(overlaps("a//b/", "a/b"));
    }
}
