//! Batched asset resolution: where an asset's bytes are (its CDN location),
//! for many assets per request, as Studio's AssetResolutionWorkflow does
//! (`POST assetdelivery.roblox.com/v1/assets/batch`, up to
//! `DFIntHttpBatchLimit` = 128 assets a request, `DFIntHttpBatchApi_maxReqs`
//! = 4 requests at a time).
//!
//! Resolution is apart from downloading: an asset is [`prefetch`]ed the
//! moment it's wanted, so the resolver sees the whole backlog and batches
//! it, while the download workers only fetch bytes from the CDN. A
//! [`resolve`] that finds its asset resolved (or in flight) doesn't ask
//! again. Locations are kept until the expiry signed into their URL (the
//! location half of the two-tier asset cache).

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Condvar, LazyLock, Mutex};
use std::time::{Duration, Instant, SystemTime};

use crate::api::AssetDownloadError;
use crate::{Error, Session};

pub const BATCH_URL: &str = "https://assetdelivery.roblox.com/v1/assets/batch";
/// Assets per batch request (Studio: DFIntHttpBatchLimit).
pub const BATCH_LIMIT: usize = 128;
/// Batch requests in flight (Studio: DFIntHttpBatchApi_maxReqs).
pub const MAX_BATCHES: usize = 4;
/// How long the resolver waits for more assets before sending a batch.
const WINDOW: Duration = Duration::from_millis(12);
/// A refusal (private, moderated, missing) holds this long, then is asked
/// again (sign-in or permission changes).
const REFUSAL_TTL: Duration = Duration::from_secs(600);
/// A location is used until this long before its signed expiry.
const EXPIRY_MARGIN: Duration = Duration::from_secs(120);

/// Where an asset's bytes are.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolved {
    pub location: String,
    pub asset_type: u32,
}

#[derive(Clone, Debug)]
enum Outcome {
    Ok(Resolved),
    Refused(String),
    Transient(u16, Option<String>, String),
    Other(String),
}

impl Outcome {
    fn into_result(self) -> Result<Resolved, AssetDownloadError> {
        match self {
            Outcome::Ok(r) => Ok(r),
            Outcome::Refused(m) => Err(AssetDownloadError::Refused(Error(m))),
            Outcome::Transient(status, retry_after, m) => Err(AssetDownloadError::Transient {
                status,
                retry_after: retry_after.and_then(|v| ureq::http::HeaderValue::from_str(&v).ok()),
                error: Error(m),
            }),
            Outcome::Other(m) => Err(AssetDownloadError::Other(Error(m))),
        }
    }
}

/// One asset in one context: the place it's asked for (`Roblox-Place-Id`
/// changes what a collaborator may see) and whether it's asked signed in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Key {
    asset: u64,
    version: Option<u64>,
    place: u64,
}

enum Entry {
    Pending,
    Done { outcome: Outcome, until: Instant },
}

struct Request {
    key: Key,
    session: Arc<Session>,
}

#[derive(Default)]
struct State {
    queue: VecDeque<Request>,
    entries: HashMap<Key, Entry>,
    batches_in_flight: usize,
    /// Counters for measuring: batch requests sent, assets they resolved,
    /// resolves answered from the cache.
    batches: u64,
    resolved: u64,
    cache_hits: u64,
}

struct Resolver {
    state: Mutex<State>,
    changed: Condvar,
}

static RESOLVER: LazyLock<Resolver> = LazyLock::new(|| {
    std::thread::Builder::new()
        .name("asset-resolver".into())
        .spawn(dispatch_loop)
        .expect("asset resolver thread");
    Resolver { state: Mutex::new(State::default()), changed: Condvar::new() }
});

fn key(asset: u64, version: Option<u64>) -> Key {
    Key { asset, version, place: crate::session::PLACE_CONTEXT.load(std::sync::atomic::Ordering::Relaxed) }
}

/// Enqueues `asset` for resolution (if it isn't resolved or on its way) and
/// returns at once.
pub fn prefetch(session: &Arc<Session>, asset: u64, version: Option<u64>) {
    let r = &*RESOLVER;
    let k = key(asset, version);
    let mut st = r.state.lock().unwrap();
    if usable(&mut st, k).is_some() || matches!(st.entries.get(&k), Some(Entry::Pending)) {
        return;
    }
    st.entries.insert(k, Entry::Pending);
    st.queue.push_back(Request { key: k, session: session.clone() });
    r.changed.notify_all();
}

/// Where `asset` is: from the cache, from a batch already on its way, or
/// from a new one (this blocks until it's answered).
pub fn resolve(session: &Arc<Session>, asset: u64, version: Option<u64>) -> Result<Resolved, AssetDownloadError> {
    let r = &*RESOLVER;
    let k = key(asset, version);
    let mut st = r.state.lock().unwrap();
    if let Some(done) = usable(&mut st, k) {
        st.cache_hits += 1;
        return done.into_result();
    }
    if !matches!(st.entries.get(&k), Some(Entry::Pending)) {
        st.entries.insert(k, Entry::Pending);
        st.queue.push_back(Request { key: k, session: session.clone() });
        r.changed.notify_all();
    }
    loop {
        if let Some(Entry::Done { outcome, .. }) = st.entries.get(&k) {
            let outcome = outcome.clone();
            // A transient failure isn't kept: the caller's retry asks again.
            if matches!(outcome, Outcome::Transient(..)) {
                st.entries.remove(&k);
            }
            return outcome.into_result();
        }
        if st.entries.get(&k).is_none() {
            return Err(AssetDownloadError::Other(Error(format!("asset {asset}: resolution was dropped"))));
        }
        st = r.changed.wait(st).unwrap();
    }
}

