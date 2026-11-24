//! Native Roblox Studio Command Bar (`Tab::CommandBar`, wired to `command_bar`):
//! evaluates Luau immediately against the Edit DataModel (or running Server /
//! Client during Play), with command history (Up/Down), return-value display,
//! and quick one-click Studio macros.

use egui::{Color32, RichText};
use rustudio_engine::runtime::Side;
use rustudio_engine::theme;

use crate::app::Editor;

pub struct CommandBarState {
    pub input: String,
    pub history: Vec<String>,
    pub hist_pos: Option<usize>,
    pub target_play_side: Side,
    pub last_status: Option<(bool, String)>,
    pending_eval: Option<(Option<Side>, u64)>,
    next_id: u64,
}

impl Default for CommandBarState {
    fn default() -> Self {
        CommandBarState {
            input: String::new(),
            history: Vec::new(),
            hist_pos: None,
            target_play_side: Side::Server,
            last_status: None,
            pending_eval: None,
            next_id: 900_000,
        }
    }
}

const MACROS: &[(&str, &str)] = &[
    ("Count Parts", "local n=0 for _,v in ipairs(workspace:GetDescendants()) do if v:IsA('BasePart') then n+=1 end end print('BaseParts:', n) return n"),
    ("Anchor All", "local n=0 for _,v in ipairs(workspace:GetDescendants()) do if v:IsA('BasePart') then v.Anchored=true n+=1 end end print('Anchored', n, 'parts')"),
    ("Unanchor Sel", "for _,v in ipairs(game:GetService('Selection'):Get()) do if v:IsA('BasePart') then v.Anchored=false end end print('Unanchored selection')"),
    ("Print Services", "for _,s in ipairs(game:GetChildren()) do print(s.ClassName, s.Name, #s:GetChildren()) end"),
    ("Reset Lighting", "local L=game:GetService('Lighting') L.ClockTime=14 L.Brightness=2.5 L.FogEnd=100000 print('Lighting reset')"),
];

impl Editor {
    /// Executes `code` via the Command Bar (Edit DataModel when stopped,
    /// Server or Client during Play).
    pub fn run_command_bar(&mut self, code: &str) {
        let trimmed = code.trim();
        if trimmed.is_empty() {
            return;
        }
        if self.command_bar.history.last().map(|s| s.as_str()) != Some(trimmed) {
            self.command_bar.history.push(trimmed.to_string());
            if self.command_bar.history.len() > 200 {
                self.command_bar.history.remove(0);
            }
        }
        self.command_bar.hist_pos = None;
        self.command_bar.next_id += 1;
        let id = self.command_bar.next_id;

        if self.game.play.is_some() {
            let side = self.command_bar.target_play_side;
            match self.game.mcp_eval_start(side, trimmed, id) {
                Ok(()) => {
                    self.command_bar.pending_eval = Some((Some(side), id));
                    self.command_bar.last_status = Some((true, format!("Running on {side:?}…")));
                }
                Err(e) => {
                    self.error(format!("Command Bar: {e}"));
                    self.command_bar.last_status = Some((false, e));
                }
            }
            return;
        }

        let identity = rustudio_engine::runtime::security::COMMAND;
        let res = self.plugins.with_edit_vm(&mut self.game.doc, |rt| -> Result<Option<serde_json::Value>, String> {
            rustudio_player::luau_eval::start(&rt.lua, trimmed, id, identity)?;
            rt.dispatch();
            Ok(rustudio_player::luau_eval::poll(&rt.lua, id))
        });
        match res {
            Ok(Ok(Some(v))) => {
                self.game.doc.commit_pending_script_changes("Command Bar");
                self.finish_command_bar_val(v);
            }
            Ok(Ok(None)) => {
                self.command_bar.pending_eval = Some((None, id));
                self.command_bar.last_status = Some((true, "Running…".into()));
            }
            Ok(Err(e)) | Err(e) => {
                self.error(format!("Command Bar: {e}"));
                self.command_bar.last_status = Some((false, e));
            }
        }
    }

