//! The on-disk asset cache (`cache_dir()/assets`), kept current and bounded
//! the way Studio keeps its own (rbx-storage):
//!
//! - **Atomic writes.** A file is written beside its place and renamed in,
//!   so a crash or a full disk never leaves a truncated asset to be served.
//! - **"Latest" stays current.** A cached `{id}_vlatest.bin` is used at once;
//!   meanwhile, in the background (signed in), where the asset lives now is
//!   resolved in batches with everything else, and its CDN content key is
//!   compared with the one the file was saved under (`….bin.key`). A
//!   different key (or none recorded) fetches the new bytes for the next
//!   load. (Studio: the asset resolution redirect cache, refreshed in the
//!   background by `DFIntNumConcurrentBatchRefreshWorkflowsPerAssetProviderInstance`.)
//! - **Bounded.** Past [`cap_bytes`] (2 GB; `RUSTUDIO_ASSET_CACHE_MB`), the
//!   least recently used files are removed. A file's modified time is its
//!   last use (touched on the first read each session).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, OnceLock};
use std::time::{Duration, SystemTime};

use crate::assets::{cache_path, AssetRef};

/// The cache's size limit.
pub fn cap_bytes() -> u64 {
    std::env::var("RUSTUDIO_ASSET_CACHE_MB")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|mb| *mb > 0)
        .unwrap_or(2048)
        * 1024
        * 1024
}

/// Writes `bytes` to `path` all at once (temporary file, then rename).
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = path.with_extension(format!("tmp{}-{n}", std::process::id()));
    std::fs::write(&tmp, bytes)?;
    match std::fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            // (Windows: another writer renamed the same asset in first;
            // theirs is as good as ours.)
            if path.is_file() {
                Ok(())
            } else {
                Err(e)
            }
        }
    }
}

fn key_path(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".key");
    PathBuf::from(s)
}

/// Saves an asset's bytes, with the CDN content key they came from when the
/// resolver knows it.
pub fn write_asset(a: AssetRef, bytes: &[u8]) -> std::io::Result<()> {
    let path = cache_path(a);
    write_atomic(&path, bytes)?;
    if let Some(key) = rustudio_account::asset_batch::peek(a.id, a.version)
        .and_then(|r| rustudio_account::asset_batch::content_key(&r.location))
    {
        let _ = write_atomic(&key_path(&path), key.as_bytes());
    }
    maintenance();
    Ok(())
}

/// The cached bytes of `a`, if any. A hit marks the file used and (for the
/// latest version) queues a background check that it is still current.
pub fn read(a: AssetRef) -> Option<Vec<u8>> {
    let path = cache_path(a);
    let bytes = std::fs::read(&path).ok()?;
    if bytes.is_empty() {
        return None;
    }
    touch_once(&path);
    if a.version.is_none() {
        revalidate_later(a.id);
    }
    maintenance();
    Some(bytes)
}

fn touch_once(path: &Path) {
    static TOUCHED: LazyLock<Mutex<HashSet<PathBuf>>> = LazyLock::new(Default::default);
    if !TOUCHED.lock().unwrap().insert(path.to_path_buf()) {
        return;
    }
    if let Ok(f) = std::fs::File::options().write(true).open(path) {
        let _ = f.set_modified(SystemTime::now());
    }
}

// ---------------------------------------------------------------- revalidation

static REVALIDATE: LazyLock<Mutex<Option<std::sync::mpsc::Sender<u64>>>> = LazyLock::new(|| {
    let (tx, rx) = std::sync::mpsc::channel::<u64>();
    let _ = std::thread::Builder::new().name("asset-revalidate".into()).spawn(move || {
        rustudio_account::web::with_priority(rustudio_account::web::Priority::Background, || {
            while let Ok(id) = rx.recv() {
                revalidate(id);
            }
        })
    });
    Mutex::new(Some(tx))
});

fn revalidate_later(id: u64) {
    static ASKED: LazyLock<Mutex<HashSet<u64>>> = LazyLock::new(Default::default);
    if crate::sim_clock::on() || !ASKED.lock().unwrap().insert(id) {
        return;
    }
    let Some(session) = session() else { return };
    // Resolved in the next batch, with whatever else is being asked for.
    rustudio_account::asset_batch::prefetch(&session, id, None);
    if let Some(tx) = REVALIDATE.lock().unwrap().as_ref() {
        let _ = tx.send(id);
    }
}

