//! GuiButton.Modal: while a Modal button is shown, the game's mouse lock
//! (MouseBehavior LockCenter / LockCurrentPosition: first person, shift
//! lock) lets go of the cursor, unless the right mouse button is down
//! (Studio's API docs). UserInputService.MouseBehavior keeps the game's
//! value; only the effective lock changes (viewport.rs update_game_mouse).
//!
//! Studio (Studio, 2026-11-09 build):
//! - Studio: the PlayerGui walks its children (16-byte shared pointers
//!   at..), keeps the layer collectors and asks each.
//! - Studio: a layer collector answers false when it isn't Enabled
//!   (); otherwise true if any Modal button registered with it
//!   (..) is shown: with FFlagOnlyVisibleFramesAllowModal
//!   (Studio, on by default) by Studio(button, 0, 0, 1, 1), else
//!   by the button's own visibility.
//! - Studio: the button and its ancestors are visible,
//!   its layer collector exists and is Enabled, its absolute rect has an
//!   area, every GuiObject ancestor up to the collector with
//!   ClipsDescendants (vtable) clips it and leaves some area, the
//!   collector's own clip () too, and what's left lies on the screen.
//!
//! Here the GUI walk's layout (gui.rs `layout`, AbsolutePosition space) gives
//! the rects; an object the walk didn't lay out isn't drawn, so isn't shown.

use std::collections::HashMap;

use rbx_dom_weak::types::{Ref, Variant};

use rustudio_core::doc::{self, Document};
use crate::gui::GuiBox;

fn flag(d: &Document, r: Ref, name: &str, default: bool) -> bool {
    match d.prop(r, name) {
        Some(Variant::Bool(b)) => b,
        _ => default,
    }
}

/// (min, max) corners of a laid-out object.
fn rect(layout: &HashMap<Ref, GuiBox>, r: Ref) -> Option<([f32; 2], [f32; 2])> {
    let b = layout.get(&r)?;
    Some((b.pos, [b.pos[0] + b.size[0], b.pos[1] + b.size[1]]))
}

fn clip(a: ([f32; 2], [f32; 2]), b: ([f32; 2], [f32; 2])) -> Option<([f32; 2], [f32; 2])> {
    let lo = [a.0[0].max(b.0[0]), a.0[1].max(b.0[1])];
    let hi = [a.1[0].min(b.1[0]), a.1[1].min(b.1[1])];
    (hi[0] - lo[0] > 0.0 && hi[1] - lo[1] > 0.0).then_some((lo, hi))
}

/// One Modal button under the layer collector `lc`, as Studio judges it.
fn shown(d: &Document, layout: &HashMap<Ref, GuiBox>, lc: Ref, button: Ref) -> bool {
    if !flag(d, button, "Visible", true) {
        return false;
    }
    let Some(mut area) = rect(layout, button).and_then(|r| clip(r, r)) else { return false };
    let mut p = d.dom.get_by_ref(button).map(|i| i.parent()).unwrap_or_else(Ref::none);
    while p != lc {
        let Some(inst) = d.dom.get_by_ref(p) else { return false };
        if doc::is_a(&inst.class, "GuiObject") {
            if !flag(d, p, "Visible", true) {
                return false;
            }
            if flag(d, p, "ClipsDescendants", false) {
                match rect(layout, p).and_then(|r| clip(area, r)) {
                    Some(a) => area = a,
                    None => return false,
                }
            }
        }
        p = inst.parent();
    }
    // The collector's own canvas is the screen it draws on.
    rect(layout, lc).and_then(|r| clip(area, r)).is_some()
}

/// The LocalPlayer's PlayerGui and CoreGui (the containers a Modal button counts in).
fn containers(d: &Document) -> [Option<Ref>; 2] {
    let root = d.dom.root_ref();
    let pg = d.find_child(root, "Players").and_then(|p| match d.prop(p, "LocalPlayer") {
        Some(Variant::Ref(lp)) if d.exists(lp) => d.find_child(lp, "PlayerGui"),
        _ => None,
    });
    [pg, d.find_child(root, "CoreGui")]
}

fn is_modal_button(d: &Document, r: Ref) -> bool {
    d.dom.get_by_ref(r).is_some_and(|i| {
        doc::is_a(&i.class, "GuiButton")
            && matches!(i.properties.get(&rbx_dom_weak::ustr("Modal")), Some(Variant::Bool(true)))
    })
}

