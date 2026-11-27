//! Script editor tabs: Luau syntax highlighting, line numbers and a live
//! compile check that underlines the line of the first syntax error.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, FontId, RichText, Stroke};
use rbx_dom_weak::types::{Ref, Variant};

use crate::app::Editor;
use crate::icons;
use rustudio_engine::runtime;
use rustudio_engine::theme;

const KEYWORDS: &[&str] = &[
    "and", "break", "continue", "do", "else", "elseif", "end", "export", "false", "for",
    "function", "if", "in", "local", "nil", "not", "or", "repeat", "return", "then", "true",
    "type", "until", "while",
];

const GLOBALS: &[&str] = &[
    "game",
    "workspace",
    "Workspace",
    "script",
    "Instance",
    "Vector3",
    "Vector2",
    "CFrame",
    "Color3",
    "BrickColor",
    "UDim",
    "UDim2",
    "Enum",
    "task",
    "print",
    "warn",
    "error",
    "require",
    "typeof",
    "type",
    "math",
    "string",
    "table",
    "pcall",
    "xpcall",
    "pairs",
    "ipairs",
    "next",
    "select",
    "tostring",
    "tonumber",
    "setmetatable",
    "getmetatable",
    "assert",
    "wait",
    "spawn",
    "delay",
    "tick",
    "time",
    "coroutine",
    "buffer",
    "bit32",
    "utf8",
    "os",
    "debug",
    "self",
    "shared",
    "_G",
    "rawget",
    "rawset",
    "rawequal",
    "unpack",
];


#[derive(Debug, Clone, Copy, PartialEq)]
enum Tok {
    Plain,
    Keyword,
    Global,
    Str,
    Num,
    Comment,
    Op,
}

/// Splits Luau source into coloured spans (byte ranges).
fn tokenize(src: &str) -> Vec<(std::ops::Range<usize>, Tok)> {
    let b = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    // `[[ ... ]]` / `[==[ ... ]==]` long brackets; returns the end index.
    let long_bracket = |at: usize| -> Option<usize> {
        if b.get(at) != Some(&b'[') {
            return None;
        }
        let mut j = at + 1;
        let mut level = 0;
        while b.get(j) == Some(&b'=') {
            level += 1;
            j += 1;
        }
        if b.get(j) != Some(&b'[') {
            return None;
        }
        let close: String = format!("]{}]", "=".repeat(level));
        Some(
            src[j + 1..]
                .find(&close)
                .map(|p| j + 1 + p + close.len())
                .unwrap_or(src.len()),
        )
    };
    while i < b.len() {
        let c = b[i];
        let start = i;
        if c == b'-' && b.get(i + 1) == Some(&b'-') {
            i = long_bracket(i + 2)
                .unwrap_or_else(|| src[i..].find('\n').map(|p| i + p).unwrap_or(src.len()));
            out.push((start..i, Tok::Comment));
        } else if c == b'"' || c == b'\'' || c == b'`' {
            i += 1;
            while i < b.len() && b[i] != c && b[i] != b'\n' {
                i += if b[i] == b'\\' { 2 } else { 1 };
            }
            i = (i + 1).min(b.len());
            out.push((start..i, Tok::Str));
        } else if let Some(end) = (c == b'[').then(|| long_bracket(i)).flatten() {
            i = end;
            out.push((start..i, Tok::Str));
        } else if c.is_ascii_digit()
            || (c == b'.' && b.get(i + 1).is_some_and(|d| d.is_ascii_digit()))
        {
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'.' || b[i] == b'_') {
                i += 1;
            }
            out.push((start..i, Tok::Num));
        } else if c.is_ascii_alphabetic() || c == b'_' {
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            let word = &src[start..i];
            let after_dot = start > 0 && b[start - 1] == b'.' || start > 0 && b[start - 1] == b':';
            let t = if KEYWORDS.contains(&word) {
                Tok::Keyword
            } else if GLOBALS.contains(&word) && !after_dot {
                Tok::Global
            } else {
                Tok::Plain
            };
            out.push((start..i, t));
        } else if c.is_ascii_punctuation() {
            i += 1;
            out.push((start..i, Tok::Op));
        } else {
            // Whitespace and anything else (including multi-byte UTF-8).
            i += 1;
            while i < b.len() && !src.is_char_boundary(i) {
                i += 1;
            }
            out.push((start..i, Tok::Plain));
        }
    }
    out
}

