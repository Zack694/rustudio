//! The shared layer every Roblox web request goes through (assets, places,
//! sessions, data stores, studio content), modelled on Studio's HTTP client:
//!
//! - **Host limits.** At most [`host_limit`] requests per host group are in
//!   flight; waiters are let in by [`Priority`] (Studio:
//!   `FFlagForceValidHttpRequestPriority`, per-category parallel limits).
//! - **Cooldowns.** A 429 (or 503 with `Retry-After`) pauses the whole host
//!   group until the server's time is up, with jitter (Studio:
//!   `DFFlagHttpAddJitterToRetryAndLocalThrottle`, local throttling on 503).
//! - **One retry policy** for callers that retry: [`transient_status`],
//!   [`backoff`] (exponential with jitter, `Retry-After` honoured).
//! - **Stats** per host group, for measuring the pipeline ([`stats`]).
//!
//! The limits, cooldowns and stats are a ureq middleware ([`middleware`]),
//! so every agent built with [`install`] gets them without its call sites
//! changing. (A middleware can send a request once, so retries stay with
//! the callers that know whether a request may be repeated.)

use std::collections::HashMap;
use std::sync::{Condvar, LazyLock, Mutex};
use std::time::{Duration, Instant};

use ureq::middleware::MiddlewareNext;
use ureq::{Body, SendBody};

/// How urgent a request is. Lower is sooner.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Priority {
    /// Someone is waiting on it now (a script's yield, a dialog, sign-in).
    Interactive = 0,
    /// Content on screen or needed to play.
    Visible = 1,
    /// Content asked for ahead of use (PreloadAsync).
    Prefetch = 2,
    /// Housekeeping (revalidation, telemetry-like lookups).
    Background = 3,
}

thread_local! {
    static PRIORITY: std::cell::Cell<Priority> = const { std::cell::Cell::new(Priority::Interactive) };
}

/// Runs `f` with this thread's requests at `priority`.
pub fn with_priority<T>(priority: Priority, f: impl FnOnce() -> T) -> T {
    let old = PRIORITY.with(|p| p.replace(priority));
    let r = f();
    PRIORITY.with(|p| p.set(old));
    r
}

fn current_priority() -> Priority {
    PRIORITY.with(|p| p.get())
}

/// The group a host's requests share limits and cooldowns with.
pub fn host_group(host: &str) -> String {
    let host = host.to_ascii_lowercase();
    // Asset bytes come from Roblox's CDNs (rbxcdn.com, contentdelivery).
    if host.ends_with(".rbxcdn.com") || host == "rbxcdn.com" || host == "contentdelivery.roblox.com" {
        "cdn".into()
    } else {
        host
    }
}

/// Concurrent requests allowed per host group.
pub fn host_limit(group: &str) -> usize {
    match group {
        // Asset bytes and their descriptors: the bulk of the traffic.
        "cdn" | "assetdelivery.roblox.com" => 16,
        _ => 8,
    }
}

/// Per host group counters.
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct HostStats {
    pub host: String,
    pub requests: u64,
    pub ok: u64,
    pub client_errors: u64,
    pub server_errors: u64,
    pub throttled: u64,
    pub transport_errors: u64,
    /// Content-Length of the responses that sent one.
    pub bytes: u64,
    /// Time to response headers, summed.
    pub ms: u64,
    pub max_ms: u64,
    /// Time spent waiting for a slot or a cooldown, summed.
    pub waited_ms: u64,
    pub peak_in_flight: usize,
}

#[derive(Default)]
struct Host {
    active: usize,
    waiting: [usize; 4],
    cooldown_until: Option<Instant>,
    stats: HostStats,
}

struct Hosts {
    hosts: Mutex<HashMap<String, Host>>,
    changed: Condvar,
}

static HOSTS: LazyLock<Hosts> = LazyLock::new(|| Hosts { hosts: Mutex::new(HashMap::new()), changed: Condvar::new() });

/// A slot in a host group, released on drop.
struct Slot {
    group: String,
}

impl Drop for Slot {
    fn drop(&mut self) {
        let mut hosts = HOSTS.hosts.lock().unwrap();
        if let Some(h) = hosts.get_mut(&self.group) {
            h.active = h.active.saturating_sub(1);
        }
        HOSTS.changed.notify_all();
    }
}

fn acquire(group: &str, priority: Priority) -> (Slot, Duration) {
    let t0 = Instant::now();
    let limit = host_limit(group);
    let mut hosts = HOSTS.hosts.lock().unwrap();
    hosts.entry(group.to_string()).or_insert_with(|| Host { stats: HostStats { host: group.to_string(), ..Default::default() }, ..Default::default() });
    hosts.get_mut(group).unwrap().waiting[priority as usize] += 1;
    loop {
        let h = hosts.get_mut(group).unwrap();
        let cooling = h.cooldown_until.filter(|t| *t > Instant::now());
        let higher_waiting = h.waiting[..priority as usize].iter().any(|n| *n > 0);
        if cooling.is_none() && h.active < limit && !higher_waiting {
            h.waiting[priority as usize] -= 1;
            h.active += 1;
            h.stats.peak_in_flight = h.stats.peak_in_flight.max(h.active);
            let waited = t0.elapsed();
            h.stats.waited_ms += waited.as_millis() as u64;
            return (Slot { group: group.to_string() }, waited);
        }
        hosts = match cooling {
            Some(until) => HOSTS.changed.wait_timeout(hosts, until.saturating_duration_since(Instant::now())).unwrap().0,
            None => HOSTS.changed.wait(hosts).unwrap(),
        };
    }
}