/// The Modal buttons of a document, kept current from its change journal
/// (`Reader::Modal`): a frame asks `shown` while the game holds the mouse
/// (every frame in first person / shift lock), and walking the whole
/// PlayerGui each time cost 6.6 ms a frame on a desktop in a place with a
/// large GUI (LBG, 2026-11-25: the biggest item on the main thread), more
/// on a phone. Now the walk happens once per document (and whenever the
/// journal answers `All`: the reader lagged, `mark_all`); afterwards only
/// `Modal` changes and new subtrees are looked at.
#[derive(Default)]
pub struct ModalIndex {
    doc: u64,
    buttons: Vec<Ref>,
}

impl ModalIndex {
    fn add_subtree(&mut self, d: &Document, r: Ref) {
        if !d.exists(r) {
            return;
        }
        if is_modal_button(d, r) && !self.buttons.contains(&r) {
            self.buttons.push(r);
        }
        for x in d.dom.descendants_of(r) {
            let b = x.referent();
            if b != r && is_modal_button(d, b) && !self.buttons.contains(&b) {
                self.buttons.push(b);
            }
        }
    }

    fn refresh(&mut self, d: &mut Document) {
        let mut full = d.id != self.doc;
        match d.marks_for(doc::Reader::Modal) {
            doc::Marks::All => full = true,
            doc::Marks::Some(marks) => {
                if !full {
                    for m in &marks {
                        match m {
                            doc::Mark::Prop(r, name) => {
                                if name.as_str() == "Modal" {
                                    let on = is_modal_button(d, *r);
                                    match self.buttons.iter().position(|b| b == r) {
                                        Some(i) if !on => {
                                            self.buttons.swap_remove(i);
                                        }
                                        None if on => self.buttons.push(*r),
                                        _ => {}
                                    }
                                }
                            }
                            // (a created, moved or removed subtree: a removed
                            // button drops out at the next `shown`)
                            doc::Mark::Tree(r) => self.add_subtree(d, *r),
                        }
                    }
                }
            }
        }
        if full {
            self.doc = d.id;
            self.buttons.clear();
            for c in containers(d).into_iter().flatten() {
                self.add_subtree(d, c);
            }
        }
        self.buttons.retain(|&b| is_modal_button(d, b));
    }

    /// `modal_shown`, from the index.
    pub fn shown(&mut self, d: &mut Document, layout: &HashMap<Ref, GuiBox>) -> bool {
        self.refresh(d);
        if self.buttons.is_empty() {
            return false;
        }
        let containers = containers(d);
        self.buttons.iter().any(|&button| {
            // Its layer collector: the first one up the tree, directly
            // under the PlayerGui or CoreGui, and Enabled.
            let mut p = d.dom.get_by_ref(button).map(|i| i.parent()).unwrap_or_else(Ref::none);
            let mut lc = None;
            while let Some(inst) = d.dom.get_by_ref(p) {
                if doc::is_a(&inst.class, "LayerCollector") {
                    lc = Some((p, inst.parent()));
                    break;
                }
                p = inst.parent();
            }
            match lc {
                Some((lc, under)) => {
                    containers.contains(&Some(under)) && flag(d, lc, "Enabled", true) && shown(d, layout, lc, button)
                }
                None => false,
            }
        })
    }
}

