//! User names and ids, batched: `POST users.roblox.com/v1/users` (ids to
//! names) and `/v1/usernames/users` (names to ids), up to 100 a request, the
//! endpoints Studio's engine names for GetNameFromUserIdAsync and
//! GetUserIdFromNameAsync. A leaderboard naming 100 players is one request,
//! not 100, and a name asked again is answered from memory.

use std::sync::LazyLock;
use std::time::Duration;

use crate::batch::{Answer, Batcher, Counters};

pub const USERS_URL: &str = "https://users.roblox.com/v1/users";
pub const USERNAMES_URL: &str = "https://users.roblox.com/v1/usernames/users";
pub const UNKNOWN: &str = "Unknown user";
const KEEP: Duration = Duration::from_secs(600);

/// A user's name and display name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct User {
    pub name: String,
    pub display_name: String,
}

static BY_ID: LazyLock<Batcher<u64, User>> =
    LazyLock::new(|| Batcher::new("users", 100, 2, Duration::from_millis(15), fetch_ids, UNKNOWN, KEEP));
static BY_NAME: LazyLock<Batcher<String, u64>> =
    LazyLock::new(|| Batcher::new("usernames", 100, 2, Duration::from_millis(15), fetch_names, UNKNOWN, KEEP));

/// `id`'s user, or None while its batch is on the way.
pub fn poll_user(id: u64) -> Option<Result<User, String>> {
    BY_ID.poll(&id)
}

/// The id of the user named `name` (any case), or None while on the way.
pub fn poll_id(name: &str) -> Option<Result<u64, String>> {
    BY_NAME.poll(&name.to_lowercase())
}

pub fn counters() -> (Counters, Counters) {
    (BY_ID.counters(), BY_NAME.counters())
}

/// "HTTP 429 (Too Many Requests)", as the lookups have always failed.
pub fn status_text(code: u16) -> String {
    let reason = match code {
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        _ => "Error",
    };
    format!("HTTP {code} ({reason})")
}

fn post(url: &str, body: serde_json::Value) -> Result<serde_json::Value, String> {
    let mut resp = crate::api::agent()
        .post(url)
        .config()
        .http_status_as_error(false)
        .build()
        .send_json(&body)
        .map_err(|e| e.to_string())?;
    let code = resp.status().as_u16();
    if code != 200 {
        return Err(status_text(code));
    }
    let text = resp.body_mut().read_to_string().map_err(|e| e.to_string())?;
    serde_json::from_str(&text).map_err(|_| "Invalid response received".to_string())
}

fn fetch_ids(ids: &[u64]) -> Result<Vec<(u64, Answer<User>)>, String> {
    let v = post(USERS_URL, serde_json::json!({ "userIds": ids, "excludeBannedUsers": false }))?;
    Ok(parse_ids(&v))
}

fn parse_ids(v: &serde_json::Value) -> Vec<(u64, Answer<User>)> {
    v["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| {
            let user = User {
                name: r["name"].as_str()?.to_string(),
                display_name: r["displayName"].as_str().unwrap_or_default().to_string(),
            };
            Some((r["id"].as_u64()?, Answer { value: Ok(user), ttl: KEEP }))
        })
        .collect()
}

fn fetch_names(names: &[String]) -> Result<Vec<(String, Answer<u64>)>, String> {
    let v = post(USERNAMES_URL, serde_json::json!({ "usernames": names, "excludeBannedUsers": false }))?;
    Ok(parse_names(&v))
}

fn parse_names(v: &serde_json::Value) -> Vec<(String, Answer<u64>)> {
    v["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| {
            let asked = r["requestedUsername"].as_str()?.to_lowercase();
            Some((asked, Answer { value: Ok(r["id"].as_u64()?), ttl: KEEP }))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_both_replies() {
        let ids = serde_json::json!({"data":[{"hasVerifiedBadge":false,"id":1,"name":"Roblox","displayName":"Roblox"}]});
        let got = parse_ids(&ids);
        assert_eq!(got[0].0, 1);
        assert_eq!(got[0].1.value, Ok(User { name: "Roblox".into(), display_name: "Roblox".into() }));
        let names = serde_json::json!({"data":[{"requestedUsername":"roblox","hasVerifiedBadge":false,"id":1,"name":"Roblox","displayName":"Roblox"}]});
        let got = parse_names(&names);
        assert_eq!((got[0].0.as_str(), got[0].1.value.clone()), ("roblox", Ok(1)));
    }
}