/// The location already resolved for `asset` (no request), if any.
pub fn peek(asset: u64, version: Option<u64>) -> Option<Resolved> {
    let mut st = RESOLVER.state.lock().unwrap();
    match usable(&mut st, key(asset, version)) {
        Some(Outcome::Ok(r)) => Some(r),
        _ => None,
    }
}

/// The CDN's content key in a location (`…/v2/sc7/<32 hex>?…`): it changes
/// when the asset's bytes do, so a cached copy filed under the same key is
/// current.
pub fn content_key(location: &str) -> Option<String> {
    let path = location.split('?').next()?;
    let last = path.rsplit('/').next()?;
    (last.len() == 32 && last.bytes().all(|b| b.is_ascii_hexdigit())).then(|| last.to_ascii_lowercase())
}

/// A finished, unexpired answer for `k` (expired ones are forgotten).
fn usable(st: &mut State, k: Key) -> Option<Outcome> {
    match st.entries.get(&k) {
        Some(Entry::Done { outcome, until }) if *until > Instant::now() && !matches!(outcome, Outcome::Transient(..)) => {
            Some(outcome.clone())
        }
        Some(Entry::Done { .. }) => {
            st.entries.remove(&k);
            None
        }
        _ => None,
    }
}

/// Counters: (batch requests, assets resolved by them, cache hits).
pub fn counters() -> (u64, u64, u64) {
    let st = RESOLVER.state.lock().unwrap();
    (st.batches, st.resolved, st.cache_hits)
}

fn dispatch_loop() {
    // (RESOLVER is being initialised when this starts: wait for it.)
    let r = loop {
        if let Some(r) = LazyLock::get(&RESOLVER) {
            break r;
        }
        std::thread::sleep(Duration::from_millis(1));
    };
    loop {
        let batch = {
            let mut st = r.state.lock().unwrap();
            while st.queue.is_empty() || st.batches_in_flight >= MAX_BATCHES {
                st = r.changed.wait(st).unwrap();
            }
            // Let more arrive (a scene's worth of assets queues in a burst).
            if st.queue.len() < BATCH_LIMIT {
                drop(st);
                std::thread::sleep(WINDOW);
                st = r.state.lock().unwrap();
            }
            // One context per batch: the first request's place.
            let place = st.queue.front().map(|q| q.key.place).unwrap_or(0);
            let mut batch = Vec::new();
            let mut rest = VecDeque::new();
            while let Some(q) = st.queue.pop_front() {
                if batch.len() < BATCH_LIMIT && q.key.place == place {
                    batch.push(q);
                } else {
                    rest.push_back(q);
                }
            }
            st.queue = rest;
            st.batches_in_flight += 1;
            batch
        };
        if batch.is_empty() {
            let mut st = r.state.lock().unwrap();
            st.batches_in_flight -= 1;
            continue;
        }
        std::thread::Builder::new()
            .name("asset-batch".into())
            .spawn(move || {
                let keys: Vec<Key> = batch.iter().map(|q| q.key).collect();
                let outcomes = crate::web::with_priority(crate::web::Priority::Visible, || send(&batch));
                let now = Instant::now();
                let mut st = r.state.lock().unwrap();
                st.batches += 1;
                st.batches_in_flight -= 1;
                for (k, outcome) in keys.into_iter().zip(outcomes) {
                    let until = match &outcome {
                        Outcome::Ok(res) => now + location_ttl(&res.location),
                        Outcome::Refused(_) => now + REFUSAL_TTL,
                        _ => now,
                    };
                    if matches!(outcome, Outcome::Ok(_)) {
                        st.resolved += 1;
                    }
                    st.entries.insert(k, Entry::Done { outcome, until });
                }
                r.changed.notify_all();
            })
            .expect("asset batch thread");
    }
}

/// One batch request; an outcome per request, in order.
fn send(batch: &[Request]) -> Vec<Outcome> {
    let body = serde_json::Value::Array(
        batch
            .iter()
            .enumerate()
            .map(|(i, q)| {
                let mut item = serde_json::json!({ "assetId": q.key.asset, "requestId": i.to_string() });
                if let Some(v) = q.key.version {
                    item["version"] = serde_json::json!(v);
                }
                item
            })
            .collect(),
    )
    .to_string();
    let place = batch[0].key.place.to_string();
    let extra: Vec<(&str, &str)> = if batch[0].key.place != 0 { vec![("Roblox-Place-Id", place.as_str())] } else { Vec::new() };
    let all = |o: Outcome| batch.iter().map(|_| o.clone()).collect::<Vec<_>>();
    let (status, text) = match batch[0].session.post_csrf_with(BATCH_URL, "application/json", body.as_bytes(), &extra) {
        Ok(r) => r,
        Err(e) => return all(Outcome::Transient(0, None, format!("asset batch: {e}"))),
    };
    if status != 200 {
        let m = format!("asset batch returned HTTP {status}");
        return all(if crate::web::transient_status(status) { Outcome::Transient(status, None, m) } else { Outcome::Other(m) });
    }
    parse(&text, batch.iter().map(|q| q.key.asset).collect())
}

