//! Thumbnail URLs, batched: `POST thumbnails.roblox.com/v1/batch` with up to
//! 100 requests, as Studio's BatchThumbnailFetcher asks (`rbxthumb://`
//! content: player list headshots, game icons, badge icons). A thumbnail
//! Roblox is still rendering comes back `Pending`; callers ask again after a
//! wait (Studio: BatchThumbnailExponentialInitialWaitMs, MaxExponentialRetries).

use std::sync::LazyLock;
use std::time::Duration;

use crate::batch::{Answer, Batcher, Counters};

pub const BATCH_URL: &str = "https://thumbnails.roblox.com/v1/batch";
/// The error for a thumbnail that isn't rendered yet (not kept).
pub const PENDING: &str = "thumbnail not ready";

/// (rbxthumb type, target id, square size)
pub type Key = (String, u64, u32);

static THUMBS: LazyLock<Batcher<Key, String>> = LazyLock::new(|| {
    Batcher::new("thumbs", 100, 4, Duration::from_millis(15), fetch, "no thumbnail", Duration::from_secs(300))
});

/// The image URL for `key`, waiting for its batch (blocking; worker threads).
pub fn url(key: &Key) -> Result<String, String> {
    THUMBS.get(key)
}

pub fn counters() -> Counters {
    THUMBS.counters()
}

fn request_id(k: &Key) -> String {
    format!("{}:{}:{}", k.0, k.1, k.2)
}

fn fetch(keys: &[Key]) -> Result<Vec<(Key, Answer<String>)>, String> {
    let body: Vec<serde_json::Value> = keys
        .iter()
        .map(|k| {
            serde_json::json!({
                "requestId": request_id(k),
                "type": k.0,
                "targetId": k.1,
                "size": format!("{0}x{0}", k.2),
                "format": "Png",
                "isCircular": false,
            })
        })
        .collect();
    let mut resp = crate::api::agent().post(BATCH_URL).send_json(&body).map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status().as_u16()));
    }
    let text = resp.body_mut().read_to_string().map_err(|e| e.to_string())?;
    Ok(parse(keys, &text))
}

fn parse(keys: &[Key], text: &str) -> Vec<(Key, Answer<String>)> {
    let v: serde_json::Value = serde_json::from_str(text).unwrap_or_default();
    let rows = v["data"].as_array().cloned().unwrap_or_default();
    keys.iter()
        .filter_map(|k| {
            let id = request_id(k);
            let row = rows.iter().find(|r| r["requestId"].as_str() == Some(id.as_str()))?;
            let url = row["imageUrl"].as_str().filter(|u| !u.is_empty());
            let state = row["state"].as_str().unwrap_or("");
            let answer = match (state, url) {
                ("Pending", _) => Answer { value: Err(PENDING.into()), ttl: Duration::ZERO },
                (_, Some(u)) => Answer { value: Ok(u.to_string()), ttl: Duration::from_secs(600) },
                _ => Answer {
                    value: Err(row["errorMessage"].as_str().filter(|m| !m.is_empty()).unwrap_or(state).to_string()),
                    ttl: Duration::from_secs(300),
                },
            };
            Some((k.clone(), answer))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_by_request_id() {
        let keys: Vec<Key> = vec![("AvatarHeadShot".into(), 1, 150), ("GameIcon".into(), 2, 512), ("Asset".into(), 3, 150)];
        let text = r#"{"data":[
            {"requestId":"GameIcon:2:512","errorCode":0,"errorMessage":"","targetId":2,"state":"Pending","imageUrl":null},
            {"requestId":"AvatarHeadShot:1:150","errorCode":0,"errorMessage":"","targetId":1,"state":"Completed","imageUrl":"https://tr.rbxcdn.com/x/150/150/AvatarHeadshot/Png"}
        ]}"#;
        let got = parse(&keys, text);
        assert_eq!(got.len(), 2, "a key without a row is left to the batcher");
        let head = got.iter().find(|(k, _)| k.1 == 1).unwrap();
        assert_eq!(head.1.value.as_deref(), Ok("https://tr.rbxcdn.com/x/150/150/AvatarHeadshot/Png"));
        let icon = got.iter().find(|(k, _)| k.1 == 2).unwrap();
        assert_eq!(icon.1.value, Err(PENDING.to_string()));
        assert_eq!(icon.1.ttl, Duration::ZERO);
    }

    #[test]
    #[ignore = "live thumbnails batch"]
    fn live_batch() {
        let t = std::thread::spawn(|| url(&("AvatarHeadShot".into(), 1, 150)));
        let b = url(&("GameIcon".into(), 1818, 150));
        let a = t.join().unwrap();
        println!("{a:?} {b:?}");
        assert!(a.is_ok() || a == Err(PENDING.into()), "{a:?}");
        assert!(b.is_ok() || b == Err(PENDING.into()), "{b:?}");
        assert_eq!(counters().batches, 1);
    }
}
