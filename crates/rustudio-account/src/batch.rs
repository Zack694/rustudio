//! Many single lookups as few batch requests, the way Studio's request
//! orchestration does it (BatchThumbnailFetcher, the users and product-info
//! batchers): a lookup is queued, the dispatcher waits a few milliseconds for
//! more, then sends up to `limit` keys in one request (at most `max_in_flight`
//! requests at a time). Answers are kept for a while, so asking again is
//! free; a lookup already queued or in flight isn't asked twice.
//!
//! [`asset_batch`](crate::asset_batch) is the asset-location special case
//! (per-session requests, signed-URL expiry, typed errors).

use std::collections::{HashMap, VecDeque};
use std::hash::Hash;
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

/// One key's answer from a batch request, kept for `ttl` (zero: handed to
/// whoever is waiting, then asked again next time).
pub struct Answer<V> {
    pub value: Result<V, String>,
    pub ttl: Duration,
}

/// A batch request: the keys asked for, and what came back for each. A key
/// left out of the reply gets `missing`'s error; an `Err` for the whole
/// request answers every key with it (not kept).
pub type Fetch<K, V> = fn(&[K]) -> Result<Vec<(K, Answer<V>)>, String>;

pub struct Batcher<K, V> {
    name: &'static str,
    limit: usize,
    max_in_flight: usize,
    window: Duration,
    fetch: Fetch<K, V>,
    missing: &'static str,
    missing_ttl: Duration,
    state: Mutex<State<K, V>>,
    changed: Condvar,
}

enum Entry<V> {
    Pending,
    /// The answer, kept until the instant; once past it, it's handed out
    /// until someone has had it (`served`), then asked anew.
    Done(Result<V, String>, Instant, bool),
}

struct State<K, V> {
    queue: VecDeque<K>,
    entries: HashMap<K, Entry<V>>,
    in_flight: usize,
    started: bool,
    batches: u64,
    asked: u64,
    hits: u64,
}

/// A batcher's counters: batch requests sent, keys they carried, lookups
/// answered from what was kept.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct Counters {
    pub batches: u64,
    pub asked: u64,
    pub hits: u64,
}

