//! Luau intelligence for the script editor: completions for the word at
//! the caret from the place itself and Roblox's API — the globals and
//! libraries, `game:GetService("…")` and the services' children by name,
//! the classes' properties, methods and events, `Enum.X.Y`, the script's
//! locals, functions and parameters, the members a required ModuleScript
//! returns (instance and string requires, `./`, `../`, `@self`), require
//! paths — and a signature help for the call the caret is in. Ported from
//! Android-rbxl (TouseefX, `luau_intelligence.rs`: [`index`]), whose API
//! tables ([`api_data`]) and reflection questions ([`schema`]) come along.
//!
//! The index of the place ([`index::ProjectIndex`]) is built from the
//! DataModel with the open tabs' unsaved text over it, and rebuilt only
//! when its fingerprint changes (scripts, names, sources: cheap to hash,
//! dear to rebuild on a big place) — never more than once a second while
//! typing, and off the frame it was asked in when it is big.

pub mod api_data;
pub mod index;
pub mod schema;

pub use index::{apply_suggestion_at, Completion, ProjectIndex, SignatureHelp};

use rbx_dom_weak::{types::Ref, WeakDom};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The editor's cached index.
#[derive(Default)]
pub struct Intel {
    index: Option<Arc<ProjectIndex>>,
    fingerprint: u64,
    built: Option<Instant>,
    /// How long the last build took: a place whose index takes a while is
    /// rebuilt less often (never while a key is held).
    cost: Duration,
}

impl Intel {
    /// The index for this DataModel and these unsaved buffers, rebuilt
    /// when they changed (and the last build is old enough for its cost).
    pub fn index<'a>(&mut self, dom: &WeakDom, overrides: impl Iterator<Item = (Ref, &'a str)> + Clone) -> Arc<ProjectIndex> {
        let fp = ProjectIndex::fingerprint(dom, overrides.clone());
        let stale = self.index.is_none() || fp != self.fingerprint;
        let soon = self.built.is_some_and(|t| t.elapsed() < (self.cost * 4).clamp(Duration::from_millis(250), Duration::from_secs(5)));
        if stale && !(soon && self.index.is_some()) {
            let started = Instant::now();
            self.index = Some(Arc::new(ProjectIndex::build_with_overrides(dom, overrides)));
            self.fingerprint = fp;
            self.cost = started.elapsed();
            self.built = Some(started);
            if self.cost > Duration::from_millis(50) {
                log::info!("luau intel: place indexed in {:.0} ms", self.cost.as_secs_f32() * 1000.0);
            }
        }
        self.index.clone().expect("an index")
    }

    /// Completions for the caret (a char index) in `source` of `script`.
    pub fn complete<'a>(
        &mut self,
        dom: &WeakDom,
        overrides: impl Iterator<Item = (Ref, &'a str)> + Clone,
        script: Ref,
        source: &str,
        cursor_char: usize,
    ) -> Vec<Completion> {
        let index = self.index(dom, overrides);
        let mut items = index.complete_at(script, source, cursor_char);
        items.truncate(24);
        items
    }

    /// The signature of the call the caret is in, if any.
    pub fn signature<'a>(
        &mut self,
        dom: &WeakDom,
        overrides: impl Iterator<Item = (Ref, &'a str)> + Clone,
        script: Ref,
        source: &str,
        cursor_char: usize,
    ) -> Option<SignatureHelp> {
        self.index(dom, overrides).signature_help_at(script, source, cursor_char)
    }
}