/// Whether the LocalPlayer's PlayerGui or CoreGui shows a Modal GuiButton now
/// (a full walk; frames use `ModalIndex`).
pub fn modal_shown(d: &Document, layout: &HashMap<Ref, GuiBox>) -> bool {
    let root = d.dom.root_ref();
    let pg = d.find_child(root, "Players").and_then(|p| match d.prop(p, "LocalPlayer") {
        Some(Variant::Ref(lp)) if d.exists(lp) => d.find_child(lp, "PlayerGui"),
        _ => None,
    });
    let cg = d.find_child(root, "CoreGui");
    for container in [pg, cg].into_iter().flatten() {
        let Some(ci) = d.dom.get_by_ref(container) else { continue };
        for &lc in ci.children() {
            let Some(li) = d.dom.get_by_ref(lc) else { continue };
            if !doc::is_a(&li.class, "LayerCollector") || !flag(d, lc, "Enabled", true) {
                continue;
            }
            for x in d.dom.descendants_of(lc) {
                if doc::is_a(&x.class, "GuiButton")
                    && matches!(x.properties.get(&rbx_dom_weak::ustr("Modal")), Some(Variant::Bool(true)))
                    && shown(d, layout, lc, x.referent())
                {
                    return true;
                }
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use rbx_dom_weak::InstanceBuilder;

    fn gb(pos: [f32; 2], size: [f32; 2]) -> GuiBox {
        GuiBox { pos, size, ..Default::default() }
    }

    /// Studio's rules: Enabled collector, the Visible chain, an area left
    /// after ClipsDescendants, on the collector's screen, Modal set.
    #[test]
    fn modal_button_shown_as_studio_judges_it() {
        let mut d = Document::new_place();
        let root = d.dom.root_ref();
        let players = d.find_child(root, "Players").unwrap_or_else(|| d.insert(root, InstanceBuilder::new("Players")));
        let lp = d.insert(players, InstanceBuilder::new("Player").with_name("P"));
        d.dom.get_by_ref_mut(players).unwrap().properties.insert(rbx_dom_weak::ustr("LocalPlayer"), Variant::Ref(lp));
        let pg = d.insert(lp, InstanceBuilder::new("PlayerGui").with_name("PlayerGui"));
        let sg = d.insert(pg, InstanceBuilder::new("ScreenGui").with_name("G"));
        let frame = d.insert(sg, InstanceBuilder::new("Frame").with_name("F"));
        let b = d.insert(frame, InstanceBuilder::new("TextButton").with_name("B").with_property("Modal", true));
        let mut layout = HashMap::new();
        layout.insert(sg, gb([0.0, 0.0], [800.0, 600.0]));
        layout.insert(frame, gb([100.0, 100.0], [200.0, 200.0]));
        layout.insert(b, gb([120.0, 120.0], [50.0, 20.0]));
        let set = |d: &mut Document, r: Ref, k: &str, v: Variant| {
            d.dom.get_by_ref_mut(r).unwrap().properties.insert(rbx_dom_weak::ustr(k), v);
        };
        assert!(modal_shown(&d, &layout), "a visible Modal button");
        set(&mut d, b, "Modal", Variant::Bool(false));
        assert!(!modal_shown(&d, &layout), "Modal off");
        set(&mut d, b, "Modal", Variant::Bool(true));
        set(&mut d, frame, "Visible", Variant::Bool(false));
        assert!(!modal_shown(&d, &layout), "an invisible ancestor");
        set(&mut d, frame, "Visible", Variant::Bool(true));
        set(&mut d, sg, "Enabled", Variant::Bool(false));
        assert!(!modal_shown(&d, &layout), "a disabled ScreenGui");
        set(&mut d, sg, "Enabled", Variant::Bool(true));
        // Clipped away by a ClipsDescendants frame it lies outside of.
        layout.insert(b, gb([400.0, 120.0], [50.0, 20.0]));
        assert!(modal_shown(&d, &layout), "outside a frame that doesn't clip");
        set(&mut d, frame, "ClipsDescendants", Variant::Bool(true));
        assert!(!modal_shown(&d, &layout), "clipped by its frame");
        set(&mut d, frame, "ClipsDescendants", Variant::Bool(false));
        // Off the screen, or with no size.
        layout.insert(b, gb([900.0, 120.0], [50.0, 20.0]));
        assert!(!modal_shown(&d, &layout), "off screen");
        layout.insert(b, gb([120.0, 120.0], [0.0, 20.0]));
        assert!(!modal_shown(&d, &layout), "no width");
        // Not laid out: not drawn.
        layout.remove(&b);
        assert!(!modal_shown(&d, &layout), "not laid out");

        // CoreGui Modal button (e.g. SettingsHub's SettingsShield).
        let cg = d.find_child(root, "CoreGui").unwrap_or_else(|| d.insert(root, InstanceBuilder::new("CoreGui")));
        let csg = d.insert(cg, InstanceBuilder::new("ScreenGui").with_name("RobloxGui"));
        let cb = d.insert(csg, InstanceBuilder::new("TextButton").with_name("SettingsShield").with_property("Modal", true));
        layout.insert(csg, gb([0.0, 0.0], [800.0, 600.0]));
        layout.insert(cb, gb([0.0, 0.0], [800.0, 600.0]));
        assert!(modal_shown(&d, &layout), "CoreGui Modal button");
        set(&mut d, cb, "Visible", Variant::Bool(false));
        assert!(!modal_shown(&d, &layout), "CoreGui Modal button hidden");
    }

    /// The index follows the journal: it agrees with the full walk after
    /// inserts, Modal / Visible changes, a reparent out of the PlayerGui,
    /// a removal, a clone brought in, and a new document.
    #[test]
    fn modal_index_follows_the_journal() {
        let mut d = Document::new_place();
        let root = d.dom.root_ref();
        let players = d.find_child(root, "Players").unwrap_or_else(|| d.insert(root, InstanceBuilder::new("Players")));
        let lp = d.insert(players, InstanceBuilder::new("Player").with_name("P"));
        d.set_prop_live(players, rbx_dom_weak::ustr("LocalPlayer"), Variant::Ref(lp));
        let pg = d.insert(lp, InstanceBuilder::new("PlayerGui").with_name("PlayerGui"));
        let sg = d.insert(pg, InstanceBuilder::new("ScreenGui").with_name("G"));
        let mut layout = HashMap::new();
        layout.insert(sg, gb([0.0, 0.0], [800.0, 600.0]));
        let mut ix = ModalIndex::default();
        assert!(!ix.shown(&mut d, &layout), "no button yet");
        // A button inserted later (a Tree mark).
        let b = d.insert(sg, InstanceBuilder::new("TextButton").with_name("B").with_property("Modal", true));
        layout.insert(b, gb([10.0, 10.0], [50.0, 20.0]));
        assert!(ix.shown(&mut d, &layout), "inserted Modal button");
        assert_eq!(ix.shown(&mut d, &layout), modal_shown(&d, &layout));
        d.set_prop_live(b, rbx_dom_weak::ustr("Modal"), Variant::Bool(false));
        assert!(!ix.shown(&mut d, &layout), "Modal off");
        d.set_prop_live(b, rbx_dom_weak::ustr("Modal"), Variant::Bool(true));
        assert!(ix.shown(&mut d, &layout), "Modal on again");
        d.set_prop_live(b, rbx_dom_weak::ustr("Visible"), Variant::Bool(false));
        assert!(!ix.shown(&mut d, &layout), "hidden");
        d.set_prop_live(b, rbx_dom_weak::ustr("Visible"), Variant::Bool(true));
        // Moved out of the PlayerGui (ReplicatedStorage): not shown; back: shown.
        let rs = d.find_child(root, "ReplicatedStorage").unwrap_or_else(|| d.insert(root, InstanceBuilder::new("ReplicatedStorage")));
        d.reparent(&[b], rs);
        assert!(!ix.shown(&mut d, &layout), "outside the PlayerGui");
        d.reparent(&[b], sg);
        assert!(ix.shown(&mut d, &layout), "back in");
        // A whole ScreenGui with a Modal button brought in, then removed.
        let sg2 = d.insert(rs, InstanceBuilder::new("ScreenGui").with_name("Menu"));
        let b2 = d.insert(sg2, InstanceBuilder::new("ImageButton").with_name("Shield").with_property("Modal", true));
        d.remove(&[b]);
        assert!(!ix.shown(&mut d, &layout), "removed");
        d.reparent(&[sg2], pg);
        layout.insert(sg2, gb([0.0, 0.0], [800.0, 600.0]));
        layout.insert(b2, gb([0.0, 0.0], [800.0, 600.0]));
        assert!(ix.shown(&mut d, &layout), "a ScreenGui moved in with its Modal button");
        assert_eq!(ix.shown(&mut d, &layout), modal_shown(&d, &layout));
        d.set_prop_live(sg2, rbx_dom_weak::ustr("Enabled"), Variant::Bool(false));
        assert!(!ix.shown(&mut d, &layout), "its ScreenGui disabled");
        // Another document: the index starts over.
        let mut d2 = Document::new_place();
        assert!(!ix.shown(&mut d2, &HashMap::new()), "a new document");
        d.set_prop_live(sg2, rbx_dom_weak::ustr("Enabled"), Variant::Bool(true));
        assert!(ix.shown(&mut d, &layout), "and back to the first");
    }
}
