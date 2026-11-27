//! Getting the logs off a phone. The crash reports (`rustudio_core::crash`,
//! `Android/data/<package>/files/crash`) and the session log
//! (`rustudio_core::diagnostics`, the app's cache folder) lie in folders a
//! Files app can't open since Android 11 and `adb` isn't at hand on most
//! phones, so a crash — a publish that took the process down — left
//! nothing a user could send. [`export`] gathers what there is into one
//! text file in the shared Download folder (`Download/rustudio/
//! rustudio-logs-<stamp>.txt`, [`super::jni::export_download`]): the
//! build and device, the crash reports (newest first, three at most),
//! the end of the session logs (logcat.rs: this run's and the previous
//! runs' — the one that crashed among them, when the export follows a
//! restart or two), and
//! the app's own logcat lines
//! (`logcat -d`: an app reads its own lines without a permission, and a
//! process that died a minute ago still has its lines in the buffer, the
//! kernel's and the system's lines about it included — `Out of memory`,
//! `lowmemorykiller`, `Fatal signal`, `am_kill`; but the buffer is small:
//! the session logs are what holds a run's `memory:` lines). Done at the
//! start after a run that left a crash report (`App::new`, named in
//! Output), and on Help > Copy Logs to Download at any time.

use std::fmt::Write as _;

/// The log file's size cap, and each session log's share of it.
const MAX: usize = 8 << 20;
const SESSION_TAIL: usize = 1 << 20;

/// Writes the logs file and returns its path as a Files app shows it.
pub fn export(reason: &str) -> Result<String, String> {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    let mut out = String::new();
    let _ = writeln!(
        out,
        "rustudio {} ({}) on Android API {}, {} — logs exported {stamp} ({reason})",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::ARCH,
        super::jni::sdk_int(),
        device(),
    );
    let _ = writeln!(out, "files {}\ncache {}\n", rustudio_core::android_dirs::files_dir().display(), rustudio_core::android_dirs::cache_dir().display());

    // Crash reports, newest first.
    let mut reports: Vec<std::path::PathBuf> = std::fs::read_dir(rustudio_core::crash::dir())
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "txt"))
        .collect();
    reports.sort();
    reports.reverse();
    let _ = writeln!(out, "==== crash reports: {} ====", reports.len());
    for p in reports.iter().take(3) {
        let _ = writeln!(out, "---- {} ----", p.file_name().unwrap_or_default().to_string_lossy());
        out.push_str(&std::fs::read_to_string(p).unwrap_or_else(|e| format!("(unreadable: {e})")));
        out.push('\n');
    }

    // The session logs' ends: the previous runs' (the crashed one among
    // them, after a restart or two), oldest first, then this run's
    // (logcat.rs).
    let dir = super::logcat::session_dir();
    let mut files: Vec<(String, String)> = super::logcat::previous_files()
        .into_iter()
        .enumerate()
        .map(|(i, name)| (name, format!("{} run(s) before this one", super::logcat::PREVIOUS - i)))
        .collect();
    files.push((super::logcat::SESSION_FILE.to_string(), "this run".to_string()));
    for (name, which) in files {
        let path = dir.join(&name);
        let Ok(text) = std::fs::read(&path) else { continue };
        let start = text.len().saturating_sub(SESSION_TAIL);
        let start = if start > 0 { text[start..].iter().position(|b| *b == b'\n').map_or(start, |i| start + i + 1) } else { 0 };
        let _ = writeln!(out, "\n==== session log, {which}: {} ({} bytes{}) ====", path.display(), text.len(), if start > 0 { ", the end" } else { "" });
        out.push_str(&String::from_utf8_lossy(&text[start..]));
    }
    // (The desktop's diagnostics log, should a build keep one here.)
    if let Some(path) = rustudio_core::diagnostics::path() {
        let text = std::fs::read(path).unwrap_or_default();
        let start = text.len().saturating_sub(SESSION_TAIL);
        let _ = writeln!(out, "\n==== diagnostics log {} ({} bytes{}) ====", path.display(), text.len(), if start > 0 { ", the end" } else { "" });
        out.push_str(&String::from_utf8_lossy(&text[start..]));
    }

    // The app's own logcat lines (and the system's about it).
    let _ = writeln!(out, "\n==== logcat -d ====");
    match std::process::Command::new("logcat").args(["-d", "-v", "threadtime", "-t", "6000"]).output() {
        Ok(o) => {
            let text = String::from_utf8_lossy(&o.stdout);
            let room = MAX.saturating_sub(out.len());
            let start = text.len().saturating_sub(room);
            let start = text[start..].find('\n').map_or(start, |i| start + i + 1);
            out.push_str(&text[start..]);
            if !o.stderr.is_empty() {
                let _ = writeln!(out, "(logcat: {})", String::from_utf8_lossy(&o.stderr).trim());
            }
        }
        Err(e) => {
            let _ = writeln!(out, "(logcat couldn't be run: {e})");
        }
    }

    let name = format!("rustudio-logs-{stamp}.txt");
    let path = super::jni::export_download(&name, "text/plain", out.as_bytes())?;
    log::info!("logs: {} bytes written to {path} ({reason})", out.len());
    Ok(path)
}

/// `Build.MANUFACTURER Build.MODEL (Build.DEVICE)`, from the system
/// properties (no JNI: readable by any app).
fn device() -> String {
    let prop = |name: &str| -> String {
        std::process::Command::new("getprop")
            .arg(name)
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default()
    };
    format!("{} {} ({}, {})", prop("ro.product.manufacturer"), prop("ro.product.model"), prop("ro.product.device"), prop("ro.build.version.release"))
}