impl<K, V> Batcher<K, V>
where
    K: Clone + Eq + Hash + Send + 'static,
    V: Clone + Send + 'static,
{
    pub fn new(
        name: &'static str,
        limit: usize,
        max_in_flight: usize,
        window: Duration,
        fetch: Fetch<K, V>,
        missing: &'static str,
        missing_ttl: Duration,
    ) -> Self {
        Batcher {
            name,
            limit,
            max_in_flight,
            window,
            fetch,
            missing,
            missing_ttl,
            state: Mutex::new(State {
                queue: VecDeque::new(),
                entries: HashMap::new(),
                in_flight: 0,
                started: false,
                batches: 0,
                asked: 0,
                hits: 0,
            }),
            changed: Condvar::new(),
        }
    }

    /// `key`'s answer if there is one; otherwise queues it (once) and
    /// returns `None` at once. For callers that poll (a script's yield).
    pub fn poll(&'static self, key: &K) -> Option<Result<V, String>> {
        let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(v) = Self::kept(&mut st, key) {
            return Some(v);
        }
        self.enqueue(&mut st, key);
        None
    }

    /// `key`'s answer, waiting for its batch if need be.
    pub fn get(&'static self, key: &K) -> Result<V, String> {
        let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(v) = Self::kept(&mut st, key) {
            return v;
        }
        loop {
            match st.entries.get_mut(key) {
                Some(Entry::Done(v, _, served)) => {
                    *served = true;
                    return v.clone();
                }
                Some(Entry::Pending) => {}
                // (An answer that isn't kept, gone before this waiter woke.)
                None => self.enqueue(&mut st, key),
            }
            st = self.changed.wait(st).unwrap_or_else(|e| e.into_inner());
        }
    }

    pub fn counters(&'static self) -> Counters {
        let st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        Counters { batches: st.batches, asked: st.asked, hits: st.hits }
    }

    fn kept(st: &mut State<K, V>, key: &K) -> Option<Result<V, String>> {
        match st.entries.get_mut(key) {
            Some(Entry::Done(v, until, served)) => {
                if *until > Instant::now() {
                    st.hits += 1;
                    return Some(v.clone());
                }
                if *served {
                    st.entries.remove(key);
                    return None;
                }
                *served = true;
                Some(v.clone())
            }
            _ => None,
        }
    }

    fn enqueue(&'static self, st: &mut State<K, V>, key: &K) {
        if st.entries.contains_key(key) {
            return;
        }
        st.entries.insert(key.clone(), Entry::Pending);
        st.queue.push_back(key.clone());
        if !st.started {
            st.started = true;
            let _ = std::thread::Builder::new().name(format!("batch-{}", self.name)).spawn(move || self.dispatch());
        }
        self.changed.notify_all();
    }

    fn dispatch(&'static self) {
        loop {
            let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
            while st.queue.is_empty() || st.in_flight >= self.max_in_flight {
                st = self.changed.wait(st).unwrap_or_else(|e| e.into_inner());
            }
            // A short window for the rest of a burst, unless it's full.
            if st.queue.len() < self.limit {
                drop(st);
                std::thread::sleep(self.window);
                st = self.state.lock().unwrap_or_else(|e| e.into_inner());
            }
            let n = st.queue.len().min(self.limit);
            let keys: Vec<K> = st.queue.drain(..n).collect();
            if keys.is_empty() {
                continue;
            }
            st.in_flight += 1;
            st.batches += 1;
            st.asked += keys.len() as u64;
            drop(st);
            let _ = std::thread::Builder::new().name(format!("batch-{}-req", self.name)).spawn(move || self.send(keys));
        }
    }

    fn send(&'static self, keys: Vec<K>) {
        let reply = (self.fetch)(&keys);
        let now = Instant::now();
        let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        st.in_flight -= 1;
        match reply {
            Ok(answers) => {
                let mut answered: HashMap<K, Answer<V>> = answers.into_iter().collect();
                for k in keys {
                    let (value, ttl) = match answered.remove(&k) {
                        Some(a) => (a.value, a.ttl),
                        None => (Err(self.missing.to_string()), self.missing_ttl),
                    };
                    st.entries.insert(k, Entry::Done(value, now + ttl, false));
                }
            }
            Err(e) => {
                for k in keys {
                    st.entries.insert(k, Entry::Done(Err(e.clone()), now, false));
                }
            }
        }
        self.changed.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static CALLS: AtomicUsize = AtomicUsize::new(0);

    fn double(keys: &[u32]) -> Result<Vec<(u32, Answer<u32>)>, String> {
        CALLS.fetch_add(1, Ordering::SeqCst);
        Ok(keys
            .iter()
            .filter(|k| **k != 13)
            .map(|k| (*k, Answer { value: Ok(k * 2), ttl: Duration::from_secs(60) }))
            .collect())
    }

    static DOUBLER: std::sync::LazyLock<Batcher<u32, u32>> = std::sync::LazyLock::new(|| {
        Batcher::new("test", 100, 2, Duration::from_millis(20), double, "unknown", Duration::from_secs(60))
    });

    #[test]
    fn a_burst_is_one_request_and_answers_are_kept() {
        let threads: Vec<_> = (0..50u32).map(|k| std::thread::spawn(move || DOUBLER.get(&k))).collect();
        let got: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
        assert_eq!(got[7], Ok(14));
        assert!(CALLS.load(Ordering::SeqCst) <= 2, "{} requests for one burst", CALLS.load(Ordering::SeqCst));
        let before = CALLS.load(Ordering::SeqCst);
        assert_eq!(DOUBLER.poll(&7), Some(Ok(14)));
        assert_eq!(CALLS.load(Ordering::SeqCst), before, "a kept answer asks nothing");
        assert_eq!(DOUBLER.get(&13), Err("unknown".into()));
        assert!(DOUBLER.counters().hits >= 1);
    }
}
