//! Compiled scripts kept by source: a Play session compiles every
//! ModuleScript the server and the client each require, and a place with
//! data modules of tens of megabytes (a large ported place: 150 MB of
//! Luau, one module 47 MB) spent seconds compiling the same text twice per
//! Play and again on every run. Bytecode of sources over MIN_MEMORY is kept
//! for the process; over MIN_DISK (on Android: 4 KB) also in
//! <cache>/bytecode/, keyed by the source and the compiler (settings and
//! its output for a probe). RUSTUDIO_NO_BYTECODE_CACHE=1 turns it off.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

/// (1 KB: a Play compiles most ModuleScripts twice, once a side; below
/// this the hash costs what the compiler does.)
const MIN_MEMORY: usize = 1024;
/// On a phone every launch is a cold process and the compiler is slower
/// (a fighting game's 685 scripts, 7 MB of Luau, compiled again each
/// launch otherwise): nearly everything the memory cache keeps goes to
/// disk too (a few MB, one file a script, in the app's cache directory).
const MIN_DISK: usize = if cfg!(target_os = "android") { 4 * 1024 } else { 256 * 1024 };
/// Bytecode held in memory at most (later sources compile as before).
const MEMORY_CAP: usize = if cfg!(target_os = "android") { 128 * 1024 * 1024 } else { 768 * 1024 * 1024 };

/// Luau's optimization level for every script: Studio's 1 (no inlining,
/// so Output stacks keep every frame: runtime.rs Runtime::new). Roblox's
/// live client and server compile at 2 (local function inlining, loop
/// unrolling, more constant folding: luau.org/performance), so a phone,
/// which plays rather than debugs, does the same. RUSTUDIO_LUAU_OPT=0|1|2
/// picks it anywhere (a script's own `--!optimize N` still wins).
pub fn optimization_level() -> u8 {
    static L: OnceLock<u8> = OnceLock::new();
    *L.get_or_init(|| {
        std::env::var("RUSTUDIO_LUAU_OPT")
            .ok()
            .and_then(|v| v.trim().parse::<u8>().ok())
            .filter(|l| *l <= 2)
            .unwrap_or(if cfg!(target_os = "android") { 2 } else { 1 })
    })
}

/// The compiler every script runs through: Studio's options (runtime.rs
/// Runtime::new explains each).
pub fn compiler() -> mlua::chunk::Compiler {
    mlua::chunk::Compiler::new()
        .set_optimization_level(optimization_level())
        .set_debug_level(1)
        .set_vector_ctor("Vector3.new")
        .set_vector_type("Vector3")
        // (Roblox's list. Imports now resolve when a script loads (vendor
        // lvmload.cpp resolveImportSafe), so chains through these, which
        // scripts change, must stay run-time lookups: `_G.Config` set by
        // another script after this one loaded.)
        .set_mutable_globals(["Game", "Workspace", "_G", "game", "plugin", "script", "shared", "workspace"])
}

fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("RUSTUDIO_NO_BYTECODE_CACHE").is_none_or(|v| v == "0"))
}

/// What the bytecode depends on besides the source: the compiler settings
/// and the compiler itself, fingerprinted by its output for a probe chunk
/// (a Luau update that changes code generation or the bytecode version
/// changes it; rebuilding rustudio alone does not).
fn salt() -> &'static [u8] {
    static SALT: OnceLock<Vec<u8>> = OnceLock::new();
    SALT.get_or_init(|| {
        const PROBE: &str = r##"local t = {1, 2.5, "s", true, nil, x = {y = vector.create(1, 2, 3)}, [10] = -0.0}
local function f(a, ...) local s = `{a}:{select("#", ...)}` for i, v in ipairs(t) do s ..= tostring(v) end return s, #t end
local v = Vector3.new(1, 2, 3) local g = game:GetService("Workspace") workspace.Name = script.Name
return f(1, 2, 3), math.floor(v.X // 2) % 3, (if t[1] then 1 else 2) + bit32.band(7, 3), string.format("%d", 4)"##;
        let mut h = blake3::Hasher::new();
        h.update(format!("rustudio bytecode 2|O{} D1 Vector3.new Vector3 mutable globals _G|", optimization_level()).as_bytes());
        h.update(&compiler().compile(PROBE).unwrap_or_default());
        h.finalize().as_bytes().to_vec()
    })
}

struct Memory {
    map: HashMap<[u8; 32], Arc<Vec<u8>>>,
    bytes: usize,
}

fn memory() -> &'static Mutex<Memory> {
    static M: OnceLock<Mutex<Memory>> = OnceLock::new();
    M.get_or_init(|| Mutex::new(Memory { map: HashMap::new(), bytes: 0 }))
}

fn dir() -> PathBuf {
    rustudio_core::assets::cache_dir().join("bytecode")
}