/// Byte range of a 1-based line.
fn line_range(src: &str, line: usize) -> Option<std::ops::Range<usize>> {
    let mut start = 0;
    for (n, l) in src.split('\n').enumerate() {
        if n + 1 == line {
            return Some(start..start + l.len());
        }
        start += l.len() + 1;
    }
    None
}

/// A highlighted layout job; the error line gets a red wavy-looking underline.
pub fn highlight(src: &str, error_line: Option<usize>, font: FontId) -> LayoutJob {
    let mut job = LayoutJob::default();
    let err = error_line.and_then(|l| line_range(src, l));
    // Studio Settings > Script Editor > Theme.
    let sc = crate::studio_settings::script_colors();
    for (range, tok) in tokenize(src) {
        let color = match tok {
            Tok::Plain => sc.text,
            Tok::Keyword => sc.keyword,
            Tok::Global => sc.builtin,
            Tok::Str => sc.string,
            Tok::Num => sc.number,
            Tok::Comment => sc.comment,
            Tok::Op => sc.operator,
        };
        // Split the token where it crosses into / out of the error line.
        let mut cuts = vec![range.start, range.end];
        if let Some(e) = &err {
            for p in [e.start, e.end] {
                if p > range.start && p < range.end {
                    cuts.push(p);
                }
            }
        }
        cuts.sort_unstable();
        for w in cuts.windows(2) {
            let (a, z) = (w[0], w[1]);
            if a == z {
                continue;
            }
            let on_err = err
                .as_ref()
                .is_some_and(|e| a >= e.start && z <= e.end && e.start != e.end);
            let fmt = TextFormat {
                font_id: font.clone(),
                color,
                italics: tok == Tok::Comment,
                underline: if on_err {
                    Stroke::new(1.5, theme::ERROR)
                } else {
                    Stroke::NONE
                },
                background: if on_err {
                    Color32::from_rgba_unmultiplied(240, 96, 86, 28)
                } else {
                    Color32::TRANSPARENT
                },
                ..Default::default()
            };
            job.append(&src[a..z], 0.0, fmt);
        }
    }
    job
}

pub struct ScriptBuf {
    pub text: String,
    pub error: Option<(usize, String)>,
    checked: Option<String>,
    edited: Instant,
    /// Plugins' ScriptEditorService analysis results (line, message) and
    /// autocomplete items (label, text) for the text they were made for.
    plugin_diags: Vec<(i64, String)>,
    plugin_checked: Option<String>,
    completions: Vec<(String, String)>,
}

#[derive(Default)]
pub struct ScriptTabs {
    pub bufs: HashMap<Ref, ScriptBuf>,
    /// Lines (1-based) to put the cursor on and scroll to once the tab
    /// draws (the profiler's "Open script at line N").
    pub goto: HashMap<Ref, usize>,
    pub active: Option<Ref>,
    pub show_find: bool,
    pub find_query: String,
    pub replace_with: String,
    pub show_goto: bool,
    pub goto_input: String,
}

const SCRIPT_SNIPPETS: &[(&str, &str)] = &[
    ("PlayerAdded", "local Players = game:GetService(\"Players\")\nPlayers.PlayerAdded:Connect(function(player)\n\tprint(player.Name, \"joined\")\nend)\n"),
    ("InputBegan", "local UIS = game:GetService(\"UserInputService\")\nUIS.InputBegan:Connect(function(input, gp)\n\tif gp then return end\n\tif input.KeyCode == Enum.KeyCode.E then\n\t\t\n\tend\nend)\n"),
    ("Heartbeat", "local RunService = game:GetService(\"RunService\")\nRunService.Heartbeat:Connect(function(dt)\n\t\nend)\n"),
    ("Tween", "local TweenService = game:GetService(\"TweenService\")\nTweenService:Create(part, TweenInfo.new(0.5, Enum.EasingStyle.Quad, Enum.EasingDirection.Out), {\n\tPosition = part.Position + Vector3.new(0, 5, 0),\n}):Play()\n"),
    ("Raycast", "local params = RaycastParams.new()\nparams.FilterType = Enum.RaycastFilterType.Exclude\nlocal res = workspace:Raycast(origin, dir * 100, params)\nif res then\n\tprint(res.Instance, res.Position)\nend\n"),
    ("Pathfinding", "local PathfindingService = game:GetService(\"PathfindingService\")\nlocal path = PathfindingService:CreatePath({ AgentRadius = 2, AgentHeight = 5, AgentCanJump = true })\npath:ComputeAsync(startPos, goalPos)\nif path.Status == Enum.PathStatus.Success then\n\tfor _, wp in ipairs(path:GetWaypoints()) do\n\t\thumanoid:MoveTo(wp.Position)\n\t\thumanoid.MoveToFinished:Wait()\n\tend\nend\n"),
    ("RemoteEvent", "local ReplicatedStorage = game:GetService(\"ReplicatedStorage\")\nlocal remote = ReplicatedStorage:WaitForChild(\"MyRemote\")\nremote.OnServerEvent:Connect(function(player, ...)\n\t\nend)\n"),
];

