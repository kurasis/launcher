//! Counts kept in memory and written to the database every few seconds
//! (`FLUSH_EVERY`): a burst of requests costs map updates, not disk writes.
//! A failed write keeps its counts for the next one.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use chrono::NaiveDate;

use crate::model::{AdRead, CountsRead, Period, PingRead};

pub const FLUSH_EVERY: Duration = Duration::from_secs(10);
/// Different rows waiting at most; beyond them new rows are lost (and
/// counted), so a database that cannot be written cannot fill the memory.
const MAX_KEYS: usize = 400_000;

/// One row of the database and what to add to it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Key {
    Player {
        day: NaiveDate,
        period: Period,
        cut: &'static str,
        value: String,
    },
    Ad {
        day: NaiveDate,
        ad: i64,
        event: &'static str,
        cut: &'static str,
        value: String,
    },
    /// What was refused or left out, by a fixed word.
    Dropped {
        day: NaiveDate,
        reason: &'static str,
    },
}

/// What becomes of one ad's counts in a batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fate {
    Counted,
    /// The ad did not show that day: left out.
    Unknown,
    /// Past its network's hour: only its impressions are kept, apart, as
    /// `burst`.
    Burst,
}

#[derive(Default)]
pub struct Tally {
    counts: Mutex<HashMap<Key, i64>>,
    /// Rows lost to `MAX_KEYS` since it was last asked.
    lost: AtomicU64,
}

impl Tally {
    fn map(&self) -> std::sync::MutexGuard<'_, HashMap<Key, i64>> {
        self.counts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn add_to(&self, map: &mut HashMap<Key, i64>, key: Key, n: i64) {
        if n <= 0 {
            return;
        }
        let full = map.len() >= MAX_KEYS;
        match map.get_mut(&key) {
            Some(count) => *count = count.saturating_add(n),
            None if full => {
                self.lost.fetch_add(1, Ordering::Relaxed);
            }
            None => {
                map.insert(key, n);
            }
        }
    }

    pub fn dropped(&self, day: NaiveDate, reason: &'static str) {
        let mut map = self.map();
        self.add_to(&mut map, Key::Dropped { day, reason }, 1);
    }

    /// One launcher's day: one in each cut of each period it counts for.
    pub fn ping(&self, read: &PingRead) {
        let mut map = self.map();
        for period in &read.periods {
            for (cut, value) in &read.cuts {
                let key = Key::Player {
                    day: read.day,
                    period: *period,
                    cut,
                    value: value.clone(),
                };
                self.add_to(&mut map, key, 1);
            }
        }
    }

    /// A batch of ad counts, each ad as `fate` says. Returns how many ads
    /// were not counted.
    pub fn counts(
        &self,
        read: &CountsRead,
        country: &str,
        mut fate: impl FnMut(&AdRead) -> Fate,
    ) -> usize {
        let mut map = self.map();
        let mut left = 0;
        for ad in &read.ads {
            let reason = match fate(ad) {
                Fate::Counted => None,
                Fate::Unknown => Some("unknown_ad"),
                Fate::Burst => {
                    let key = Key::Ad {
                        day: read.day,
                        ad: ad.ad,
                        event: "burst",
                        cut: "all",
                        value: String::new(),
                    };
                    self.add_to(&mut map, key, i64::from(ad.shown()));
                    Some("burst")
                }
            };
            if let Some(reason) = reason {
                left += 1;
                let key = Key::Dropped {
                    day: read.day,
                    reason,
                };
                self.add_to(&mut map, key, 1);
                continue;
            }
            for (event, n) in &ad.events {
                for (cut, value) in [("all", ""), ("country", country), ("ui", &read.ui)] {
                    let key = Key::Ad {
                        day: read.day,
                        ad: ad.ad,
                        event,
                        cut,
                        value: value.to_owned(),
                    };
                    self.add_to(&mut map, key, i64::from(*n));
                }
            }
            for (version, n) in &ad.picked {
                let key = Key::Ad {
                    day: read.day,
                    ad: ad.ad,
                    event: "picked",
                    cut: "version",
                    value: version.clone(),
                };
                self.add_to(&mut map, key, i64::from(*n));
            }
        }
        left
    }

    /// Everything counted since the last call.
    pub fn take(&self) -> HashMap<Key, i64> {
        std::mem::take(&mut *self.map())
    }

    /// Counts a failed write could not keep, for the next one.
    pub fn put_back(&self, counts: HashMap<Key, i64>) {
        let mut map = self.map();
        for (key, n) in counts {
            self.add_to(&mut map, key, n);
        }
    }

    /// Rows lost since the last call.
    pub fn take_lost(&self) -> u64 {
        self.lost.swap(0, Ordering::Relaxed)
    }
}