    fn finish_command_bar_val(&mut self, v: serde_json::Value) {
        let ok = v["ok"].as_bool().unwrap_or(false);
        if ok {
            let vals = &v["values"];
            let msg = if vals.as_array().is_some_and(|a| !a.is_empty()) {
                format!("=> {vals}")
            } else {
                "OK".into()
            };
            self.info(format!("> {}", msg));
            self.command_bar.last_status = Some((true, msg));
        } else {
            let err = v["error"].as_str().unwrap_or("error").to_string();
            self.error(format!("Command Bar: {err}"));
            self.command_bar.last_status = Some((false, err));
        }
    }

    /// Native Command Bar dock panel.
    pub fn command_bar_ui(&mut self, ui: &mut egui::Ui) {
        // Poll async yielding evals.
        if let Some((side, id)) = self.command_bar.pending_eval {
            let got = match side {
                None => self
                    .plugins
                    .with_edit_vm(&mut self.game.doc, |rt| rustudio_player::luau_eval::poll(&rt.lua, id))
                    .ok()
                    .flatten(),
                Some(s) => self.game.mcp_eval_poll(s, id),
            };
            if let Some(v) = got {
                self.command_bar.pending_eval = None;
                if side.is_none() {
                    self.game.doc.commit_pending_script_changes("Command Bar");
                }
                self.finish_command_bar_val(v);
            } else {
                ui.ctx().request_repaint();
            }
        }

        let mut run_now: Option<String> = None;

        ui.horizontal(|ui| {
            let mode_badge = if self.game.play.is_some() {
                if ui
                    .small_button(format!("{:?}", self.command_bar.target_play_side))
                    .on_hover_text("Click to toggle Server / Client target during Play")
                    .clicked()
                {
                    self.command_bar.target_play_side = match self.command_bar.target_play_side {
                        Side::Server => Side::Client,
                        Side::Client => Side::Server,
                    };
                }
                ""
            } else {
                "Edit >"
            };
            if !mode_badge.is_empty() {
                ui.label(RichText::new(mode_badge).monospace().strong().color(theme::SELECT));
            }

            let resp = ui.add(
                egui::TextEdit::singleline(&mut self.command_bar.input)
                    .font(egui::TextStyle::Monospace)
                    .hint_text("Run a Luau command… (Enter to execute, ↑/↓ history)")
                    .desired_width((ui.available_width() - 120.0).max(160.0)),
            );

            if resp.has_focus() {
                if ui.input(|i| i.key_pressed(egui::Key::ArrowUp)) && !self.command_bar.history.is_empty() {
                    let next = match self.command_bar.hist_pos {
                        None => self.command_bar.history.len() - 1,
                        Some(p) => p.saturating_sub(1),
                    };
                    self.command_bar.hist_pos = Some(next);
                    self.command_bar.input = self.command_bar.history[next].clone();
                } else if ui.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
                    if let Some(p) = self.command_bar.hist_pos {
                        if p + 1 < self.command_bar.history.len() {
                            self.command_bar.hist_pos = Some(p + 1);
                            self.command_bar.input = self.command_bar.history[p + 1].clone();
                        } else {
                            self.command_bar.hist_pos = None;
                            self.command_bar.input.clear();
                        }
                    }
                }
            }

            if (resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) || ui.button("Run").clicked() {
                run_now = Some(self.command_bar.input.clone());
                self.command_bar.input.clear();
                resp.request_focus();
            }

            egui::ComboBox::from_id_salt("cmd_macros").selected_text("Macros").width(72.0).show_ui(ui, |ui| {
                for &(label, code) in MACROS {
                    if ui.selectable_label(false, label).clicked() {
                        run_now = Some(code.to_string());
                    }
                }
            });
        });

        if let Some((ok, msg)) = &self.command_bar.last_status {
            let col = if *ok { Color32::from_rgb(130, 210, 140) } else { theme::ERROR };
            ui.label(RichText::new(msg).small().monospace().color(col));
        }

        if let Some(code) = run_now {
            self.run_command_bar(&code);
        }
    }
}
