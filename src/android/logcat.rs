//! Logging on Android: `log` records go to logcat under the tag `rustudio`
//! (eframe's, wgpu's and winit's lines included), and a panic's message is
//! logged before the crash report is written. stdout and stderr are already
//! forwarded to logcat by android-activity (tag `RustStdoutStderr`), so
//! `println!` / `eprintln!` in the desktop code show up too.
//!
//! `adb logcat -s rustudio RustStdoutStderr` follows the app.
//!
//! The same records also go to a file, the session log
//! (`<cache>/rustudio/logs/session.log`; [`open_session`]), because
//! logcat's buffer is small and a phone's system fills it fast: the
//! export of 2026-11-25 (a GPU out-of-memory on a Note20, 5.3 GB
//! resident) held the last 20 s of the app's lines and none of the
//! `memory:` lines that said when and into what the memory had grown.
//! The file keeps the app's own lines at Info and the graphics stack's
//! warnings (what the desktop's `rustudio_core::diagnostics` keeps —
//! that logger isn't used here, android_logger holds the `log` slot),
//! up to [`SESSION_MAX`]; past that the file becomes the previous one
//! (`session-prev.log`) and a new one starts, so the last two files
//! always hold what happened. A new process rotates the same way at its
//! start, and keeps [`PREVIOUS`] previous files (`session-prev.log`,
//! `session-prev2.log`, …): after a crash the app restarts itself, and
//! that restart may crash or be left and started again before the user
//! gets to Help > Copy Logs — with one previous file the run that
//! crashed was gone by then (the 2026-11-25 report after the GPU
//! out-of-memory: "the session log was not saved"). logs.rs exports them
//! all, oldest first.
//!
//! The file is written unbuffered, so a process killed keeps every line
//! up to its last; and it is synced to the flash every [`SYNC_EVERY`]
//! ([`Session::write`]) for the crash that takes the phone down with it
//! (a GPU fault that restarts the device loses what the kernel still
//! held in memory).
//!
//! stdout and stderr go to the file too (`capture_stdio`): the `[startup]`
//! timings, the `LOADTRACE` trail, Luau's own prints and every
//! `eprintln!` of the desktop code are not `log` records. The pipe that
//! android-activity put on the two descriptors to forward them to logcat
//! is replaced by one of the app's own, whose reader does both.

use std::fs::{File, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, Once, OnceLock};
use std::time::{Duration, Instant};

static INSTALLED: Once = Once::new();

/// The session log's size cap; at it the file rotates to `session-prev.log`.
const SESSION_MAX: u64 = 6 << 20;
/// Previous runs' files kept (`session-prev.log` … `session-prev{PREVIOUS}.log`).
pub const PREVIOUS: usize = 3;
/// The file is synced to storage this often while lines come.
const SYNC_EVERY: Duration = Duration::from_millis(1500);
/// Lines kept in memory from before the cache folder is known (a start's
/// first lines), written to the file when it opens.
const EARLY_LINES: usize = 400;

/// The session log's file name, and the previous one's.
pub const SESSION_FILE: &str = "session.log";
pub const SESSION_PREV_FILE: &str = "session-prev.log";

/// The previous runs' file names, oldest first (`session-prev3.log`,
/// `session-prev2.log`, `session-prev.log`).
pub fn previous_files() -> Vec<String> {
    (1..=PREVIOUS).rev().map(prev_name).collect()
}

fn prev_name(n: usize) -> String {
    if n <= 1 { SESSION_PREV_FILE.to_string() } else { format!("session-prev{n}.log") }
}

/// `session.log` becomes `session-prev.log`, which becomes
/// `session-prev2.log`, and so on up to [`PREVIOUS`]; the oldest goes.
fn rotate(dir: &Path) {
    for n in (1..=PREVIOUS).rev() {
        let from = if n == 1 { dir.join(SESSION_FILE) } else { dir.join(prev_name(n - 1)) };
        if from.exists() {
            let _ = std::fs::rename(&from, dir.join(prev_name(n)));
        }
    }
}

struct Session {
    path: PathBuf,
    file: File,
    size: u64,
    /// Lines written since the last sync, and when it was.
    dirty: bool,
    synced: Instant,
}

