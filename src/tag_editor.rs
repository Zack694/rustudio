//! Native CollectionService Tag Editor (`Tab::TagEditor`) and PhysicsService
//! Collision Groups Matrix (`Tab::CollisionGroups`) panels.

use std::collections::BTreeSet;

use egui::RichText;
use rbx_dom_weak::types::{Ref, Tags, Variant};
use rbx_dom_weak::ustr;
use rustudio_core::collision_groups::{self, CollisionGroups};
use rustudio_core::doc;
use rustudio_engine::theme;

use crate::app::Editor;

#[derive(Default)]
pub struct TagEditorState {
    pub filter: String,
    pub new_tag: String,
    pub custom_tags: BTreeSet<String>,
    pub new_group: String,
    pub groups: CollisionGroups,
    pub groups_synced_doc: u64,
    pub status: Option<String>,
}

/// Parses Roblox's `Workspace.CollisionGroups` string format (`Name^id^mask\...`).
fn parse_collision_groups_str(raw: &str) -> CollisionGroups {
    let mut cg = CollisionGroups::default();
    let mut entries: Vec<(String, u32, i32)> = Vec::new();
    for entry in raw.split('\\') {
        let parts: Vec<&str> = entry.split('^').collect();
        if parts.len() == 3 {
            if let (Ok(id), Ok(mask)) = (parts[1].parse::<u32>(), parts[2].parse::<i32>()) {
                entries.push((parts[0].to_string(), id, mask));
            }
        }
    }
    entries.sort_by_key(|(_, id, _)| *id);
    for (name, _, _) in &entries {
        if name != collision_groups::DEFAULT {
            let _ = cg.register(name);
        }
    }
    for (name_a, _, mask_a) in &entries {
        for (name_b, id_b, _) in &entries {
            if *id_b < 32 {
                let collides = (*mask_a & (1i32.wrapping_shl(*id_b))) != 0;
                if !collides {
                    cg.set_collidable(name_a, name_b, false);
                }
            }
        }
    }
    cg
}

/// Encodes `CollisionGroups` back to Roblox's `Workspace.CollisionGroups` format (`Name^id^mask\...`).
fn encode_collision_groups_str(cg: &CollisionGroups) -> String {
    let names = cg.names();
    let mut parts = Vec::with_capacity(names.len());
    for (i, name) in names.iter().enumerate() {
        let mut mask: i32 = -1;
        for (j, other) in names.iter().enumerate() {
            if j < 32 && !cg.collidable(name, other) {
                mask &= !(1i32.wrapping_shl(j as u32));
            }
        }
        parts.push(format!("{name}^{i}^{mask}"));
    }
    parts.join("\\")
}

impl Editor {
    fn sync_collision_groups_from_doc(&mut self) {
        if self.tag_ed.groups_synced_doc == self.game.doc.id {
            return;
        }
        self.tag_ed.groups_synced_doc = self.game.doc.id;
        if let Some(ws) = self.game.doc.workspace() {
            if let Some(Variant::String(raw)) = self.game.doc.prop(ws, "CollisionGroups") {
                if !raw.is_empty() {
                    self.tag_ed.groups = parse_collision_groups_str(&raw);
                    return;
                }
            }
        }
        self.tag_ed.groups = CollisionGroups::default();
    }

    fn save_collision_groups_to_doc(&mut self) {
        if self.game.play.is_some() {
            return;
        }
        let encoded = encode_collision_groups_str(&self.tag_ed.groups);
        if let Some(ws) = self.game.doc.workspace() {
            self.game.doc.set_props(
                "Edit Collision Groups",
                vec![(ws, ustr("CollisionGroups"), Variant::String(encoded))],
                None,
            );
        }
    }