fn session() -> Option<Arc<rustudio_account::Session>> {
    let client = crate::assets::session_client()?;
    let s = client.lock().ok()?.as_ref()?.session.downloader();
    Some(Arc::new(s))
}

/// Compares the cached copy's content key with where the asset is now;
/// fetches the new bytes when they differ (or no key was recorded).
fn revalidate(id: u64) {
    let a = AssetRef { id, version: None };
    let Some(session) = session() else { return };
    let Ok(now) = rustudio_account::asset_batch::resolve(&session, id, None) else { return };
    let Some(key) = rustudio_account::asset_batch::content_key(&now.location) else { return };
    let path = cache_path(a);
    let recorded = std::fs::read_to_string(key_path(&path)).ok();
    if recorded.as_deref().map(str::trim) == Some(key.as_str()) {
        return;
    }
    if let Ok(bytes) = session.download_asset_typed(id, None) {
        let changed = std::fs::read(&path).map_or(true, |old| old != bytes);
        let _ = write_asset(a, &bytes);
        if changed && recorded.is_some() {
            log::info!("asset {id} changed on Roblox: the cached copy was updated");
        }
    }
}

// ---------------------------------------------------------------- eviction

/// Starts the size check once per process (a while after start-up, off the
/// main thread), and again after enough has been written.
fn maintenance() {
    static STARTED: OnceLock<()> = OnceLock::new();
    STARTED.get_or_init(|| {
        let _ = std::thread::Builder::new().name("asset-cache-evict".into()).spawn(|| {
            std::thread::sleep(Duration::from_secs(20));
            loop {
                evict(&crate::assets::cache_dir().join("assets"), cap_bytes());
                std::thread::sleep(Duration::from_secs(600));
            }
        });
    });
}

/// Removes the least recently used files under `dir` until it is back under
/// 90 % of `cap` (only when it's over `cap`). Returns the bytes removed.
pub fn evict(dir: &Path, cap: u64) -> u64 {
    let Ok(rd) = std::fs::read_dir(dir) else { return 0 };
    let mut files: Vec<(SystemTime, u64, PathBuf)> = rd
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            m.is_file().then(|| (m.modified().unwrap_or(SystemTime::UNIX_EPOCH), m.len(), e.path()))
        })
        .collect();
    let total: u64 = files.iter().map(|f| f.1).sum();
    if total <= cap {
        return 0;
    }
    files.sort_by_key(|f| f.0);
    let target = cap / 10 * 9;
    let mut left = total;
    let mut removed = 0;
    for (_, len, path) in files {
        if left <= target {
            break;
        }
        // (A key file goes with its asset; a lone one is tiny.)
        if path.extension().is_some_and(|e| e == "key") {
            continue;
        }
        if std::fs::remove_file(&path).is_ok() {
            left -= len;
            removed += len;
            let _ = std::fs::remove_file(key_path(&path));
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rs-asset-cache-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn atomic_write_replaces_whole_files() {
        let d = dir("atomic");
        let p = d.join("1_vlatest.bin");
        write_atomic(&p, b"first").unwrap();
        write_atomic(&p, b"second, longer").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"second, longer");
        let leftovers: Vec<_> = std::fs::read_dir(&d).unwrap().filter_map(|e| e.ok()).filter(|e| e.path() != p).collect();
        assert!(leftovers.is_empty(), "temporary files left: {leftovers:?}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn eviction_removes_least_recently_used_first() {
        let d = dir("evict");
        let now = SystemTime::now();
        for (i, age) in [(1u64, 300u64), (2, 200), (3, 100), (4, 0)] {
            let p = d.join(format!("{i}_vlatest.bin"));
            std::fs::write(&p, vec![0u8; 1000]).unwrap();
            std::fs::write(key_path(&p), b"k").unwrap();
            let f = std::fs::File::options().write(true).open(&p).unwrap();
            f.set_modified(now - Duration::from_secs(age)).unwrap();
        }
        assert_eq!(evict(&d, 10_000), 0, "under the cap: nothing goes");
        let removed = evict(&d, 2_500);
        assert_eq!(removed, 2_000);
        assert!(!d.join("1_vlatest.bin").exists() && !d.join("2_vlatest.bin").exists());
        assert!(!key_path(&d.join("1_vlatest.bin")).exists());
        assert!(d.join("3_vlatest.bin").exists() && d.join("4_vlatest.bin").exists());
        let _ = std::fs::remove_dir_all(&d);
    }
}
