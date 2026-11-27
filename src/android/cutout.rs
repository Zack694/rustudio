//! The display cutout: a phone's camera hole or notch. The window is laid
//! out under it on the short edges of the screen
//! (`layoutInDisplayCutoutMode` shortEdges, API 28+: a landscape phone's
//! hole at the left or right edge, the top edge in portrait), where
//! Android had kept the window out of the hole's whole side — a
//! letterboxed strip the editor never got, black, the way the navigation
//! bar's was before system_bars.rs — and the editor pads itself by the
//! cutout's safe insets instead:
//!
//! * the root `Ui` of `App::ui` ends short of the hole ([`Cutout::child`]
//!   makes the `Ui` the rest of the frame is laid out in, as keyboard.rs's
//!   `shrink` takes the keyboard's room off the bottom), the strip painted
//!   the window's colour; a game in Play lies inside it, so no cutout
//!   reaches the game's view or its GUIs;
//! * the insets are egui's safe-area insets too ([`Cutout::before_frame`];
//!   the keyboard's bottom stays with them), so windows, menus and popups
//!   keep off the hole as they keep above the keyboard.
//!
//! The insets are read from the window on the Java main thread
//! ([`jni::display_cutout_later`](super::jni::display_cutout_later)),
//! which the editor's thread never waits for: at the start — again every
//! [`POLL`] until the window has insets to give — when the window regains
//! the focus, and after a resize (a rotation moves the hole to the other
//! side; the new insets follow the new size by a moment, so the question
//! is put a few times over the second after). `RUSTUDIO_CUTOUT=0`
//! (`debug.rustudio.env`) leaves the window out of the cutout, as before.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use egui::epaint::MarginF32;

use super::jni::CutoutAnswer;

/// Between the readings while one is wanted.
const POLL: Duration = Duration::from_millis(500);
/// After a trigger (the start, a focus, a resize): when the readings are
/// taken — the insets settle after the size.
const READINGS: [Duration; 3] = [Duration::ZERO, Duration::from_millis(300), Duration::from_millis(1200)];
/// How long an unanswered question is waited for before the next.
const GIVE_UP: Duration = Duration::from_secs(3);
/// Without an answer yet (no insets before the window is attached), the
/// question is repeated this long after the start.
const RETRY_FOR: Duration = Duration::from_secs(20);
/// The least the padded area keeps in each direction (a hole bigger than
/// that leaves the editor where it was).
const MIN_SIDE: f32 = 120.0;

static KEEP_OUT: AtomicBool = AtomicBool::new(false);
static DECIDED: AtomicBool = AtomicBool::new(false);

/// Whether the window goes under the cutout (decided once).
fn wanted() -> bool {
    if !DECIDED.swap(true, Ordering::SeqCst) {
        let keep_out = std::env::var("RUSTUDIO_CUTOUT").is_ok_and(|v| v == "0");
        KEEP_OUT.store(keep_out, Ordering::SeqCst);
        if keep_out {
            log::info!("display cutout: the window stays out of it (RUSTUDIO_CUTOUT=0)");
        }
    }
    !KEEP_OUT.load(Ordering::SeqCst)
}

/// Asks the main thread to lay the window out under the cutout on the
/// short edges (nothing waits for it; before the window is attached the
/// attribute is kept for the attach).
pub fn request() {
    if !wanted() {
        return;
    }
    let Some(app) = super::app() else {
        log::warn!("display cutout: no activity to ask");
        return;
    };
    app.run_on_java_main_thread(Box::new(|| {
        match super::jni::set_layout_in_display_cutout_mode(super::jni::LAYOUT_IN_DISPLAY_CUTOUT_MODE_SHORT_EDGES) {
            Ok(()) => log::info!("display cutout: window laid out under it on the short edges"),
            Err(e) => log::warn!("display cutout: {e}"),
        }
    }));
}

/// The cutout's insets as the editor knows them, and the questions out.
#[derive(Default)]
pub struct Cutout {
    /// The last answer: left, top, right, bottom in window pixels.
    px: [i32; 4],
    /// Whether any answer has come.
    answered: bool,
    /// The question out to the main thread, and when it was asked.
    asked: Option<(Instant, CutoutAnswer)>,
    /// When the next questions are due.
    due: Vec<Instant>,
    /// The size the window had at the last trigger.
    size: Option<egui::Vec2>,
    /// When the first frame came.
    started: Option<Instant>,
}