/// Simple Luau auto-indenter for Script > Format Document.
pub fn format_luau(src: &str) -> String {
    let mut out = String::with_capacity(src.len() + 64);
    let mut indent: usize = 0;
    for raw in src.lines() {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            out.push('\n');
            continue;
        }
        let first = trimmed.split(|c: char| !c.is_alphanumeric() && c != '_').next().unwrap_or("");
        let dedent = matches!(first, "end" | "else" | "elseif" | "until") || trimmed.starts_with('}') || trimmed.starts_with(')');
        if dedent {
            indent = indent.saturating_sub(1);
        }
        for _ in 0..indent {
            out.push('\t');
        }
        out.push_str(trimmed);
        out.push('\n');
        let opens = (trimmed.ends_with("then") || trimmed.ends_with("do") || trimmed.ends_with("else") || trimmed.ends_with("repeat") || trimmed.ends_with('{') || trimmed.starts_with("function ") || trimmed.starts_with("local function "))
            && !trimmed.ends_with("end");
        if opens {
            indent += 1;
        }
    }
    out
}

/// Built-in Luau + Roblox API autocomplete (fallback when no plugin provides completions).
fn builtin_completions(text: &str, cursor_chars: usize) -> Vec<(String, String)> {
    let chars: Vec<char> = text.chars().collect();
    let end = cursor_chars.min(chars.len());
    let mut start = end;
    while start > 0 && (chars[start - 1].is_alphanumeric() || chars[start - 1] == '_') {
        start -= 1;
    }
    let prefix: String = chars[start..end].iter().collect();
    if prefix.len() < 2 {
        return Vec::new();
    }
    let low = prefix.to_ascii_lowercase();
    let mut out: Vec<(String, String)> = Vec::new();
    fn push_item(out: &mut Vec<(String, String)>, low: &str, prefix: &str, item: &str) {
        if item.to_ascii_lowercase().starts_with(low) && item != prefix && !out.iter().any(|(l, _)| l == item) {
            out.push((item.to_string(), item.to_string()));
        }
    }
    for &k in KEYWORDS {
        push_item(&mut out, &low, &prefix, k);
    }
    for &g in GLOBALS {
        push_item(&mut out, &low, &prefix, g);
    }
    for svc in [
        "Players", "Workspace", "ReplicatedStorage", "ServerScriptService", "ServerStorage",
        "StarterGui", "StarterPack", "StarterPlayer", "Lighting", "RunService",
        "UserInputService", "TweenService", "PathfindingService", "CollectionService",
        "PhysicsService", "HttpService", "DataStoreService", "TeleportService",
        "ContextActionService", "SoundService", "Debris", "MarketplaceService",
        "KeyframeSequenceProvider", "TextService", "GuiService", "ProximityPromptService",
        "GetService", "WaitForChild", "FindFirstChild", "FindFirstChildOfClass",
        "GetChildren", "GetDescendants", "IsA", "Connect", "Once", "Clone", "Destroy",
    ] {
        push_item(&mut out, &low, &prefix, svc);
    }
    if out.len() < 12 {
        if let Some(api) = rustudio_core::api::get() {
            for cname in &api.class_order {
                push_item(&mut out, &low, &prefix, cname);
                if out.len() >= 12 {
                    break;
                }
            }
        }
    }
    out.truncate(12);
    out
}