/// Pauses a host group (a 429, a 503 with Retry-After): nobody sends to it
/// until `delay` (jittered) has passed.
pub fn cool_down(group: &str, delay: Duration) {
    let until = Instant::now() + jitter(delay);
    let mut hosts = HOSTS.hosts.lock().unwrap();
    let h = hosts.entry(group.to_string()).or_insert_with(|| Host { stats: HostStats { host: group.to_string(), ..Default::default() }, ..Default::default() });
    h.cooldown_until = Some(h.cooldown_until.map_or(until, |t| t.max(until)));
    HOSTS.changed.notify_all();
}

fn record(group: &str, f: impl FnOnce(&mut HostStats)) {
    let mut hosts = HOSTS.hosts.lock().unwrap();
    if let Some(h) = hosts.get_mut(group) {
        f(&mut h.stats);
    }
}

/// The middleware: host limits, cooldowns and stats for every request an
/// agent built with [`install`] sends.
pub fn middleware(request: ureq::http::Request<SendBody>, next: MiddlewareNext) -> Result<ureq::http::Response<Body>, ureq::Error> {
    let group = host_group(request.uri().host().unwrap_or(""));
    let (slot, _waited) = acquire(&group, current_priority());
    let t0 = Instant::now();
    let result = next.handle(request);
    let ms = t0.elapsed().as_millis() as u64;
    drop(slot);
    match &result {
        Ok(response) => {
            let status = response.status().as_u16();
            let bytes = response
                .headers()
                .get("content-length")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(0);
            if status == 429 || (status == 503 && response.headers().contains_key("retry-after")) {
                let delay = response
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| retry_after(v, std::time::SystemTime::now()))
                    .unwrap_or(Duration::from_secs(1));
                cool_down(&group, delay);
            }
            record(&group, |s| {
                s.requests += 1;
                s.ms += ms;
                s.max_ms = s.max_ms.max(ms);
                s.bytes += bytes;
                match status {
                    429 => s.throttled += 1,
                    200..=399 => s.ok += 1,
                    400..=499 => s.client_errors += 1,
                    _ => s.server_errors += 1,
                }
            });
        }
        Err(_) => record(&group, |s| {
            s.requests += 1;
            s.ms += ms;
            s.transport_errors += 1;
        }),
    }
    result
}

/// Adds the shared layer to an agent's configuration.
pub fn install(
    config: ureq::config::ConfigBuilder<ureq::typestate::AgentScope>,
) -> ureq::config::ConfigBuilder<ureq::typestate::AgentScope> {
    config.middleware(middleware)
}

/// Statuses worth asking again for (the server didn't do the work, or
/// couldn't now): request timeout, rate limit, server errors.
pub fn transient_status(status: u16) -> bool {
    matches!(status, 408 | 429 | 500 | 502 | 503 | 504)
}

/// Transport failures worth retrying (the request may never have arrived):
/// timeouts, refused or reset connections, DNS hiccups.
pub fn transient_error(error: &ureq::Error) -> bool {
    matches!(
        error,
        ureq::Error::Timeout(_)
            | ureq::Error::Io(_)
            | ureq::Error::ConnectionFailed
            | ureq::Error::HostNotFound
            | ureq::Error::Protocol(_)
    )
}

const BACKOFF_BASE: Duration = Duration::from_millis(500);
const BACKOFF_MAX: Duration = Duration::from_secs(8);
const MAX_RETRY_AFTER: Duration = Duration::from_secs(60);

/// The wait before retry number `attempt` (0-based): exponential from half a
/// second, capped at 8 s, jittered ±25 %; at least what `Retry-After` asks.
pub fn backoff(attempt: u32, retry_after_header: Option<&str>) -> Duration {
    let exp = BACKOFF_BASE.saturating_mul(1u32 << attempt.min(8)).min(BACKOFF_MAX);
    let asked = retry_after_header.and_then(|v| retry_after(v, std::time::SystemTime::now())).unwrap_or_default();
    jitter(exp).max(asked)
}

/// `Retry-After`: seconds or an HTTP date; capped at a minute.
pub fn retry_after(value: &str, now: std::time::SystemTime) -> Option<Duration> {
    let value = value.trim();
    let delay = if let Ok(seconds) = value.parse::<u64>() {
        Duration::from_secs(seconds)
    } else {
        let date = httpdate(value)?;
        date.duration_since(now).unwrap_or_default()
    };
    Some(delay.min(MAX_RETRY_AFTER))
}

