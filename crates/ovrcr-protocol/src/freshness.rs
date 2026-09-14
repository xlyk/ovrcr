//! How old a sample is, and when it counts as stale.
//!
//! One authority for managed agent reporting: the runtime stamps each component
//! of a metrics snapshot through [`received_at`], and the Dashboard,
//! `session list`, `session context` and `session usage` read that stamp through
//! [`age_ms`] and [`is_stale`]. Providers do not certify ordering, so the stamp
//! is the only provenance a reader gets.
//!
//! A timer replay of an unchanged provider sample keeps its stamp; only a
//! sample that actually differs advances it. A live session whose totals keep
//! moving therefore never goes stale.
//!
//! One receipt stamp is still kept elsewhere: the legacy unbound `report
//! context` path in `ovrcr-runtime`'s `session::AgentUpdate::Context` stores the
//! arrival time of every accepted report, so a replay advances it there. It
//! reads the five-minute rule from [`is_stale`] but does not use
//! [`received_at`]. Folding that second store in is out of this module's scope.

/// A sample this old or older is stale.
pub const STALE_AFTER_MS: u64 = 300_000;

/// Milliseconds since `received_unix_ms`; `None` when the stamp is in the
/// future and the clock cannot vouch for it.
pub fn age_ms(received_unix_ms: u64, now_unix_ms: u64) -> Option<u64> {
    now_unix_ms.checked_sub(received_unix_ms)
}

/// Stale once the session has exited, once the sample reaches
/// [`STALE_AFTER_MS`], or once its stamp is in the future.
pub fn is_stale(received_unix_ms: u64, now_unix_ms: u64, exited: bool) -> bool {
    exited || age_ms(received_unix_ms, now_unix_ms).is_none_or(|age| age >= STALE_AFTER_MS)
}

/// The receipt stamp for `sample`: `now_unix_ms` when it differs from the last
/// sample `watermark` saw, the stamp already recorded when a timer replays an
/// unchanged one.
///
/// "Unchanged" is whole-sample equality, deliberately: for a `Measurement` the
/// reporter's `source` label counts too, so the same value arriving under a
/// different source is a new reading and takes a new stamp. Production runs one
/// source per component per binding, so this only decides the ambiguous case,
/// and it decides it the safe way — a reader is told a value is fresh, never
/// that a genuinely new one is old. Comparing `value` alone would also leave
/// this module knowing the shape of a protocol type it otherwise ignores.
pub fn received_at<T: Clone + PartialEq>(
    watermark: &mut Option<(T, u64)>,
    sample: &T,
    now_unix_ms: u64,
) -> u64 {
    match watermark {
        Some((seen, received)) if seen == sample => *received,
        _ => {
            *watermark = Some((sample.clone(), now_unix_ms));
            now_unix_ms
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replay_keeps_its_stamp_and_a_change_advances_it() {
        let mut watermark = None;
        assert_eq!(received_at(&mut watermark, &"a", 1_000), 1_000);
        assert_eq!(received_at(&mut watermark, &"a", 9_000), 1_000);
        assert_eq!(received_at(&mut watermark, &"b", 9_000), 9_000);
        assert_eq!(received_at(&mut watermark, &"a", 12_000), 12_000);
    }

    #[test]
    fn stale_after_five_minutes_or_exit() {
        assert_eq!(age_ms(1_000, 4_000), Some(3_000));
        assert_eq!(age_ms(4_000, 1_000), None);
        assert!(!is_stale(1_000, 1_000 + STALE_AFTER_MS - 1, false));
        assert!(is_stale(1_000, 1_000 + STALE_AFTER_MS, false));
        assert!(is_stale(1_000, 999, false), "a future stamp is stale");
        assert!(is_stale(1_000, 1_000, true), "an exited session is stale");
    }
}