/// logcat and the session log, one `log::Log`.
struct Tee {
    android: android_logger::AndroidLogger,
    session: Mutex<Option<Session>>,
    early: Mutex<Vec<String>>,
}

static TEE: OnceLock<Tee> = OnceLock::new();

pub fn install() {
    INSTALLED.call_once(|| {
        // Debug builds log at Debug (wgpu's adapter / surface choices among
        // it); release builds at Info.
        let level = if cfg!(debug_assertions) { log::LevelFilter::Debug } else { log::LevelFilter::Info };
        // The audio decoders (symphonia) say a line per stream opened at
        // Info; a place with a few sounds fills logcat with them: Warn.
        // (a module prefix: every symphonia_* crate)
        let filter = android_logger::FilterBuilder::new().filter_level(level).filter_module("symphonia", log::LevelFilter::Warn).build();
        let config = android_logger::Config::default().with_max_level(level).with_filter(filter).with_tag("rustudio");
        let tee = TEE.get_or_init(|| Tee {
            android: android_logger::AndroidLogger::new(config),
            session: Mutex::new(None),
            early: Mutex::new(Vec::new()),
        });
        if log::set_logger(tee).is_ok() {
            log::set_max_level(level);
        }
        // crash::install() chains its own hook after this one (it takes the
        // hook that is current when it runs), so the message is in logcat
        // before the report is written, even if writing it fails.
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            log::error!("panic: {info}");
            sync_session();
            previous(info);
        }));
    });
}

/// The folder of the session logs under the app's cache folder.
pub fn session_dir() -> PathBuf {
    rustudio_core::android_dirs::cache_dir().join("rustudio").join("logs")
}

/// Opens the session log once the cache folder is known (`run`, after
/// `init_dirs`): the previous process's file becomes `session-prev.log`,
/// and the lines logged so far go in first.
pub fn open_session() {
    let Some(tee) = TEE.get() else { return };
    let dir = session_dir();
    if let Err(e) = std::fs::create_dir_all(&dir) {
        log::warn!("session log: {}: {e}", dir.display());
        return;
    }
    let path = dir.join(SESSION_FILE);
    rotate(&dir);
    match open_new(&path) {
        Ok(mut s) => {
            let early = std::mem::take(&mut *tee.early.lock().unwrap_or_else(|e| e.into_inner()));
            for line in early {
                s.write(&line);
            }
            *tee.session.lock().unwrap_or_else(|e| e.into_inner()) = Some(s);
            log::info!("session log: {}", path.display());
            capture_stdio(tee);
        }
        Err(e) => log::warn!("session log: {}: {e}", path.display()),
    }
}

/// A line of stdout / stderr into the session log (and logcat, as
/// android-activity's forwarder did, under its tag).
fn stdio_line(tee: &'static Tee, line: &str) {
    if let Ok(msg) = std::ffi::CString::new(line.trim_end_matches('\n')) {
        // (liblog's; android_logger links it)
        unsafe extern "C" {
            fn __android_log_write(prio: i32, tag: *const std::ffi::c_char, text: *const std::ffi::c_char) -> i32;
        }
        const ANDROID_LOG_INFO: i32 = 4;
        unsafe {
            __android_log_write(ANDROID_LOG_INFO, c"RustStdoutStderr".as_ptr(), msg.as_ptr());
        }
    }
    let line = line.get(..line.floor_char_boundary(4096)).unwrap_or(line);
    let stamped = format!("{} stdio: {}\n", chrono::Local::now().format("%m-%d %H:%M:%S%.3f"), line.trim_end_matches('\n'));
    if let Some(s) = tee.session.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        s.write(&stamped);
    }
}