/// An IMF-fixdate ("Sun, 06 Nov 1994 08:49:37 GMT"), the form servers send.
fn httpdate(s: &str) -> Option<std::time::SystemTime> {
    let parts: Vec<&str> = s.split_whitespace().collect();
    if parts.len() != 6 || parts[5] != "GMT" {
        return None;
    }
    let day: u64 = parts[1].parse().ok()?;
    let month = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"]
        .iter()
        .position(|m| *m == parts[2])? as u64
        + 1;
    let year: i64 = parts[3].parse().ok()?;
    let hms: Vec<u64> = parts[4].split(':').filter_map(|x| x.parse().ok()).collect();
    if hms.len() != 3 {
        return None;
    }
    // Days from the civil date (Howard Hinnant's algorithm).
    let (y, m) = if month <= 2 { (year - 1, month + 9) } else { (year, month - 3) };
    let era = y.div_euclid(400);
    let yoe = (y - era * 400) as u64;
    let doy = (153 * m + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe as i64 - 719_468;
    let secs = days * 86_400 + (hms[0] * 3600 + hms[1] * 60 + hms[2]) as i64;
    Some(std::time::UNIX_EPOCH + Duration::from_secs(u64::try_from(secs).ok()?))
}

/// `d` ±25 %: retries and cooldowns spread out instead of arriving together.
pub fn jittered(d: Duration) -> Duration {
    jitter(d)
}

/// ±25 %, from a cheap per-call source (no RNG dependency needed).
fn jitter(d: Duration) -> Duration {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(Instant::now().elapsed().as_nanos());
    let r = (h.finish() % 1000) as f64 / 1000.0; // 0..1
    d.mul_f64(0.75 + 0.5 * r)
}

/// Counters per host group since start-up or the last [`reset_stats`].
pub fn stats() -> Vec<HostStats> {
    let hosts = HOSTS.hosts.lock().unwrap();
    let mut v: Vec<HostStats> = hosts.values().map(|h| h.stats.clone()).collect();
    v.sort_by(|a, b| b.requests.cmp(&a.requests));
    v
}

pub fn reset_stats() {
    let mut hosts = HOSTS.hosts.lock().unwrap();
    for (group, h) in hosts.iter_mut() {
        h.stats = HostStats { host: group.clone(), ..Default::default() };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_and_limits() {
        assert_eq!(host_group("c4.rbxcdn.com"), "cdn");
        assert_eq!(host_group("contentdelivery.roblox.com"), "cdn");
        assert_eq!(host_group("AssetDelivery.Roblox.com"), "assetdelivery.roblox.com");
        assert_eq!(host_limit("cdn"), 16);
        assert_eq!(host_limit("apis.roblox.com"), 8);
    }

    #[test]
    fn retry_policy() {
        for s in [408, 429, 500, 502, 503, 504] {
            assert!(transient_status(s), "{s}");
        }
        for s in [200, 301, 400, 401, 403, 404, 409] {
            assert!(!transient_status(s), "{s}");
        }
        let b0 = backoff(0, None);
        assert!(b0 >= Duration::from_millis(375) && b0 <= Duration::from_millis(625), "{b0:?}");
        assert!(backoff(10, None) <= Duration::from_secs(10));
        assert!(backoff(0, Some("5")) >= Duration::from_secs(5));
        assert_eq!(retry_after("120", std::time::SystemTime::now()), Some(Duration::from_secs(60)));
        let now = httpdate("Sun, 06 Nov 1994 08:49:37 GMT").unwrap();
        assert_eq!(retry_after("Sun, 06 Nov 1994 08:49:47 GMT", now), Some(Duration::from_secs(10)));
        assert_eq!(now.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(), 784_111_777);
    }

    #[test]
    fn priority_orders_waiters_and_limits_hold() {
        let group = "test.limits.invalid";
        let limit = host_limit(group);
        let slots: Vec<_> = (0..limit).map(|_| acquire(group, Priority::Background).0).collect();
        let order = std::sync::Arc::new(Mutex::new(Vec::new()));
        let mut threads = Vec::new();
        for (i, p) in [Priority::Background, Priority::Interactive, Priority::Prefetch].into_iter().enumerate() {
            let order = order.clone();
            threads.push(std::thread::spawn(move || {
                let _ = i;
                let (_slot, _) = acquire(group, p);
                order.lock().unwrap().push(p);
                std::thread::sleep(Duration::from_millis(30));
            }));
        }
        // Wait until all three are queued, then free every slot at once:
        // the most urgent goes first.
        loop {
            let queued: usize = HOSTS.hosts.lock().unwrap().get(group).map_or(0, |h| h.waiting.iter().sum());
            if queued == 3 {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        drop(slots);
        for t in threads {
            t.join().unwrap();
        }
        let order = order.lock().unwrap();
        assert_eq!(order[0], Priority::Interactive, "{order:?}");
    }

    #[test]
    fn cooldown_holds_new_requests() {
        let group = "test.cooldown.invalid";
        cool_down(group, Duration::from_millis(200));
        let t0 = Instant::now();
        let (_slot, _) = acquire(group, Priority::Interactive);
        assert!(t0.elapsed() >= Duration::from_millis(140), "{:?}", t0.elapsed());
    }
}