/// Bytecode for a script's source, compiled once per process (and kept on
/// disk for big sources). None: compile it the usual way (small, a syntax
/// error, or the cache is off).
pub fn get(source: &str) -> Option<Arc<Vec<u8>>> {
    // (A Source holding bytecode already is loaded as such by mlua.)
    if !enabled() || source.len() < MIN_MEMORY || source.as_bytes()[0] < 0x09 {
        return None;
    }
    let mut h = blake3::Hasher::new();
    h.update(salt());
    h.update(source.as_bytes());
    let key: [u8; 32] = *h.finalize().as_bytes();
    if let Some(b) = memory().lock().ok()?.map.get(&key) {
        return Some(b.clone());
    }
    let hex: String = key.iter().map(|b| format!("{b:02x}")).collect();
    let file = dir().join(format!("{hex}.luauc"));
    let disk = source.len() >= MIN_DISK;
    let from_disk = disk.then(|| std::fs::read(&file).ok()).flatten().filter(|b| b.first().is_some_and(|v| *v != 0));
    let bytes = match from_disk {
        Some(b) => b,
        None => {
            // A failed compile (or Luau's error bytecode, first byte 0) goes
            // the usual way, for the usual message.
            let b = compiler().compile(source).ok().filter(|b| b.first().is_some_and(|v| *v != 0))?;
            if disk {
                let _ = std::fs::create_dir_all(dir());
                let tmp = file.with_extension(format!("tmp{}", std::process::id()));
                if std::fs::write(&tmp, &b).is_ok() {
                    let _ = std::fs::rename(&tmp, &file);
                }
            }
            b
        }
    };
    let b = Arc::new(bytes);
    if let Ok(mut m) = memory().lock() {
        if m.bytes + b.len() <= MEMORY_CAP {
            m.bytes += b.len();
            m.map.insert(key, b.clone());
        }
    }
    Some(b)
}

/// The precompile of an opened place: which one is current (a later
/// place cancels an earlier one's) and whether one is running.
static PRECOMPILE_GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static PRECOMPILING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Compiles a place's scripts into the cache ahead of Play, on one
/// background thread, as Roblox has a place's scripts compiled by the time
/// the player is in. Without it the first Play of a place compiles
/// everything on the main thread as the game starts — a fighting game's
/// 685 scripts, 7 MB of Luau, several seconds on a phone — and only the
/// second Play finds the cache (the 2026-11-25 report: Roblox "compiles
/// the whole game in four seconds", rustudio's Play takes far longer).
/// Called when a place opens (`Editor::set_document`) with every Script,
/// LocalScript and ModuleScript source; a new place supersedes the last
/// one's run, Play's own compiles meet the cache as they would after a
/// second Play. `RUSTUDIO_NO_PRECOMPILE=1` (or the cache being off) skips
/// it.
pub fn precompile(sources: Vec<String>) {
    use std::sync::atomic::Ordering;
    let generation = PRECOMPILE_GEN.fetch_add(1, Ordering::SeqCst) + 1;
    if !enabled() || std::env::var_os("RUSTUDIO_NO_PRECOMPILE").is_some_and(|v| v != "0") {
        return;
    }
    let sources: Vec<String> = sources.into_iter().filter(|s| s.len() >= MIN_MEMORY).collect();
    if sources.is_empty() {
        return;
    }
    let total: usize = sources.iter().map(|s| s.len()).sum();
    let spawned = std::thread::Builder::new().name("luau-precompile".into()).spawn(move || {
        // One at a time: a place opened over another waits for its turn
        // (the first run notices the new generation and stops).
        while PRECOMPILING.swap(true, Ordering::SeqCst) {
            std::thread::sleep(std::time::Duration::from_millis(50));
            if PRECOMPILE_GEN.load(Ordering::SeqCst) != generation {
                return;
            }
        }
        let t0 = std::time::Instant::now();
        let (mut done, mut fresh) = (0usize, 0usize);
        for s in &sources {
            if PRECOMPILE_GEN.load(Ordering::SeqCst) != generation {
                break;
            }
            let had = {
                let mut h = blake3::Hasher::new();
                h.update(salt());
                h.update(s.as_bytes());
                let key: [u8; 32] = *h.finalize().as_bytes();
                memory().lock().is_ok_and(|m| m.map.contains_key(&key))
            };
            if get(s).is_some() {
                done += 1;
                if !had {
                    fresh += 1;
                }
            }
            // A big place's compile shares the CPUs with the editor's own
            // frames: a breath between scripts keeps the UI responsive on a
            // two-core phone.
            std::thread::yield_now();
        }
        PRECOMPILING.store(false, Ordering::SeqCst);
        if PRECOMPILE_GEN.load(Ordering::SeqCst) == generation {
            eprintln!(
                "[startup] {done} of {} scripts ({} KB) precompiled for Play in {} ms ({fresh} compiled, the rest were cached)",
                sources.len(),
                total / 1024,
                t0.elapsed().as_millis()
            );
        }
    });
    if spawned.is_err() {
        PRECOMPILING.store(false, Ordering::SeqCst);
    }
}