/// Puts a pipe of the app's own on stdout and stderr (over
/// android-activity's, whose reader then sees its end of file and stops)
/// and reads it on a thread into logcat and the session log.
fn capture_stdio(tee: &'static Tee) {
    use std::io::BufRead as _;
    use std::os::fd::FromRawFd as _;
    let reader = unsafe {
        let mut fds: [libc::c_int; 2] = [0, 0];
        if libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) != 0 {
            log::warn!("session log: no pipe for stdout / stderr: {}", std::io::Error::last_os_error());
            return;
        }
        libc::dup2(fds[1], libc::STDOUT_FILENO);
        libc::dup2(fds[1], libc::STDERR_FILENO);
        libc::close(fds[1]);
        std::fs::File::from_raw_fd(fds[0])
    };
    let spawned = std::thread::Builder::new().name("stdio-to-session".into()).spawn(move || {
        let mut reader = std::io::BufReader::new(reader);
        let mut buf = Vec::new();
        loop {
            buf.clear();
            // (bytes, not read_line: a line that isn't UTF-8 — a binary
            // dump — must not end the capture)
            match reader.read_until(b'\n', &mut buf) {
                Ok(0) | Err(_) => break,
                Ok(_) => stdio_line(tee, &String::from_utf8_lossy(&buf)),
            }
        }
    });
    if let Err(e) = spawned {
        log::warn!("session log: no thread for stdout / stderr: {e}");
    }
}

fn open_new(path: &Path) -> std::io::Result<Session> {
    let file = OpenOptions::new().write(true).create(true).truncate(true).open(path)?;
    Ok(Session { path: path.to_path_buf(), file, size: 0, dirty: false, synced: Instant::now() })
}

impl Session {
    fn write(&mut self, line: &str) {
        if self.size >= SESSION_MAX {
            // The file rotates: this one becomes the previous, a new one
            // goes on (the files hold the most recent 6–24 MB).
            let _ = self.file.flush();
            let _ = self.file.sync_data();
            if let Some(dir) = self.path.parent() {
                rotate(dir);
            }
            match open_new(&self.path) {
                Ok(s) => *self = s,
                Err(_) => return,
            }
        }
        if self.file.write_all(line.as_bytes()).is_ok() {
            self.size += line.len() as u64;
            self.dirty = true;
        }
        if self.dirty && self.synced.elapsed() >= SYNC_EVERY {
            let _ = self.file.sync_data();
            self.dirty = false;
            self.synced = Instant::now();
        }
    }

    fn sync(&mut self) {
        let _ = self.file.flush();
        if self.dirty {
            let _ = self.file.sync_data();
            self.dirty = false;
            self.synced = Instant::now();
        }
    }
}

/// Writes the session log through to storage now (a crash report being
/// written, the GPU lost: what follows may not run).
pub fn sync_session() {
    if let Some(tee) = TEE.get() {
        if let Some(s) = tee.session.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
            s.sync();
        }
    }
}

/// What the session log keeps: the app's own lines at Info and up, and the
/// graphics stack's warnings and errors (a lost device or a refused
/// allocation is otherwise only a panic with no cause).
fn session_wants(m: &log::Metadata<'_>) -> bool {
    let target = m.target();
    (m.level() <= log::Level::Info && target.starts_with("rustudio"))
        || (m.level() <= log::Level::Warn
            && ["wgpu", "naga", "egui_wgpu", "eframe", "winit", "egui_winit"].iter().any(|t| target.starts_with(t)))
}

impl log::Log for Tee {
    fn enabled(&self, m: &log::Metadata<'_>) -> bool {
        self.android.enabled(m) || session_wants(m)
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.android.enabled(record.metadata()) {
            self.android.log(record);
        }
        if !session_wants(record.metadata()) {
            return;
        }
        // One event per line; a record's text bounded (a dump of a whole
        // document would be the file).
        let text = record.args().to_string();
        let text = text.get(..text.floor_char_boundary(4096)).unwrap_or(&text);
        let line = format!(
            "{} {:5} {}: {}\n",
            chrono::Local::now().format("%m-%d %H:%M:%S%.3f"),
            record.level(),
            record.target(),
            text.replace('\n', "\\n")
        );
        let mut session = self.session.lock().unwrap_or_else(|e| e.into_inner());
        match session.as_mut() {
            Some(s) => s.write(&line),
            None => {
                let mut early = self.early.lock().unwrap_or_else(|e| e.into_inner());
                if early.len() < EARLY_LINES {
                    early.push(line);
                }
            }
        }
    }

    fn flush(&self) {
        if let Some(s) = self.session.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
            s.sync();
        }
    }
}