    /// Native CollectionService Tag Editor panel (`Tab::TagEditor`).
    pub fn tag_editor_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.tag_ed.new_tag)
                    .hint_text("New tag name…")
                    .desired_width((ui.available_width() - 95.0).max(90.0)),
            );
            if ui.button("+ Add Tag").clicked() {
                let t = self.tag_ed.new_tag.trim().to_string();
                if !t.is_empty() {
                    self.tag_ed.custom_tags.insert(t.clone());
                    // Also tag current selection immediately for fast workflow.
                    if !self.game.doc.selection.is_empty() && self.game.play.is_none() {
                        self.apply_tag_to_selection(&t, true);
                    }
                    self.tag_ed.new_tag.clear();
                }
            }
        });
        ui.add(
            egui::TextEdit::singleline(&mut self.tag_ed.filter)
                .hint_text("Filter tags…")
                .desired_width(f32::INFINITY),
        );
        ui.separator();

        // Collect all tags across the place + custom tags.
        let mut counts: std::collections::BTreeMap<String, Vec<Ref>> = std::collections::BTreeMap::new();
        for t in &self.tag_ed.custom_tags {
            counts.entry(t.clone()).or_default();
        }
        for inst in self.game.doc.dom.descendants() {
            if let Some(Variant::Tags(tags)) = inst.properties.get(&ustr("Tags")) {
                for t in tags.iter() {
                    counts.entry(t.to_string()).or_default().push(inst.referent());
                }
            }
        }

        let sel = self.game.doc.selection.clone();
        let filter = self.tag_ed.filter.to_ascii_lowercase();
        let mut toggle_action: Option<(String, bool)> = None;
        let mut select_action: Option<Vec<Ref>> = None;
        let mut delete_tag: Option<String> = None;

        ui.label(
            RichText::new(format!("{} selected instance(s) · {} tag(s)", sel.len(), counts.len()))
                .small()
                .color(theme::TEXT_DIM),
        );

        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for (tag, refs) in &counts {
                if !filter.is_empty() && !tag.to_ascii_lowercase().contains(&filter) {
                    continue;
                }
                let all_have = !sel.is_empty() && sel.iter().all(|r| refs.contains(r));
                let mut checked = all_have;
                ui.horizontal(|ui| {
                    if ui.add_enabled(!sel.is_empty() && self.game.play.is_none(), egui::Checkbox::new(&mut checked, "")).changed() {
                        toggle_action = Some((tag.clone(), checked));
                    }
                    ui.label(RichText::new(tag).strong());
                    ui.label(RichText::new(format!("({})", refs.len())).small().color(theme::TEXT_DIM));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("×").on_hover_text("Remove tag from all instances").clicked() && self.game.play.is_none() {
                            delete_tag = Some(tag.clone());
                        }
                        if ui.add_enabled(!refs.is_empty(), egui::Button::new("Select").small()).clicked() {
                            select_action = Some(refs.clone());
                        }
                    });
                });
            }
        });

        if let Some((tag, add)) = toggle_action {
            self.apply_tag_to_selection(&tag, add);
        }
        if let Some(refs) = select_action {
            self.select(refs);
        }
        if let Some(tag) = delete_tag {
            self.tag_ed.custom_tags.remove(&tag);
            let mut edits = Vec::new();
            for inst in self.game.doc.dom.descendants() {
                if let Some(Variant::Tags(tags)) = inst.properties.get(&ustr("Tags")) {
                    if tags.iter().any(|t| t == tag) {
                        let mut nt = Tags::default();
                        for t in tags.iter() {
                            if t != tag {
                                nt.push(t);
                            }
                        }
                        edits.push((inst.referent(), ustr("Tags"), Variant::Tags(nt)));
                    }
                }
            }
            if !edits.is_empty() {
                self.game.doc.set_props(&format!("Delete tag {tag}"), edits, None);
            }
        }
    }

    fn apply_tag_to_selection(&mut self, tag: &str, add: bool) {
        let mut edits = Vec::new();
        for &r in &self.game.doc.selection {
            let mut nt = Tags::default();
            let mut had = false;
            if let Some(Variant::Tags(existing)) = self.game.doc.prop(r, "Tags") {
                for t in existing.iter() {
                    if t == tag {
                        had = true;
                        if add {
                            nt.push(t);
                        }
                    } else {
                        nt.push(t);
                    }
                }
            }
            if add && !had {
                nt.push(tag);
            }
            edits.push((r, ustr("Tags"), Variant::Tags(nt)));
        }
        if !edits.is_empty() {
            let label = if add { format!("Add tag {tag}") } else { format!("Remove tag {tag}") };
            self.game.doc.set_props(&label, edits, None);
        }
    }

    /// Native PhysicsService Collision Groups Matrix panel (`Tab::CollisionGroups`).
    pub fn collision_groups_ui(&mut self, ui: &mut egui::Ui) {
        self.sync_collision_groups_from_doc();

        let mut dirty = false;
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.tag_ed.new_group)
                    .hint_text("New collision group…")
                    .desired_width((ui.available_width() - 105.0).max(90.0)),
            );
            if ui.button("+ Add Group").clicked() {
                let name = self.tag_ed.new_group.trim().to_string();
                match self.tag_ed.groups.register(&name) {
                    Ok(()) => {
                        self.tag_ed.new_group.clear();
                        self.tag_ed.status = Some(format!("Added group '{name}'"));
                        dirty = true;
                    }
                    Err(e) => self.tag_ed.status = Some(e),
                }
            }
        });

        // Quick preset groups for games
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Quick add:").small().color(theme::TEXT_DIM));
            for preset in ["Players", "Enemies", "Projectiles", "Debris", "NoCollide"] {
                if !self.tag_ed.groups.is_registered(preset) && ui.small_button(format!("+{preset}")).clicked() {
                    let _ = self.tag_ed.groups.register(preset);
                    if preset == "Players" {
                        self.tag_ed.groups.set_collidable("Players", "Players", false);
                    } else if preset == "NoCollide" {
                        self.tag_ed.groups.set_collidable("NoCollide", "Default", false);
                        self.tag_ed.groups.set_collidable("NoCollide", "NoCollide", false);
                    }
                    dirty = true;
                }
            }
        });

        if let Some(msg) = &self.tag_ed.status {
            ui.label(RichText::new(msg).small().color(theme::TEXT_DIM));
        }
        ui.separator();

        let names: Vec<String> = self.tag_ed.groups.names().to_vec();
        let selected_parts: Vec<Ref> = self
            .game
            .doc
            .selection
            .iter()
            .copied()
            .filter(|r| self.game.doc.class_of(*r).is_some_and(|c| doc::is_a(c.as_str(), "BasePart")))
            .collect();
        let mut assign_group: Option<String> = None;
        let mut remove_group: Option<String> = None;

        egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
            egui::Grid::new("collision_matrix").striped(true).spacing([6.0, 4.0]).show(ui, |ui| {
                ui.label(RichText::new("Group").strong());
                ui.label(RichText::new("Assign").strong());
                for col in &names {
                    ui.label(RichText::new(col).small().strong());
                }
                ui.end_row();

                for (i, row) in names.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(row);
                        if row != collision_groups::DEFAULT && ui.small_button("×").clicked() {
                            remove_group = Some(row.clone());
                        }
                    });
                    if ui
                        .add_enabled(!selected_parts.is_empty() && self.game.play.is_none(), egui::Button::new("Set").small())
                        .on_hover_text("Assign selected BaseParts to this CollisionGroup")
                        .clicked()
                    {
                        assign_group = Some(row.clone());
                    }
                    for (j, col) in names.iter().enumerate() {
                        if j <= i {
                            let mut on = self.tag_ed.groups.collidable(row, col);
                            if ui.checkbox(&mut on, "").changed() {
                                self.tag_ed.groups.set_collidable(row, col, on);
                                dirty = true;
                            }
                        } else {
                            ui.label("");
                        }
                    }
                    ui.end_row();
                }
            });
        });

        if let Some(g) = remove_group {
            self.tag_ed.groups.unregister(&g);
            dirty = true;
        }
        if let Some(g) = assign_group {
            let edits: Vec<_> = selected_parts
                .into_iter()
                .map(|r| (r, ustr("CollisionGroup"), Variant::String(g.clone())))
                .collect();
            self.game.doc.set_props(&format!("Set CollisionGroup {g}"), edits, None);
            self.tag_ed.status = Some(format!("Assigned selected parts to '{g}'"));
        }
        if dirty {
            self.save_collision_groups_to_doc();
        }
    }
}
