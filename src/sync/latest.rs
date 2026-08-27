//! Wait-free latest-value handoff between one writer and one reader.
//!
//! The UI must never wait on the engine. A mutex cannot promise that: even a
//! short critical section is a section, and `try_lock` only converts the wait
//! into a *missed update*, which is how bursty row delivery and stale readings
//! get in. So the boundary uses no lock at all.
//!
//! This is the classic triple buffer. Three slots, three indices — one owned by
//! the writer, one by the reader, one published — and every handoff is a single
//! atomic swap. Neither side can block the other, or even delay it: both
//! operations are wait-free, bounded by one `swap` regardless of what the other
//! side is doing.
//!
//! Latest-value, not a queue: a reader that falls behind sees the newest value
//! and never a backlog. Use a queue ([`rtrb`]) for anything where every item
//! matters, like spectrum rows.

use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// Low bits hold the published slot; the flag says it has not been read yet.
const INDEX_MASK: usize = 0b011;
const FRESH_FLAG: usize = 0b100;

struct Shared<T> {
    /// Exactly one of these is owned by the writer, one by the reader, and one
    /// is published. The three indices are always a permutation of 0..3, which
    /// is what makes the unsynchronised slot access sound.
    slots: [UnsafeCell<T>; 3],
    published: AtomicUsize,
}

// SAFETY: a slot is only ever touched by the side that currently owns its
// index, and ownership moves only through the atomic swap in `publish`/`fetch`.
// No two threads can hold the same index at the same time, so no slot is ever
// aliased.
unsafe impl<T: Send> Send for Shared<T> {}
unsafe impl<T: Send> Sync for Shared<T> {}

/// The writing half. Fill [`Self::slot`], then [`Self::publish`].
pub struct LatestWriter<T> {
    shared: Arc<Shared<T>>,
    owned: usize,
}

/// The reading half. [`Self::fetch`], then read [`Self::slot`].
pub struct LatestReader<T> {
    shared: Arc<Shared<T>>,
    owned: usize,
}

/// Create a handoff seeded with three values (one per slot).
///
/// Three are required rather than one clone because `T` need not be `Clone` —
/// and a caller building three explicitly is a caller who has noticed that this
/// costs three buffers' worth of memory.
pub fn latest_cell<T: Send>(a: T, b: T, c: T) -> (LatestWriter<T>, LatestReader<T>) {
    let shared = Arc::new(Shared {
        slots: [UnsafeCell::new(a), UnsafeCell::new(b), UnsafeCell::new(c)],
        // Slot 1 is published and stale; the writer owns 0 and the reader 2.
        published: AtomicUsize::new(1),
    });
    (
        LatestWriter {
            shared: Arc::clone(&shared),
            owned: 0,
        },
        LatestReader { shared, owned: 2 },
    )
}

impl<T> LatestWriter<T> {
    /// The slot to write the next value into. Private to this side until
    /// [`Self::publish`] hands it over.
    pub fn slot(&mut self) -> &mut T {
        // SAFETY: `owned` is this side's index and no other side can hold it.
        unsafe { &mut *self.shared.slots[self.owned].get() }
    }

    /// Publish the current slot and take ownership of another. Wait-free.
    pub fn publish(&mut self) {
        let prev = self
            .shared
            .published
            .swap(self.owned | FRESH_FLAG, Ordering::AcqRel);
        self.owned = prev & INDEX_MASK;
    }
}

impl<T> LatestReader<T> {
    /// Take the newest published value if there is one since the last fetch.
    ///
    /// Returns `false` when nothing new has been published — the slot then
    /// still holds the previous value, which stays valid. Wait-free.
    pub fn fetch(&mut self) -> bool {
        if self.shared.published.load(Ordering::Relaxed) & FRESH_FLAG == 0 {
            return false;
        }
        let prev = self.shared.published.swap(self.owned, Ordering::AcqRel);
        self.owned = prev & INDEX_MASK;
        true
    }

