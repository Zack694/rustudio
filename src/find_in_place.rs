//! Native Find & Replace Across All Scripts (`Tab::FindInPlace`, wired to
//! `find_in_place` in Script ribbon & Window > Script > Find in Place).

use egui::RichText;
use rbx_dom_weak::types::{Ref, Variant};
use rbx_dom_weak::ustr;
use rustudio_core::doc;
use rustudio_engine::theme;

use crate::app::Editor;

#[derive(Default)]
pub struct FindInPlaceState {
    pub query: String,
    pub replace_with: String,
    pub case_sensitive: bool,
    pub show_replace: bool,
    pub status: Option<String>,
}

impl Editor {
    pub fn find_in_place_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.find_in_place.query)
                    .hint_text("Search across all scripts…")
                    .desired_width((ui.available_width() - 120.0).max(100.0)),
            );
            ui.checkbox(&mut self.find_in_place.case_sensitive, "Aa").on_hover_text("Case sensitive");
            if ui.small_button(if self.find_in_place.show_replace { "▾" } else { "▸ Replace" }).clicked() {
                self.find_in_place.show_replace = !self.find_in_place.show_replace;
            }
        });

        let mut do_replace_all = false;
        if self.find_in_place.show_replace {
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.find_in_place.replace_with)
                        .hint_text("Replace with…")
                        .desired_width((ui.available_width() - 110.0).max(100.0)),
                );
                if ui.button("Replace All").clicked() && !self.find_in_place.query.is_empty() && self.game.play.is_none() {
                    do_replace_all = true;
                }
            });
        }

        if let Some(msg) = &self.find_in_place.status {
            ui.label(RichText::new(msg).small().color(theme::TEXT_DIM));
        }

        let query = self.find_in_place.query.clone();
        if query.is_empty() {
            ui.add_space(6.0);
            ui.label(RichText::new("Type a search term to search every Script, LocalScript, and ModuleScript in the place.").small().color(theme::TEXT_DIM));
            return;
        }

        let cs = self.find_in_place.case_sensitive;
        let q_low = query.to_ascii_lowercase();

        if do_replace_all {
            let mut edits = Vec::new();
            let mut total_hits = 0usize;
            let mut scripts_hit = 0usize;
            let rep = self.find_in_place.replace_with.clone();
            for inst in self.game.doc.dom.descendants() {
                if !doc::is_a(inst.class.as_str(), "LuaSourceContainer") {
                    continue;
                }
                let Some(Variant::String(src)) = inst.properties.get(&ustr("Source")) else { continue };
                let (new_src, hits) = replace_text(src, &query, &rep, cs);
                if hits > 0 {
                    total_hits += hits;
                    scripts_hit += 1;
                    edits.push((inst.referent(), ustr("Source"), Variant::String(new_src)));
                }
            }
            if !edits.is_empty() {
                for (r, _, v) in &edits {
                    if let (Some(buf), Variant::String(s)) = (self.scripts.bufs.get_mut(r), v) {
                        buf.text = s.clone();
                    }
                }
                self.game.doc.set_props("Replace in Place", edits, None);
            }
            self.find_in_place.status = Some(format!("Replaced {total_hits} match(es) across {scripts_hit} script(s)"));
        }

        // Collect hits
        let mut hits: Vec<(Ref, String, usize, String)> = Vec::new();
        for inst in self.game.doc.dom.descendants() {
            if !doc::is_a(inst.class.as_str(), "LuaSourceContainer") {
                continue;
            }
            let Some(Variant::String(src)) = inst.properties.get(&ustr("Source")) else { continue };
            let matches_script = if cs { src.contains(&query) } else { src.to_ascii_lowercase().contains(&q_low) };
            if !matches_script {
                continue;
            }
            let r = inst.referent();
            let path = self.script_title(r);
            for (i, line) in src.lines().enumerate() {
                let hit = if cs { line.contains(&query) } else { line.to_ascii_lowercase().contains(&q_low) };
                if hit {
                    let mut preview = line.trim().to_string();
                    crate::mcp::truncate_utf8(&mut preview, 140);
                    hits.push((r, path.clone(), i + 1, preview));
                    if hits.len() >= 400 {
                        break;
                    }
                }
            }
            if hits.len() >= 400 {
                break;
            }
        }

        ui.label(RichText::new(format!("{} match(es)", hits.len())).small().color(theme::TEXT_DIM));
        ui.separator();

        let mut open_req: Option<(Ref, usize)> = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for (r, title, line_no, text) in &hits {
                ui.horizontal(|ui| {
                    if ui
                        .selectable_label(false, RichText::new(format!("{title}:{line_no}")).monospace().color(theme::SELECT))
                        .clicked()
                    {
                        open_req = Some((*r, *line_no));
                    }
                    ui.label(RichText::new(text).monospace().small());
                });
            }
        });

        if let Some((r, line)) = open_req {
            self.open_script.push(r);
            self.scripts.goto.insert(r, line);
        }
    }
}

fn replace_text(src: &str, needle: &str, replacement: &str, case_sensitive: bool) -> (String, usize) {
    if needle.is_empty() {
        return (src.to_string(), 0);
    }
    if case_sensitive {
        let count = src.matches(needle).count();
        return (src.replace(needle, replacement), count);
    }
    let low_src = src.to_ascii_lowercase();
    let low_needle = needle.to_ascii_lowercase();
    let mut out = String::with_capacity(src.len());
    let mut count = 0usize;
    let mut pos = 0usize;
    while let Some(idx) = low_src[pos..].find(&low_needle) {
        let abs = pos + idx;
        out.push_str(&src[pos..abs]);
        out.push_str(replacement);
        pos = abs + needle.len();
        count += 1;
    }
    out.push_str(&src[pos..]);
    (out, count)
}