/// The batch answer, by requestId (the index in the request).
fn parse(text: &str, assets: Vec<u64>) -> Vec<Outcome> {
    let mut out: Vec<Outcome> = assets
        .iter()
        .map(|a| Outcome::Other(format!("asset {a}: missing from the asset batch answer")))
        .collect();
    let Ok(serde_json::Value::Array(items)) = serde_json::from_str::<serde_json::Value>(text) else {
        return out.into_iter().map(|_| Outcome::Transient(0, None, "asset batch: unreadable answer".into())).collect();
    };
    for item in items {
        let Some(i) = item.get("requestId").and_then(|v| v.as_str()).and_then(|s| s.parse::<usize>().ok()) else {
            continue;
        };
        let Some(asset) = assets.get(i) else { continue };
        let asset_type = item.get("assetTypeId").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        out[i] = if let Some(loc) = item.get("location").and_then(|v| v.as_str()) {
            Outcome::Ok(Resolved { location: loc.to_string(), asset_type })
        } else {
            let e = item.pointer("/errors/0");
            let code = e.and_then(|e| e.get("code")).and_then(|c| c.as_u64()).unwrap_or(0) as u16;
            let msg = e.and_then(|e| e.get("message")).and_then(|m| m.as_str()).unwrap_or("no download location");
            let m = format!("asset {asset}: asset delivery: {msg} ({code})");
            match code {
                c if crate::web::transient_status(c) => Outcome::Transient(c, None, m),
                // Not allowed, not found, moderated, archived: a refusal.
                401 | 403 | 404 | 409 | 410 => Outcome::Refused(m),
                _ => Outcome::Other(m),
            }
        };
    }
    out
}

/// How long a signed CDN location may be used: until its `exp` /
/// `Expires`, less a margin; ten minutes when it carries none.
fn location_ttl(location: &str) -> Duration {
    let exp = ["exp=", "Expires="].iter().find_map(|k| {
        let at = location.find(k)? + k.len();
        let digits: String = location[at..].chars().take_while(|c| c.is_ascii_digit()).collect();
        digits.parse::<u64>().ok()
    });
    match exp {
        Some(exp) => {
            let now = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default().as_secs();
            Duration::from_secs(exp.saturating_sub(now)).saturating_sub(EXPIRY_MARGIN)
        }
        None => Duration::from_secs(600),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_locations_and_errors_by_request_id() {
        let text = r#"[
            {"requestId":"1","errors":[{"code":404,"message":"Request asset was not found"}]},
            {"requestId":"0","location":"https://fts.rbxcdn.com/x?exp=4102444800","assetTypeId":4},
            {"requestId":"2","errors":[{"code":409,"message":"User is not authorized to access Asset."}]},
            {"requestId":"3","errors":[{"code":429,"message":"Too many requests"}]}
        ]"#;
        let out = parse(text, vec![10, 11, 12, 13, 14]);
        assert!(matches!(&out[0], Outcome::Ok(r) if r.asset_type == 4 && r.location.contains("rbxcdn")));
        assert!(matches!(&out[1], Outcome::Refused(m) if m.contains("asset 11")));
        assert!(matches!(&out[2], Outcome::Refused(_)));
        assert!(matches!(&out[3], Outcome::Transient(429, ..)));
        assert!(matches!(&out[4], Outcome::Other(m) if m.contains("missing")));
    }

    #[test]
    fn content_key_is_the_cdn_hash() {
        assert_eq!(
            content_key("https://fts.rbxcdn.com/v2/sc7/D1507cd356d1844680c1e9078798a7df?encoding=gzip&version=1").as_deref(),
            Some("d1507cd356d1844680c1e9078798a7df")
        );
        assert_eq!(content_key("https://c0.rbxcdn.com/0123456789abcdef0123456789abcdef").as_deref(), Some("0123456789abcdef0123456789abcdef"));
        assert_eq!(content_key("https://c/x/short?y"), None);
    }

    #[test]
    fn location_ttl_follows_the_signed_expiry() {
        let now = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs();
        let ttl = location_ttl(&format!("https://c.rbxcdn.com/a?__token__=key-id=1~exp={}~acl=x", now + 3600));
        assert!(ttl > Duration::from_secs(3300) && ttl <= Duration::from_secs(3600 - 120 + 1), "{ttl:?}");
        assert_eq!(location_ttl(&format!("https://c/x?Expires={}", now.saturating_sub(10))), Duration::ZERO);
        assert_eq!(location_ttl("https://c/x"), Duration::from_secs(600));
    }
}