    /// The most recently fetched value.
    pub fn slot(&self) -> &T {
        // SAFETY: `owned` is this side's index and no other side can hold it.
        unsafe { &*self.shared.slots[self.owned].get() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reader_sees_nothing_until_the_writer_publishes() {
        let (mut w, mut r) = latest_cell(0u32, 0, 0);
        assert!(!r.fetch(), "fetch reported data before any publish");
        *w.slot() = 7;
        assert!(!r.fetch(), "writing without publishing must not be visible");
        w.publish();
        assert!(r.fetch());
        assert_eq!(*r.slot(), 7);
        assert!(!r.fetch(), "the same value must not read as fresh twice");
    }

    /// A reader that falls behind must see the newest value, not a backlog.
    #[test]
    fn reader_skips_to_the_newest_value() {
        let (mut w, mut r) = latest_cell(0u32, 0, 0);
        for v in 1..=10 {
            *w.slot() = v;
            w.publish();
        }
        assert!(r.fetch());
        assert_eq!(*r.slot(), 10, "reader should have skipped to the newest");
    }

    /// The reader's slot stays valid between publishes, so the UI can keep
    /// rendering the last snapshot rather than blanking.
    #[test]
    fn last_value_survives_an_empty_fetch() {
        let (mut w, mut r) = latest_cell(0u32, 0, 0);
        *w.slot() = 42;
        w.publish();
        r.fetch();
        assert!(!r.fetch());
        assert_eq!(*r.slot(), 42);
    }

    /// The three indices must always be distinct, or two sides would alias a
    /// slot and the unsafe access above would be unsound. Drive many handoffs
    /// in both orders and check the invariant every step.
    #[test]
    fn the_three_indices_stay_a_permutation() {
        let (mut w, mut r) = latest_cell(0u32, 0, 0);
        for i in 0..if cfg!(miri) { 60 } else { 500 } {
            if i % 3 != 0 {
                *w.slot() = i;
                w.publish();
            }
            if i % 2 == 0 {
                r.fetch();
            }
            let published = w.shared.published.load(Ordering::Relaxed) & INDEX_MASK;
            assert_ne!(w.owned, r.owned, "writer and reader alias a slot at {i}");
            assert_ne!(w.owned, published, "writer aliases the published slot at {i}");
            assert_ne!(r.owned, published, "reader aliases the published slot at {i}");
        }
    }

    /// Under real concurrency the reader must never observe a half-written
    /// value. Each published value is a vector whose elements all equal the
    /// same counter, so any tear shows up as a mismatched element.
    #[test]
    fn concurrent_handoff_never_tears() {
        const LEN: usize = 512;
        const ROUNDS: u32 = if cfg!(miri) { 200 } else { 20_000 };
        let (mut w, mut r) = latest_cell(
            vec![0u32; LEN],
            vec![0u32; LEN],
            vec![0u32; LEN],
        );

        let writer = std::thread::spawn(move || {
            for v in 1..=ROUNDS {
                let slot = w.slot();
                for e in slot.iter_mut() {
                    *e = v;
                }
                w.publish();
            }
        });

        let mut seen = 0u32;
        let mut fetches = 0u32;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while seen < ROUNDS && std::time::Instant::now() < deadline {
            if r.fetch() {
                fetches += 1;
                let slot = r.slot();
                let first = slot[0];
                assert!(
                    slot.iter().all(|&e| e == first),
                    "torn read: slot contains more than one generation"
                );
                assert!(first >= seen, "value went backwards: {first} after {seen}");
                seen = first;
            }
        }
        writer.join().expect("writer panicked");
        assert!(fetches > 0, "reader never observed a publish");
    }

    /// Neither side may be delayed by the other. Hammer the writer while
    /// timing the reader's worst single fetch.
    ///
    /// A wall-clock budget, so it means nothing under Miri's interpreter.
    #[test]
    #[cfg_attr(miri, ignore)]
    fn fetch_is_not_delayed_by_a_busy_writer() {
        use std::sync::atomic::AtomicBool;

        let (mut w, mut r) = latest_cell(vec![0u8; 8192], vec![0u8; 8192], vec![0u8; 8192]);
        let stop = Arc::new(AtomicBool::new(false));
        let stop_w = Arc::clone(&stop);
        let writer = std::thread::spawn(move || {
            let mut v = 0u8;
            while !stop_w.load(Ordering::Relaxed) {
                v = v.wrapping_add(1);
                w.slot().fill(v);
                w.publish();
            }
        });

        let mut worst = std::time::Duration::ZERO;
        for _ in 0..if cfg!(miri) { 200 } else { 20_000 } {
            let t0 = std::time::Instant::now();
            r.fetch();
            worst = worst.max(t0.elapsed());
        }
        stop.store(true, Ordering::Relaxed);
        writer.join().expect("writer panicked");

        // A lock here would show millisecond stalls under this contention.
        assert!(
            worst < std::time::Duration::from_millis(5),
            "worst fetch took {worst:?} — the reader is being delayed by the writer"
        );
    }
}