impl Editor {
    pub fn script_title(&self, r: Ref) -> String {
        match self.game.doc.dom.get_by_ref(r) {
            Some(i) => i.name.clone(),
            None => "Script".to_string(),
        }
    }

    /// Formats the currently active script tab's Luau indentation.
    pub fn format_active_script(&mut self) {
        let Some(r) = self.scripts.active else { return };
        if self.game.play.is_some() {
            return;
        }
        if let Some(buf) = self.scripts.bufs.get_mut(&r) {
            let formatted = format_luau(&buf.text);
            if formatted != buf.text {
                buf.text = formatted.clone();
                buf.checked = None;
                self.game.doc.set_props("Format script", vec![(r, rbx_dom_weak::ustr("Source"), Variant::String(formatted))], None);
            }
        }
    }

    pub fn script_tab_ui(&mut self, ui: &mut egui::Ui, r: Ref) {
        self.scripts.active = Some(r);
        if ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::F)) {
            self.scripts.show_find = !self.scripts.show_find;
        }
        if ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::G)) {
            self.scripts.show_goto = !self.scripts.show_goto;
        }
        self.live_draft_toolbar(ui,r);
        let Some(inst) = self.game.doc.dom.get_by_ref(r) else {
            ui.add_space(12.0);
            ui.label(RichText::new("This script is not in the DataModel being shown (it may belong to the edit copy while playing).").color(theme::TEXT_DIM));
            return;
        };
        let class = inst.class.to_string();
        let source = match self.game.doc.prop(r, "Source") {
            Some(Variant::String(s)) => s,
            _ => String::new(),
        };
        let full_name = self.full_name(r);
        let follow_live_source=self.live_script_has_draft(r)==Some(false);
        // A tab on screen is an open ScriptEditorService document.
        if !crate::plugin_host::script_docs::is_open(r) {
            crate::plugin_host::script_docs::open(r, None, true);
        }
        let buf = self.scripts.bufs.entry(r).or_insert_with(|| ScriptBuf {
            text: source.clone(),
            error: None,
            checked: None,
            edited: Instant::now(),
            plugin_diags: Vec::new(),
            plugin_checked: None,
            completions: Vec::new(),
        });
        let editor_id = egui::Id::new(("script_text", r));
        let goto = self.scripts.goto.get(&r).copied();
        let focused = ui.ctx().memory(|m| m.has_focus(editor_id));
        // Undo/redo or a script change elsewhere: follow the document.
        if (!focused || follow_live_source) && buf.text != source {
            buf.text = source.clone();
            buf.checked = None;
        }
        // Compile check once typing pauses.
        if buf.checked.as_deref() != Some(buf.text.as_str())
            && buf.edited.elapsed() > Duration::from_millis(250)
        {
            buf.error = runtime::check_syntax(&buf.text, &full_name);
            buf.checked = Some(buf.text.clone());
        }
        // Plugins' script analysis callbacks (ScriptEditorService), once
        // typing pauses too.
        let analyze = buf.plugin_checked.as_deref() != Some(buf.text.as_str())
            && buf.edited.elapsed() > Duration::from_millis(250);
        if analyze {
            buf.plugin_checked = Some(buf.text.clone());
            let diags = self
                .plugins
                .with_edit(&mut self.game.doc, |rt| {
                    let lua = &rt.lua;
                    if crate::plugin_host::api::script_editor::has_callbacks(lua).1 {
                        crate::plugin_host::api::script_editor::analyze(lua, r).unwrap_or_default()
                    } else {
                        Vec::new()
                    }
                })
                .unwrap_or_default();
            if let Some(b) = self.scripts.bufs.get_mut(&r) {
                b.plugin_diags = diags;
            }
        }
        let Some(buf) = self.scripts.bufs.get_mut(&r) else { return };

        let mut toolbar_changed = false;
        ui.horizontal(|ui| {
            icons::class_icon(ui, &class, 16.0);
            ui.label(RichText::new(&full_name).strong());
            ui.label(
                RichText::new(format!("{} lines", buf.text.lines().count().max(1)))
                    .small()
                    .color(theme::TEXT_DIM),
            );
            match &buf.error {
                Some((_, msg)) => {
                    ui.label(RichText::new(format!("⚠ {msg}")).color(theme::ERROR));
                }
                None => {
                    let color = Color32::from_rgb(130, 200, 120);
                    ui.label(rustudio_engine::ti::text("check", 12.0).color(color));
                    ui.label(RichText::new("no syntax errors").small().color(color));
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                egui::ComboBox::from_id_salt(("script_snippets", r)).selected_text("+ Snippet").width(84.0).show_ui(ui, |ui| {
                    for &(sname, scode) in SCRIPT_SNIPPETS {
                        if ui.selectable_label(false, sname).clicked() {
                            if !buf.text.ends_with('\n') && !buf.text.is_empty() {
                                buf.text.push('\n');
                            }
                            buf.text.push_str(scode);
                            buf.edited = Instant::now();
                            buf.checked = None;
                            toolbar_changed = true;
                        }
                    }
                });
                if ui.small_button("Format").on_hover_text("Auto-indent Luau").clicked() {
                    buf.text = format_luau(&buf.text);
                    buf.edited = Instant::now();
                    buf.checked = None;
                    toolbar_changed = true;
                }
                if ui.small_button("Line").on_hover_text("Go to line (Ctrl+G)").clicked() {
                    self.scripts.show_goto = !self.scripts.show_goto;
                }
                if ui.small_button("Find").on_hover_text("Find & Replace (Ctrl+F)").clicked() {
                    self.scripts.show_find = !self.scripts.show_find;
                }
            });
        });
        if self.scripts.show_find {
            ui.horizontal_wrapped(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.scripts.find_query).hint_text("Find…").desired_width(130.0));
                ui.add(egui::TextEdit::singleline(&mut self.scripts.replace_with).hint_text("Replace…").desired_width(130.0));
                let q = &self.scripts.find_query;
                let hits = if q.is_empty() { 0 } else { buf.text.matches(q.as_str()).count() };
                ui.label(RichText::new(format!("{hits} hit(s)")).small().color(theme::TEXT_DIM));
                if ui.small_button("Replace All").clicked() && !q.is_empty() {
                    buf.text = buf.text.replace(q.as_str(), &self.scripts.replace_with);
                    buf.edited = Instant::now();
                    toolbar_changed = true;
                }
                if ui.small_button("×").clicked() {
                    self.scripts.show_find = false;
                }
            });
        }
        if self.scripts.show_goto {
            ui.horizontal(|ui| {
                ui.label("Go to line:");
                let resp = ui.add(egui::TextEdit::singleline(&mut self.scripts.goto_input).desired_width(64.0));
                if (resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) || ui.small_button("Go").clicked() {
                    if let Ok(line_no) = self.scripts.goto_input.trim().parse::<usize>() {
                        if line_no > 0 {
                            self.scripts.goto.insert(r, line_no);
                            self.scripts.show_goto = false;
                        }
                    }
                }
            });
        }
        for (line, msg) in &buf.plugin_diags {
            ui.label(RichText::new(format!("⚠ line {line}: {msg}")).small().color(theme::ERROR));
        }
        // Plugins' autocomplete items for the word at the cursor: a click
        // puts the item in place of the word.
        let mut accept: Option<String> = None;
        if !buf.completions.is_empty() {
            ui.horizontal_wrapped(|ui| {
                for (label, insert) in buf.completions.iter().take(12) {
                    if ui.small_button(label).clicked() {
                        accept = Some(insert.clone());
                    }
                }
            });
        }
        ui.separator();

        // Studio Settings > Script Editor > Font (size), Background Color,
        // Text Wrapping.
        let font = FontId::monospace(crate::studio_settings::font_px("Studio", "Font"));
        let row_h = ui.fonts_mut(|f| f.row_height(&font));
        let wrap = crate::studio_settings::flag("Studio", "Text Wrapping", false);
        let bg = crate::studio_settings::script_colors().background;
        ui.painter().rect_filled(ui.available_rect_before_wrap(), 0.0, bg);
        let error_line = buf.error.as_ref().map(|e| e.0);
        let mut changed = toolbar_changed;
        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .id_salt(("script_scroll", r))
            .show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    // Line numbers.
                    let lines = buf.text.split('\n').count().max(1);
                    let digits = lines.to_string().len().max(2);
                    let mut gutter = LayoutJob::default();
                    for n in 1..=lines {
                        let c = if Some(n) == error_line {
                            theme::ERROR
                        } else {
                            theme::TEXT_DIM
                        };
                        let nl = if n == lines { "" } else { "\n" };
                        gutter.append(
                            &format!("{n:>digits$}{nl}"),
                            0.0,
                            TextFormat {
                                font_id: font.clone(),
                                color: c,
                                ..Default::default()
                            },
                        );
                    }
                    let galley = ui.fonts_mut(|f| f.layout_job(gutter));
                    let (rect, _) = ui.allocate_exact_size(
                        egui::vec2(galley.size().x + 10.0, galley.size().y.max(row_h)),
                        egui::Sense::hover(),
                    );
                    ui.painter().rect_filled(rect, 0.0, theme::PANEL_DARK);
                    ui.painter()
                        .galley(rect.min + egui::vec2(4.0, 2.0), galley, theme::TEXT_DIM);

                    let mut layouter = |ui: &egui::Ui,
                                        text: &dyn egui::TextBuffer,
                                        wrap_w: f32|
                     -> Arc<egui::Galley> {
                        let mut job = highlight(text.as_str(), error_line, font.clone());
                        if wrap {
                            job.wrap.max_width = wrap_w;
                        }
                        ui.fonts_mut(|f| f.layout_job(job))
                    };
                    let out = egui::TextEdit::multiline(&mut buf.text)
                        .id(editor_id)
                        .code_editor()
                        .frame(egui::Frame::NONE)
                        .desired_width(f32::INFINITY)
                        .desired_rows(lines.max(24))
                        .lock_focus(true)
                        .layouter(&mut layouter)
                        .show(ui);
                    // The soft keyboard came up under the editor: the caret
                    // scrolls into the room left above it.
                    #[cfg(target_os = "android")]
                    crate::android::keyboard::reveal_cursor(ui, &out);
                    let resp = out.response.response;
                    if resp.changed() {
                        changed = true;
                        buf.edited = Instant::now();
                    }
                    if let Some(line) = goto {
                        let y = resp.rect.top() + line.saturating_sub(1) as f32 * row_h;
                        ui.scroll_to_rect(egui::Rect::from_min_size(egui::pos2(resp.rect.left(), y), egui::vec2(1.0, row_h)), Some(egui::Align::Center));
                    }
                });
            });
        // The cursor / selection as the plugins' ScriptDocument sees it
        // (Studio's 1-based line, character), and selections they set.
        let mut state = egui::TextEdit::load_state(ui.ctx(), editor_id);
        let mut word_end: Option<usize> = None;
        if let Some(st) = state.as_mut() {
            if let Some(cr) = st.cursor.char_range() {
                let (a, b) = (cr.primary.index.0, cr.secondary.index.0);
                let pa = crate::plugin_host::script_docs::pos_of(&buf.text, a);
                let pb = crate::plugin_host::script_docs::pos_of(&buf.text, b);
                crate::plugin_host::script_docs::set_selection(r, (pa.0, pa.1, pb.0, pb.1), true);
                if a == b {
                    word_end = Some(a);
                }
            }
        }
        if let (Some(line), Some(mut st)) = (goto, state.clone()) {
            // (the profiler's jump: the cursor at the line's start, focused)
            let at = crate::plugin_host::script_docs::index_of(&buf.text, line as i64, 1);
            st.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(at))));
            st.store(ui.ctx(), editor_id);
            ui.ctx().memory_mut(|m| m.request_focus(editor_id));
            self.scripts.goto.remove(&r);
        }
        if let Some((sel, mut st)) = crate::plugin_host::script_docs::take_selection_request(r).zip(state) {
            let a = crate::plugin_host::script_docs::index_of(&buf.text, sel.0, sel.1);
            let b = crate::plugin_host::script_docs::index_of(&buf.text, sel.2, sel.3);
            st.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(b), egui::text::CCursor::new(a))));
            st.store(ui.ctx(), editor_id);
        }
        if let (Some(insert), Some(end)) = (accept, word_end) {
            let chars: Vec<char> = buf.text.chars().collect();
            let end = end.min(chars.len());
            let mut start = end;
            while start > 0 && (chars[start - 1].is_alphanumeric() || chars[start - 1] == '_') {
                start -= 1;
            }
            buf.text = chars[..start].iter().chain(insert.chars().collect::<Vec<_>>().iter()).chain(chars[end..].iter()).collect();
            buf.completions.clear();
            changed = true;
        }
        if changed {
            let text = buf.text.clone();
            // ScriptEditorService's TextDocumentDidChange for the person's typing.
            if let Some(c) = crate::plugin_host::script_docs::diff(&source, &text) {
                crate::plugin_host::script_docs::changed(r, vec![c]);
            }
            // Plugins' autocomplete for the word being typed.
            let pos = word_end.map(|i| crate::plugin_host::script_docs::pos_of(&text, i));
            self.game.doc.set_props(
                "Edit script",
                vec![(r, rbx_dom_weak::ustr("Source"), Variant::String(text.clone()))],
                Some(format!("source:{r:?}")),
            );
            let typing_word = word_end.is_some_and(|i| {
                let c: Vec<char> = text.chars().collect();
                i > 0 && i <= c.len() && (c[i - 1].is_alphanumeric() || c[i - 1] == '_' || c[i - 1] == '.' || c[i - 1] == ':')
            });
            let items = match (pos, typing_word) {
                (Some((line, ch)), true) => self
                    .plugins
                    .with_edit(&mut self.game.doc, |rt| {
                        let lua = &rt.lua;
                        if crate::plugin_host::api::script_editor::has_callbacks(lua).0 {
                            crate::plugin_host::api::script_editor::autocomplete(lua, r, line, ch).unwrap_or_default()
                        } else {
                            Vec::new()
                        }
                    })
                    .unwrap_or_default(),
                _ => Vec::new(),
            };
            let final_items = if items.is_empty() {
                word_end.map(|end| builtin_completions(&text, end)).unwrap_or_default()
            } else {
                items
            };
            if let Some(b) = self.scripts.bufs.get_mut(&r) {
                b.completions = final_items;
            }
        }
        if buf_needs_repaint(self.scripts.bufs.get(&r)) {
            ui.ctx().request_repaint_after(Duration::from_millis(300));
        }
    }

    /// Opens the script at instance path `path` (as the profiler names it),
    /// at `line` if given. False when no script has that path.
    pub fn open_script_named(&mut self, path: &str, line: Option<u32>) -> bool {
        let found = self
            .game
            .doc
            .dom
            .descendants()
            .filter(|i| matches!(i.class.as_str(), "Script" | "LocalScript" | "ModuleScript"))
            .map(|i| i.referent())
            .find(|&r| self.full_name(r) == path);
        match found {
            Some(r) => {
                self.open_script.push(r);
                if let Some(l) = line {
                    self.scripts.goto.insert(r, l as usize);
                }
                true
            }
            None => {
                self.warn(format!("MicroProfiler: no script named {path} in the DataModel being shown"));
                false
            }
        }
    }

    fn full_name(&self, r: Ref) -> String {
        let root = self.game.doc.dom.root_ref();
        let mut parts = Vec::new();
        let mut cur = r;
        while let Some(i) = self.game.doc.dom.get_by_ref(cur) {
            if cur == root {
                break;
            }
            parts.push(i.name.clone());
            cur = i.parent();
        }
        parts.reverse();
        parts.join(".")
    }
}

fn buf_needs_repaint(b: Option<&ScriptBuf>) -> bool {
    b.is_some_and(|b| b.checked.as_deref() != Some(b.text.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens() {
        let t = tokenize("local x = 5 -- hi\nprint(\"a\")");
        let kinds: Vec<Tok> = t
            .iter()
            .filter(|(_, k)| *k != Tok::Plain || false)
            .map(|(_, k)| *k)
            .collect();
        assert_eq!(kinds[0], Tok::Keyword);
        assert!(
            kinds.contains(&Tok::Num)
                && kinds.contains(&Tok::Comment)
                && kinds.contains(&Tok::Str)
                && kinds.contains(&Tok::Global)
        );
    }

    #[test]
    fn syntax_error_line() {
        let e = runtime::check_syntax("local a = 1\nlocal = 2\n", "Workspace.S").unwrap();
        assert_eq!(e.0, 2, "{e:?}");
    }
}