impl Cutout {
    /// In `raw_input_hook`, after keyboard.rs's `before_frame`: the
    /// questions, their answers, and egui's safe-area insets.
    pub fn before_frame(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        if !wanted() {
            return;
        }
        let now = Instant::now();
        let started = *self.started.get_or_insert(now);
        let size = raw.screen_rect.map(|r| r.size());
        let resized = match (self.size, size) {
            (Some(a), Some(b)) => (a - b).abs().max_elem() >= 1.0,
            (None, Some(_)) => true,
            _ => false,
        };
        let focused = raw.events.iter().any(|e| matches!(e, egui::Event::WindowFocused(true)));
        if resized || focused {
            self.size = size;
            self.due = READINGS.iter().map(|d| now + *d).collect();
        }
        self.take_answer(now);
        // Without an answer yet (no insets before the window is attached):
        // again in a while, for a time.
        if !self.answered && self.asked.is_none() && self.due.is_empty() && now.duration_since(started) < RETRY_FOR {
            self.due.push(now + POLL);
        }
        if self.asked.is_none() && self.due.first().is_some_and(|t| *t <= now) {
            self.due.remove(0);
            self.asked = Some((now, super::jni::display_cutout_later()));
        }
        if self.asked.is_some() || !self.due.is_empty() {
            ctx.request_repaint_after(POLL);
        }
        // egui's safe area: the cutout's sides, with the keyboard's bottom
        // (keyboard.rs set it this frame; the larger of the two below).
        let m = self.margin(ctx.pixels_per_point());
        if m != MarginF32::ZERO {
            let mut sa = raw.safe_area_insets.unwrap_or_default();
            sa.0.left = sa.0.left.max(m.left);
            sa.0.top = sa.0.top.max(m.top);
            sa.0.right = sa.0.right.max(m.right);
            sa.0.bottom = sa.0.bottom.max(m.bottom);
            raw.safe_area_insets = Some(sa);
        }
    }

    /// The last question's answer, once it has come.
    fn take_answer(&mut self, now: Instant) {
        let Some((when, slot)) = &self.asked else { return };
        let when = *when;
        let answer = slot.lock().unwrap_or_else(|e| e.into_inner()).take();
        match answer {
            Some(Ok(px)) => {
                self.asked = None;
                if !self.answered || px != self.px {
                    log::info!("display cutout: insets left {} top {} right {} bottom {} px", px[0], px[1], px[2], px[3]);
                }
                self.answered = true;
                self.px = px;
            }
            Some(Err(e)) => {
                self.asked = None;
                if self.answered {
                    log::warn!("display cutout: {e}");
                } else {
                    log::debug!("display cutout: {e}");
                }
            }
            None if now.duration_since(when) >= GIVE_UP => self.asked = None,
            None => {}
        }
    }

    /// The insets in points.
    fn margin(&self, pixels_per_point: f32) -> MarginF32 {
        let ppp = pixels_per_point.max(0.1);
        let pt = |px: i32| (px.max(0) as f32 / ppp).ceil();
        MarginF32 { left: pt(self.px[0]), right: pt(self.px[2]), top: pt(self.px[1]), bottom: pt(self.px[3]) }
    }

    /// At the start of `App::ui`, after keyboard.rs's `shrink`: the `Ui`
    /// the frame is laid out in — the root's area less the cutout's
    /// insets, the strips given up painted the window's colour (nothing
    /// else draws there). Always a child of the root, insets or none, so
    /// the widgets' ids don't depend on the hole.
    pub fn child(&self, ui: &mut egui::Ui) -> egui::Ui {
        let full = ui.available_rect_before_wrap();
        let mut rect = full;
        if wanted() {
            let m = self.margin(ui.ctx().pixels_per_point());
            rect = egui::Rect::from_min_max(
                egui::pos2(full.min.x + m.left, full.min.y + m.top),
                egui::pos2(full.max.x - m.right, full.max.y - m.bottom),
            );
            if !rect.is_positive() || rect.width() < MIN_SIDE.min(full.width()) || rect.height() < MIN_SIDE.min(full.height()) {
                rect = full;
            }
        }
        if rect != full {
            let painter = ui.ctx().layer_painter(egui::LayerId::background());
            let strips = [
                egui::Rect::from_min_max(full.min, egui::pos2(rect.min.x, full.max.y)),
                egui::Rect::from_min_max(egui::pos2(rect.max.x, full.min.y), full.max),
                egui::Rect::from_min_max(egui::pos2(rect.min.x, full.min.y), egui::pos2(rect.max.x, rect.min.y)),
                egui::Rect::from_min_max(egui::pos2(rect.min.x, rect.max.y), egui::pos2(rect.max.x, full.max.y)),
            ];
            for s in strips {
                if s.is_positive() {
                    painter.rect_filled(s, 0.0, rustudio_engine::theme::BG);
                }
            }
        }
        ui.new_child(egui::UiBuilder::new().max_rect(rect).id_salt("cutout"))
    }
}
