//! Extended MCP tools: Roblox Creator Store / Toolbox / Inventory / Asset
//! download & upload / Cloud publish, GUI inspection with live screen rects,
//! Reflection API inspection, atomic `build_tree` & `batch_edit`, Terrain
//! voxel editing, KeyframeSequence animation authoring, Rig spawning,
//! Lighting presets, and instant playable game templates.

use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use glam::Vec3;
use rbx_dom_weak::types::{BinaryString, CFrame, Enum, Ref, Variant, Vector3};
use rbx_dom_weak::{ustr, InstanceBuilder};
use rustudio_account::cloud::{CloudApi, Endpoints};
use rustudio_account::VersionType;
use rustudio_core::doc;
use rustudio_core::terrain::{self, Voxels, VOXEL};
use rustudio_engine::rig::{BodyColors, RigType};
use serde_json::{json, Value as J};

use super::server::ToolResult;
use super::{templates, tree, Op, Step};
use crate::app::Editor;
use crate::toolbox::api::{self as tb_api, AnonHttp, Filters, Http, ListReq, SessionHttp, Sort, Source};
use crate::toolbox::categories::Kind as TbKind;
use crate::toolbox::insert as tb_insert;
use crate::toolbox::model::Asset as TbAsset;

type R<T> = Result<T, String>;

/// Result sent back from a background MCP worker thread (Toolbox / Asset / Cloud calls).
pub(crate) enum AsyncOutcome {
    Ready(ToolResult),
    InsertAsset {
        asset_id: u64,
        name: String,
        kind: TbKind,
        bytes: Option<Vec<u8>>,
        parent: Option<Ref>,
        position: Option<[f32; 3]>,
    },
}

fn s<'a>(a: &'a J, k: &str) -> Option<&'a str> {
    a.get(k).and_then(|v| v.as_str())
}

fn req<'a>(a: &'a J, k: &str) -> R<&'a str> {
    s(a, k).ok_or_else(|| format!("missing \"{k}\" (a string)"))
}

fn num(a: &J, k: &str) -> Option<f64> {
    a.get(k).and_then(|v| v.as_f64())
}

fn flag(a: &J, k: &str, d: bool) -> bool {
    a.get(k).and_then(|v| v.as_bool()).unwrap_or(d)
}

fn vec3_arg(a: &J, k: &str) -> Option<[f32; 3]> {
    let arr = a.get(k)?.as_array()?;
    if arr.len() < 3 {
        return None;
    }
    Some([arr[0].as_f64()? as f32, arr[1].as_f64()? as f32, arr[2].as_f64()? as f32])
}

fn parse_tb_kind(raw: Option<&str>) -> R<TbKind> {
    match raw.unwrap_or("model").trim().to_ascii_lowercase().as_str() {
        "model" | "models" => Ok(TbKind::Model),
        "audio" | "sound" | "sounds" | "music" => Ok(TbKind::Audio),
        "decal" | "decals" | "image" | "images" => Ok(TbKind::Decal),
        "mesh" | "meshpart" | "meshes" => Ok(TbKind::MeshPart),
        "plugin" | "plugins" => Ok(TbKind::Plugin),
        "font" | "fonts" => Ok(TbKind::Font),
        "video" | "videos" => Ok(TbKind::Video),
        "animation" | "animations" | "anim" => Ok(TbKind::Animation),
        "package" | "packages" => Ok(TbKind::Package),
        other => Err(format!(
            "Unknown asset kind {other:?}. Use one of: model, audio, decal, meshpart, plugin, font, video, animation, package"
        )),
    }
}

fn asset_json(a: &TbAsset) -> J {
    json!({
        "id": a.id,
        "rbxassetid": format!("rbxassetid://{}", a.id),
        "name": a.name,
        "description": a.description,
        "type_id": a.type_id,
        "creator": {
            "id": a.creator.id,
            "name": a.creator.name,
            "verified": a.creator.verified,
        },
        "price": a.price.label(false),
        "free": a.price.is_free(),
        "votes": {
            "up": a.votes.up,
            "down": a.votes.down,
            "percent": a.votes.percent,
        },
        "has_scripts": a.has_scripts,
        "script_count": a.script_count,
        "triangles": a.triangles,
        "vertices": a.vertices,
        "category_path": a.category_path,
        "audio": a.audio.as_ref().map(|au| json!({
            "duration": au.duration,
            "type": au.audio_type,
            "artist": au.artist,
            "title": au.title,
            "genre": au.genre,
        })),
    })
}

fn detect_upload_type(path: &std::path::Path, override_type: Option<&str>) -> R<(&'static str, &'static str)> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();
    let (default_type, mime) = match ext.as_str() {
        "png" => ("Image", "image/png"),
        "jpg" | "jpeg" => ("Image", "image/jpeg"),
        "bmp" => ("Image", "image/bmp"),
        "tga" => ("Image", "image/tga"),
        "ogg" => ("Audio", "audio/ogg"),
        "mp3" => ("Audio", "audio/mpeg"),
        "wav" => ("Audio", "audio/wav"),
        "flac" => ("Audio", "audio/flac"),
        "rbxm" => ("Model", "model/x-rbxm"),
        "rbxmx" => ("Model", "model/x-rbxmx"),
        "fbx" => ("Model", "model/fbx"),
        "obj" => ("Model", "model/obj"),
        "gltf" => ("Model", "model/gltf+json"),
        "glb" => ("Model", "model/gltf-binary"),
        _ => return Err(format!("Unsupported file extension .{ext}; supported: png, jpg, bmp, tga, ogg, mp3, wav, flac, rbxm, rbxmx, fbx, obj, gltf, glb")),
    };
    let asset_type = match override_type.map(|t| t.trim().to_ascii_lowercase()).as_deref() {
        None | Some("") => default_type,
        Some("image") => "Image",
        Some("decal") => "Decal",
        Some("audio") => "Audio",
        Some("model") => "Model",
        Some("animation") => "Animation",
        Some("mesh") | Some("meshpart") => "Mesh",
        Some(other) => {
            return Err(format!(
                "Unsupported asset_type {other:?}; use one of: Image, Decal, Audio, Model, Animation, Mesh"
            ))
        }
    };
    Ok((asset_type, mime))
}

fn easing_style_id(name: &str) -> u32 {
    match name.trim().to_ascii_lowercase().as_str() {
        "linear" => 0,
        "constant" => 1,
        "elastic" => 2,
        "cubic" | "cubicv2" => 3,
        "bounce" => 4,
        _ => 0,
    }
}

fn easing_dir_id(name: &str) -> u32 {
    match name.trim().to_ascii_lowercase().as_str() {
        "in" => 0,
        "out" => 1,
        "inout" => 2,
        _ => 2,
    }
}

impl Editor {
    /// Clones a downloader session if signed in to Roblox.
    fn mcp_session(&self) -> Option<rustudio_account::session::Session> {
        let shared = self.game.account.shared_client()?;
        let guard = shared.lock().ok()?;
        guard.as_ref().map(|c| c.session.downloader())
    }

    fn mcp_require_session(&self) -> R<rustudio_account::session::Session> {
        self.mcp_session()
            .ok_or_else(|| "Not signed in to Roblox. Sign in via the Account menu in rustudio first.".into())
    }

    // --------------------------------------------------------- GUI inspection

    /// Lists rendered ScreenGuis / BillboardGuis / SurfaceGuis and their GuiObjects
    /// with live viewport positions, sizes, and click centers.
    pub(super) fn mcp_inspect_gui(&self, a: &J) -> R<ToolResult> {
        let root = match s(a, "path") {
            Some(p) => self.mcp_resolve(p)?,
            None => self.game.doc.dom.root_ref(),
        };
        let visible_only = flag(a, "visible_only", false);
        let limit = num(a, "limit").map_or(300, |l| l.clamp(1.0, 2000.0) as usize);
        let boxes = self.game.gui.laid_out_boxes();
        let doc = &self.game.doc;
        let vp = self.game.viewport_rect;

        let mut items = Vec::new();
        let mut total = 0usize;
        let mut stack = vec![root];
        while let Some(r) = stack.pop() {
            let Some(inst) = doc.dom.get_by_ref(r) else { continue };
            for &c in inst.children().iter().rev() {
                stack.push(c);
            }
            let cls = inst.class.as_str();
            let is_layer = doc::is_a(cls, "LayerCollector");
            let is_obj = doc::is_a(cls, "GuiObject");
            if !is_layer && !is_obj {
                continue;
            }
            let visible = if is_layer {
                !matches!(doc.prop(r, "Enabled"), Some(Variant::Bool(false)))
            } else {
                !matches!(doc.prop(r, "Visible"), Some(Variant::Bool(false)))
            };
            if visible_only && !visible {
                continue;
            }
            total += 1;
            if items.len() >= limit {
                continue;
            }
            let mut row = json!({
                "path": tree::path(doc, r),
                "id": tree::id(r),
                "class": cls,
                "name": inst.name,
                "visible": visible,
            });
            if let Some(Variant::String(t)) = doc.prop(r, "Text") {
                row["text"] = json!(t);
            }
            if let Some(img) = doc::content_string(doc.prop(r, "Image")) {
                if !img.is_empty() {
                    row["image"] = json!(img);
                }
            }
            if let Some(Variant::Int32(z)) = doc.prop(r, "ZIndex") {
                row["z_index"] = json!(z);
            }
            if let Some(Variant::Bool(act)) = doc.prop(r, "Active") {
                row["active"] = json!(act);
            }
            if let Some(b) = boxes.get(&r) {
                let cx = b.pos[0] + b.size[0] * 0.5;
                let cy = b.pos[1] + b.size[1] * 0.5;
                row["abs_pos"] = json!([b.pos[0], b.pos[1]]);
                row["abs_size"] = json!([b.size[0], b.size[1]]);
                row["click_center"] = json!([cx, cy]);
                row["rotation"] = json!(b.rot);
                if let Some(tb) = b.text {
                    row["text_bounds"] = json!([tb[0], tb[1]]);
                }
            }
            items.push(row);
        }

        Ok(ToolResult::json(&json!({
            "mode": self.mcp_mode(),
            "datamodel": self.mcp_datamodel(),
            "viewport_size": [vp.width(), vp.height()],
            "laid_out_count": boxes.len(),
            "total": total,
            "items": items,
        })))
    }

    // --------------------------------------------------- Reflection API tools

    pub(super) fn mcp_describe_class(&self, a: &J) -> R<ToolResult> {
        let Some(api) = rustudio_core::api::get() else {
            return Err("api-dump.json is not loaded".into());
        };
        if let Some(name) = s(a, "class") {
            let Some(cls) = api.classes.get(name) else {
                return Err(format!("Unknown Roblox class {name:?}"));
            };
            let include_inherited = flag(a, "inherited", false);
            let include_hidden = flag(a, "include_hidden", false);
            let mut props = Vec::new();
            let mut funcs = Vec::new();
            let mut events = Vec::new();
            let mut callbacks = Vec::new();
            let mut cur_name: Option<&str> = Some(name);
            let mut chain = Vec::new();
            let mut seen = std::collections::HashSet::new();

            while let Some(cn) = cur_name {
                let Some(c) = api.classes.get(cn) else { break };
                chain.push(cn.to_string());
                for mname in &c.order {
                    if !seen.insert(mname.clone()) {
                        continue;
                    }
                    let Some(m) = c.members.get(mname) else { continue };
                    if m.hidden && !include_hidden {
                        continue;
                    }
                    let entry = json!({
                        "name": mname,
                        "owner": cn,
                        "category": m.category,
                        "read_only": m.read_only,
                        "deprecated": m.deprecated,
                        "security": m.security,
                    });
                    match m.kind {
                        rustudio_core::api::Kind::Property => props.push(entry),
                        rustudio_core::api::Kind::Function => funcs.push(entry),
                        rustudio_core::api::Kind::Event => events.push(entry),
                        rustudio_core::api::Kind::Callback => callbacks.push(entry),
                    }
                }
                if !include_inherited {
                    break;
                }
                cur_name = c.superclass.as_deref();
            }

            return Ok(ToolResult::json(&json!({
                "class": name,
                "superclass": cls.superclass,
                "chain": chain,
                "creatable": cls.creatable,
                "service": cls.service,
                "properties": props,
                "functions": funcs,
                "events": events,
                "callbacks": callbacks,
            })));
        }

        // Otherwise list/filter classes.
        let query = s(a, "query").map(|q| q.to_ascii_lowercase());
        let base = s(a, "base");
        let creatable_only = flag(a, "creatable_only", false);
        let limit = num(a, "limit").map_or(200, |l| l.clamp(1.0, 2000.0) as usize);
        let mut matched = Vec::new();
        for cname in &api.class_order {
            let Some(c) = api.classes.get(cname) else { continue };
            if creatable_only && !c.creatable {
                continue;
            }
            if let Some(b) = base {
                if !doc::is_a(cname, b) {
                    continue;
                }
            }
            if let Some(q) = &query {
                if !cname.to_ascii_lowercase().contains(q) {
                    continue;
                }
            }
            matched.push(json!({
                "class": cname,
                "superclass": c.superclass,
                "creatable": c.creatable,
                "service": c.service,
            }));
            if matched.len() >= limit {
                break;
            }
        }
        Ok(ToolResult::json(&json!({
            "count": matched.len(),
            "classes": matched,
        })))
    }

    pub(super) fn mcp_list_enums(&self, a: &J) -> R<ToolResult> {
        let Some(api) = rustudio_core::api::get() else {
            return Err("api-dump.json is not loaded".into());
        };
        if let Some(name) = s(a, "enum_name") {
            let Some(items) = api.enums.get(name) else {
                return Err(format!("Unknown Enum {name:?}"));
            };
            let list: Vec<J> = items.iter().map(|(n, v)| json!({ "name": n, "value": v })).collect();
            return Ok(ToolResult::json(&json!({ "enum": name, "items": list })));
        }
        let query = s(a, "query").map(|q| q.to_ascii_lowercase());
        let limit = num(a, "limit").map_or(200, |l| l.clamp(1.0, 1000.0) as usize);
        let mut names: Vec<&String> = api
            .enums
            .keys()
            .filter(|k| query.as_ref().is_none_or(|q| k.to_ascii_lowercase().contains(q)))
            .collect();
        names.sort();
        names.truncate(limit);
        Ok(ToolResult::json(&json!({ "count": names.len(), "enums": names })))
    }

    // ----------------------------------------- Toolbox / Inventory / Assets

    pub(super) fn mcp_search_toolbox(&mut self, a: &J) -> R<Step> {
        let session = self.mcp_require_session()?;
        let kind = parse_tb_kind(s(a, "kind"))?;
        let query = s(a, "query").unwrap_or("").to_string();
        let cursor = s(a, "cursor").map(String::from);
        let section = s(a, "section").map(String::from);
        let mut filters = Filters::default();
        filters.sort = match s(a, "sort").unwrap_or("relevance").to_ascii_lowercase().as_str() {
            "ratings" | "rating" => Sort::Ratings,
            "newest" | "recent" => Sort::Newest,
            _ => Sort::Relevance,
        };
        filters.free_only = flag(a, "free_only", false);
        filters.include_unverified = flag(a, "include_unverified", false);
        if let Some(creator) = s(a, "creator") {
            filters.creator_name = creator.to_string();
        }

        let source = match section {
            Some(sec) if !sec.is_empty() => Source::Home(sec),
            _ => Source::Store,
        };
        let req = ListReq {
            source,
            kind,
            query,
            category_path: s(a, "category_path").map(String::from),
            sub_type: None,
            filters,
        };

        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let http = SessionHttp(session);
            let res = match tb_api::fetch_page(&http, &req, cursor.as_deref(), &|| false) {
                Ok(page) => ToolResult::json(&json!({
                    "count": page.assets.len(),
                    "next_cursor": page.next,
                    "assets": page.assets.iter().map(asset_json).collect::<Vec<_>>(),
                })),
                Err(e) => ToolResult::error(format!("Toolbox search failed: {}", e.describe())),
            };
            let _ = tx.send(AsyncOutcome::Ready(res));
        });
        Ok(Step::Wait(Op::AsyncJob { rx, label: "Toolbox search".into() }, Duration::from_secs(45)))
    }

    pub(super) fn mcp_browse_inventory(&mut self, a: &J) -> R<Step> {
        let session = self.mcp_require_session()?;
        let kind = parse_tb_kind(s(a, "kind"))?;
        let group = num(a, "group_id").map(|g| g as u64);
        let source = match s(a, "source").unwrap_or("inventory").to_ascii_lowercase().as_str() {
            "creations" | "my_creations" => Source::Creations { group },
            "recent" => Source::Recent,
            _ => Source::Inventory { group },
        };
        let query = s(a, "query").unwrap_or("").to_string();
        let cursor = s(a, "cursor").map(String::from);
        let req = ListReq {
            source,
            kind,
            query,
            category_path: None,
            sub_type: None,
            filters: Filters::default(),
        };

        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let http = SessionHttp(session);
            let res = match tb_api::fetch_page(&http, &req, cursor.as_deref(), &|| false) {
                Ok(page) => ToolResult::json(&json!({
                    "count": page.assets.len(),
                    "next_cursor": page.next,
                    "assets": page.assets.iter().map(asset_json).collect::<Vec<_>>(),
                })),
                Err(e) => ToolResult::error(format!("Inventory browse failed: {}", e.describe())),
            };
            let _ = tx.send(AsyncOutcome::Ready(res));
        });
        Ok(Step::Wait(Op::AsyncJob { rx, label: "Browse inventory".into() }, Duration::from_secs(45)))
    }

    pub(super) fn mcp_get_asset_details(&mut self, a: &J) -> R<Step> {
        let ids: Vec<u64> = if let Some(arr) = a.get("asset_ids").and_then(|v| v.as_array()) {
            arr.iter().filter_map(|v| v.as_u64()).collect()
        } else if let Some(id) = num(a, "asset_id") {
            vec![id as u64]
        } else {
            return Err("Provide \"asset_id\" (integer) or \"asset_ids\" (array of integers)".into());
        };
        if ids.is_empty() || ids.len() > 30 {
            return Err("Provide between 1 and 30 asset IDs".into());
        }
        let session = self.mcp_session();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let res = match session {
                Some(s) => tb_api::fetch_details(&SessionHttp(s), &ids, &|| false),
                None => tb_api::fetch_details(&AnonHttp, &ids, &|| false),
            };
            let tr = match res {
                Ok(list) => ToolResult::json(&json!({
                    "count": list.len(),
                    "assets": list.iter().map(asset_json).collect::<Vec<_>>(),
                })),
                Err(e) => ToolResult::error(format!("Asset details failed: {}", e.describe())),
            };
            let _ = tx.send(AsyncOutcome::Ready(tr));
        });
        Ok(Step::Wait(Op::AsyncJob { rx, label: "Get asset details".into() }, Duration::from_secs(30)))
    }

    pub(super) fn mcp_insert_asset(&mut self, a: &J) -> R<Step> {
        self.mcp_writable()?;
        let asset_id = num(a, "asset_id")
            .filter(|n| *n > 0.0)
            .map(|n| n as u64)
            .ok_or("missing positive \"asset_id\" (integer)")?;
        let parent = match s(a, "parent") {
            Some(p) => Some(self.mcp_resolve(p)?),
            None => None,
        };
        let position = vec3_arg(a, "position");
        let kind_override = s(a, "kind").map(|k| parse_tb_kind(Some(k))).transpose()?;
        let session = self.mcp_session();

        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let http_box: Box<dyn Http> = match session {
                Some(s) => Box::new(SessionHttp(s)),
                None => Box::new(AnonHttp),
            };
            let details = tb_api::fetch_details(http_box.as_ref(), &[asset_id], &|| false)
                .ok()
                .and_then(|v| v.into_iter().next());
            let name = details
                .as_ref()
                .filter(|d| !d.name.is_empty())
                .map(|d| d.name.clone())
                .unwrap_or_else(|| format!("Asset_{asset_id}"));
            let kind = kind_override.unwrap_or_else(|| {
                details
                    .as_ref()
                    .and_then(|d| TbKind::from_asset_type(d.type_id))
                    .unwrap_or(TbKind::Model)
            });

            match kind {
                TbKind::Audio | TbKind::Video | TbKind::Font => {
                    let _ = tx.send(AsyncOutcome::InsertAsset {
                        asset_id,
                        name,
                        kind,
                        bytes: None,
                        parent,
                        position,
                    });
                }
                _ => {
                    let dl = tb_insert::download(http_box.as_ref(), asset_id, &|| false)
                        .or_else(|_| rustudio_account::public_asset(asset_id).map_err(|e| tb_api::Failure::Other(e.0)));
                    match dl {
                        Ok(bytes) => {
                            let _ = tx.send(AsyncOutcome::InsertAsset {
                                asset_id,
                                name,
                                kind,
                                bytes: Some(bytes),
                                parent,
                                position,
                            });
                        }
                        Err(e) => {
                            let _ = tx.send(AsyncOutcome::Ready(ToolResult::error(format!(
                                "Failed to download asset {asset_id}: {}",
                                e.describe()
                            ))));
                        }
                    }
                }
            }
        });
        Ok(Step::Wait(Op::AsyncJob { rx, label: format!("Insert asset {asset_id}") }, Duration::from_secs(60)))
    }

    pub(super) fn finish_insert_asset(
        &mut self,
        asset_id: u64,
        name: String,
        kind: TbKind,
        bytes: Option<Vec<u8>>,
        parent: Option<Ref>,
        position: Option<[f32; 3]>,
    ) -> ToolResult {
        if let Err(e) = self.mcp_writable() {
            return ToolResult::error(e);
        }
        let ws = match self.game.doc.workspace() {
            Some(w) => w,
            None => return ToolResult::error("Workspace is missing"),
        };
        let parent = parent.filter(|r| self.game.doc.exists(*r)).unwrap_or(ws);
        let uri = format!("rbxassetid://{asset_id}");

        match kind {
            TbKind::Audio => {
                let target = if parent == ws {
                    self.game.doc.find_child(self.game.doc.dom.root_ref(), "SoundService").unwrap_or(ws)
                } else {
                    parent
                };
                let b = InstanceBuilder::new("Sound")
                    .with_name(name)
                    .with_property("SoundId", Variant::ContentId(uri.clone().into()));
                let r = self.game.doc.insert(target, b);
                return ToolResult::json(&json!({
                    "asset_id": asset_id,
                    "kind": "audio",
                    "rbxassetid": uri,
                    "inserted": [tree::path(&self.game.doc, r)],
                }));
            }
            TbKind::Video => {
                let b = InstanceBuilder::new("VideoFrame")
                    .with_name(name)
                    .with_property("Video", Variant::ContentId(uri.clone().into()));
                let r = self.game.doc.insert(parent, b);
                return ToolResult::json(&json!({
                    "asset_id": asset_id,
                    "kind": "video",
                    "rbxassetid": uri,
                    "inserted": [tree::path(&self.game.doc, r)],
                }));
            }
            _ => {}
        }

        let Some(bytes) = bytes else {
            return ToolResult::error(format!("No data downloaded for asset {asset_id}"));
        };

        // If the downloaded bytes are a valid Roblox model/place (.rbxm/.rbxmx/.rbxl/.rbxlx), paste it directly!
        if doc::Document::decode(&bytes).is_ok() {
            let refs = self.game.doc.paste(&bytes, parent, &format!("Insert asset {asset_id} (MCP)"));
            if let Some(pos) = position {
                if let Some((lo, hi)) = tb_insert::aabb(&self.game.doc.dom, &refs) {
                    let center = (lo + hi) * 0.5;
                    let delta = Vec3::new(pos[0], pos[1], pos[2]) - Vec3::new(center.x, lo.y, center.z);
                    tb_insert::translate(&mut self.game.doc.dom, &refs, delta);
                    self.game.doc.mark_all();
                }
            }
            return ToolResult::json(&json!({
                "asset_id": asset_id,
                "name": name,
                "rbxassetid": uri,
                "inserted": refs.iter().map(|r| tree::path(&self.game.doc, *r)).collect::<Vec<_>>(),
            }));
        }

        // Non-DOM raw assets (e.g., raw PNG/JPG image asset or raw mesh asset): create the matching Roblox instance!
        let b = match kind {
            TbKind::Decal => InstanceBuilder::new("Decal")
                .with_name(name)
                .with_property("Texture", Variant::ContentId(uri.clone().into())),
            TbKind::Animation => InstanceBuilder::new("Animation")
                .with_name(name)
                .with_property("AnimationId", Variant::ContentId(uri.clone().into())),
            TbKind::MeshPart => {
                let pos = position.unwrap_or([0.0, 4.0, 0.0]);
                InstanceBuilder::new("MeshPart")
                    .with_name(name)
                    .with_property("Anchored", true)
                    .with_property("MeshId", Variant::ContentId(uri.clone().into()))
                    .with_property("Size", Vector3::new(4.0, 4.0, 4.0))
                    .with_property("CFrame", templates::cframe_from_pos_euler_deg(pos, [0.0, 0.0, 0.0]))
            }
            _ => {
                return ToolResult::error(format!(
                    "Asset {asset_id} ({} bytes) is not a Roblox .rbxm/.rbxmx model and could not be auto-wrapped as {:?}",
                    bytes.len(),
                    kind
                ));
            }
        };
        let r = self.game.doc.insert(parent, b);
        ToolResult::json(&json!({
            "asset_id": asset_id,
            "rbxassetid": uri,
            "inserted": [tree::path(&self.game.doc, r)],
        }))
    }

    pub(super) fn mcp_download_asset(&mut self, a: &J) -> R<Step> {
        let asset_id = num(a, "asset_id")
            .filter(|n| *n > 0.0)
            .map(|n| n as u64)
            .ok_or("missing positive \"asset_id\" (integer)")?;
        let out_path = PathBuf::from(req(a, "path")?);
        if !out_path.is_absolute() {
            return Err("\"path\" must be an absolute file path".into());
        }
        let session = self.mcp_session();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let dl = match session {
                Some(s) => tb_insert::download(&SessionHttp(s), asset_id, &|| false)
                    .map_err(|e| e.describe())
                    .or_else(|_| rustudio_account::public_asset(asset_id).map_err(|e| e.0)),
                None => rustudio_account::public_asset(asset_id).map_err(|e| e.0),
            };
            let tr = match dl {
                Ok(bytes) => {
                    if let Some(p) = out_path.parent() {
                        let _ = std::fs::create_dir_all(p);
                    }
                    match std::fs::write(&out_path, &bytes) {
                        Ok(()) => ToolResult::json(&json!({
                            "asset_id": asset_id,
                            "path": out_path.display().to_string(),
                            "bytes": bytes.len(),
                        })),
                        Err(e) => ToolResult::error(format!("Write {}: {e}", out_path.display())),
                    }
                }
                Err(e) => ToolResult::error(format!("Download asset {asset_id} failed: {e}")),
            };
            let _ = tx.send(AsyncOutcome::Ready(tr));
        });
        Ok(Step::Wait(Op::AsyncJob { rx, label: format!("Download asset {asset_id}") }, Duration::from_secs(60)))
    }

    pub(super) fn mcp_upload_asset(&mut self, a: &J) -> R<Step> {
        let session = self.mcp_require_session()?;
        let file = PathBuf::from(req(a, "path")?);
        if !file.is_absolute() || !file.is_file() {
            return Err(format!("{} must be an existing absolute file path", file.display()));
        }
        let (asset_type, mime) = detect_upload_type(&file, s(a, "asset_type"))?;
        let display_name = s(a, "name")
            .map(String::from)
            .unwrap_or_else(|| file.file_stem().and_then(|x| x.to_str()).unwrap_or("Asset").to_string());
        let description = s(a, "description").unwrap_or("").to_string();
        let group_id = num(a, "group_id").map(|g| g as u64);
        let filename = file.file_name().and_then(|x| x.to_str()).unwrap_or("asset").to_string();
        let bytes = std::fs::read(&file).map_err(|e| format!("Read {}: {e}", file.display()))?;

        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let api = CloudApi { session, ep: Endpoints::default() };
            let tr = match api.upload_asset(asset_type, &display_name, &description, group_id, &filename, mime, &bytes) {
                Ok(id) => ToolResult::json(&json!({
                    "asset_id": id,
                    "rbxassetid": format!("rbxassetid://{id}"),
                    "name": display_name,
                    "asset_type": asset_type,
                    "bytes": bytes.len(),
                })),
                Err(e) => ToolResult::error(format!("Upload asset failed: {}", e.0)),
            };
            let _ = tx.send(AsyncOutcome::Ready(tr));
        });
        Ok(Step::Wait(Op::AsyncJob { rx, label: "Upload asset to Roblox".into() }, Duration::from_secs(120)))
    }

    pub(super) fn mcp_publish_place(&mut self, a: &J) -> R<Step> {
        self.mcp_writable()?;
        let session = self.mcp_require_session()?;
        let linked = self
            .live_team_create_ids()
            .or_else(|| self.game.doc.cloud.as_ref().map(|c| (c.universe_id, c.place_id)));
        let universe_id = num(a, "universe_id")
            .map(|n| n as u64)
            .or_else(|| linked.map(|(u, _)| u))
            .ok_or("Provide \"universe_id\" (or open a Roblox cloud place first)")?;
        let place_id = num(a, "place_id")
            .map(|n| n as u64)
            .or_else(|| linked.map(|(_, p)| p))
            .ok_or("Provide \"place_id\" (or open a Roblox cloud place first)")?;
        let publish = flag(a, "publish", true);
        let vt = if publish { VersionType::Published } else { VersionType::Saved };
        let bytes = self.game.doc.encode_all(false)?;

        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let api = CloudApi { session, ep: Endpoints::default() };
            let tr = match api.upload_place(universe_id, place_id, &bytes, false, vt) {
                Ok(version) => ToolResult::json(&json!({
                    "universe_id": universe_id,
                    "place_id": place_id,
                    "published": publish,
                    "version": version,
                    "bytes": bytes.len(),
                })),
                Err(e) => ToolResult::error(format!("Roblox place save/publish failed: {}", e.0)),
            };
            let _ = tx.send(AsyncOutcome::Ready(tr));
        });
        Ok(Step::Wait(Op::AsyncJob { rx, label: "Publish place to Roblox".into() }, Duration::from_secs(120)))
    }

    // ---------------------------------------- Fast game building power tools

    /// Recursively constructs an `InstanceBuilder` from a JSON tree spec.
    fn build_node(spec: &J) -> R<InstanceBuilder> {
        let class = req(spec, "class")?;
        if !doc::db().classes.contains_key(class) {
            return Err(format!("{class:?} is not a known Roblox class"));
        }
        let empty = serde_json::Map::new();
        let prop_map = spec.get("properties").and_then(|p| p.as_object()).unwrap_or(&empty);
        let (props, rename) = Self::mcp_resolve_props(class, prop_map)?;
        let name = s(spec, "name").map(String::from).or(rename).unwrap_or_else(|| class.to_string());
        let mut b = InstanceBuilder::new(class).with_name(name);
        for (k, v) in props {
            b.add_property(ustr(&k), v);
        }
        if let Some(src) = s(spec, "source") {
            b.add_property(ustr("Source"), Variant::String(src.to_string()));
        }
        if let Some(attrs) = spec.get("attributes").and_then(|v| v.as_object()) {
            let mut cur = rbx_dom_weak::types::Attributes::default();
            for (k, v) in attrs {
                if let Ok((_, Variant::Attributes(one))) =
                    rustudio_core::project_values::resolve(class, "Attributes", &json!({ "Attributes": { k.clone(): v } }))
                {
                    if let Some(x) = one.get(k.as_str()) {
                        cur.insert(k.clone(), x.clone());
                    }
                }
            }
            b.add_property(ustr("Attributes"), Variant::Attributes(cur));
        }
        if let Some(tags) = spec.get("tags").and_then(|t| t.as_array()) {
            let mut t = rbx_dom_weak::types::Tags::default();
            for x in tags {
                if let Some(ts) = x.as_str() {
                    t.push(ts);
                }
            }
            b.add_property(ustr("Tags"), Variant::Tags(t));
        }
        if let Some(kids) = spec.get("children").and_then(|c| c.as_array()) {
            for kid in kids {
                b.add_child(Self::build_node(kid)?);
            }
        }
        Ok(b)
    }

    pub(super) fn mcp_build_tree(&mut self, a: &J) -> R<ToolResult> {
        let parent = self.mcp_resolve(s(a, "parent").unwrap_or("Workspace"))?;
        let specs: Vec<&J> = if let Some(arr) = a.get("nodes").and_then(|v| v.as_array()) {
            arr.iter().collect()
        } else if let Some(node) = a.get("node") {
            vec![node]
        } else {
            return Err("Provide \"node\" (object) or \"nodes\" (array of node objects)".into());
        };
        if specs.is_empty() {
            return Err("No nodes specified".into());
        }
        let mut builders = Vec::with_capacity(specs.len());
        for sp in specs {
            builders.push(Self::build_node(sp)?);
        }
        let mut created = Vec::new();
        for b in builders {
            let r = self.game.doc.insert(parent, b);
            created.push(json!({
                "path": tree::path(&self.game.doc, r),
                "id": tree::id(r),
                "descendants": self.game.doc.subtree(r).len().saturating_sub(1),
            }));
        }
        Ok(ToolResult::json(&json!({ "created": created })))
    }

    pub(super) fn mcp_batch_edit(&mut self, a: &J) -> R<ToolResult> {
        let ops = a.get("ops").and_then(|v| v.as_array()).ok_or("missing \"ops\" (array)")?;
        let mut results = Vec::with_capacity(ops.len());
        let mut errors = 0usize;
        for (i, op) in ops.iter().enumerate() {
            let kind = s(op, "op").unwrap_or("");
            let res: R<ToolResult> = match kind {
                "create" | "create_instance" => self.mcp_create(op),
                "set_properties" => self.mcp_set_properties(op),
                "set_source" => self.mcp_set_source(op),
                "clone" | "clone_instance" => self.mcp_clone(op),
                "move" | "move_instance" => self.mcp_move(op),
                "delete" | "delete_instances" => self.mcp_delete(op),
                "build_tree" => self.mcp_build_tree(op),
                other => Err(format!("Unknown batch op {other:?}")),
            };
            match res {
                Ok(tr) => results.push(json!({ "index": i, "op": kind, "ok": !tr.is_error, "result": tr.summary() })),
                Err(e) => {
                    errors += 1;
                    results.push(json!({ "index": i, "op": kind, "ok": false, "error": e }));
                }
            }
        }
        let mut out = ToolResult::json(&json!({
            "total": ops.len(),
            "errors": errors,
            "results": results,
        }));
        out.is_error = errors > 0;
        Ok(out)
    }

    pub(super) fn mcp_edit_terrain(&mut self, a: &J) -> R<ToolResult> {
        let ws = self.game.doc.workspace().ok_or("Workspace is missing")?;
        let terrain_ref = self
            .game
            .doc
            .find_child(ws, "Terrain")
            .ok_or("Workspace.Terrain is missing")?;
        let bytes: Vec<u8> = match self
            .game
            .doc
            .dom
            .get_by_ref(terrain_ref)
            .and_then(|i| i.properties.get(&ustr("SmoothGrid")))
        {
            Some(Variant::BinaryString(b)) => AsRef::<[u8]>::as_ref(b).to_vec(),
            _ => Vec::new(),
        };
        let mut vox = Voxels::decode(&bytes).unwrap_or_else(|_| Voxels::empty());
        let op = req(a, "op")?.to_ascii_lowercase();
        let mat_of = |name: &str| -> R<u8> {
            if name.eq_ignore_ascii_case("air") {
                return Ok(0);
            }
            terrain::material_index(name).ok_or_else(|| format!("Unknown terrain material {name:?} (e.g. Grass, Rock, Sand, Water, Snow, Asphalt, Mud, LeafyGrass, Cobblestone, Ice, Air)"))
        };

        let summary = match op.as_str() {
            "clear" => {
                vox.clear();
                "Cleared all Terrain voxels".to_string()
            }
            "fill_block" => {
                let c = vec3_arg(a, "center").ok_or("fill_block needs \"center\": [x, y, z]")?;
                let sz = vec3_arg(a, "size").ok_or("fill_block needs \"size\": [x, y, z]")?;
                let mat = mat_of(s(a, "material").unwrap_or("Grass"))?;
                let half = [sz[0].abs() * 0.5, sz[1].abs() * 0.5, sz[2].abs() * 0.5];
                let lo = [c[0] - half[0], c[1] - half[1], c[2] - half[2]];
                let hi = [c[0] + half[0], c[1] + half[1], c[2] + half[2]];
                vox.fill(lo, hi, mat, |p| {
                    let dx = (p[0] - c[0]).abs() - half[0];
                    let dy = (p[1] - c[1]).abs() - half[1];
                    let dz = (p[2] - c[2]).abs() - half[2];
                    dx.max(dy).max(dz)
                });
                format!("Filled block at {c:?} size {sz:?} with {}", s(a, "material").unwrap_or("Grass"))
            }
            "fill_ball" => {
                let c = vec3_arg(a, "center").ok_or("fill_ball needs \"center\": [x, y, z]")?;
                let r = num(a, "radius").ok_or("fill_ball needs \"radius\"")? as f32;
                let mat = mat_of(s(a, "material").unwrap_or("Grass"))?;
                let lo = [c[0] - r, c[1] - r, c[2] - r];
                let hi = [c[0] + r, c[1] + r, c[2] + r];
                vox.fill(lo, hi, mat, |p| {
                    let d = Vec3::new(p[0] - c[0], p[1] - c[1], p[2] - c[2]).length();
                    d - r
                });
                format!("Filled ball at {c:?} radius {r} with {}", s(a, "material").unwrap_or("Grass"))
            }
            "fill_cylinder" => {
                let c = vec3_arg(a, "center").ok_or("fill_cylinder needs \"center\": [x, y, z]")?;
                let r = num(a, "radius").ok_or("fill_cylinder needs \"radius\"")? as f32;
                let h = num(a, "height").ok_or("fill_cylinder needs \"height\"")? as f32;
                let mat = mat_of(s(a, "material").unwrap_or("Grass"))?;
                let hh = h.abs() * 0.5;
                let lo = [c[0] - r, c[1] - hh, c[2] - r];
                let hi = [c[0] + r, c[1] + hh, c[2] + r];
                vox.fill(lo, hi, mat, |p| {
                    let dr = ((p[0] - c[0]).hypot(p[2] - c[2])) - r;
                    let dy = (p[1] - c[1]).abs() - hh;
                    dr.max(dy)
                });
                format!("Filled cylinder at {c:?} r={r} h={h} with {}", s(a, "material").unwrap_or("Grass"))
            }
            "replace_material" => {
                let c = vec3_arg(a, "center").ok_or("replace_material needs \"center\": [x, y, z]")?;
                let sz = vec3_arg(a, "size").ok_or("replace_material needs \"size\": [x, y, z]")?;
                let from = mat_of(req(a, "from_material")?)?;
                let to = mat_of(req(a, "to_material")?)?;
                let lo = [0, 1, 2].map(|k| ((c[k] - sz[k].abs() * 0.5) / VOXEL).floor() as i32);
                let hi = [0, 1, 2].map(|k| ((c[k] + sz[k].abs() * 0.5) / VOXEL).ceil() as i32);
                vox.replace(lo, hi, from, to);
                format!("Replaced {} with {} in region", req(a, "from_material")?, req(a, "to_material")?)
            }
            other => {
                return Err(format!(
                    "Unknown terrain op {other:?}. Use: fill_block, fill_ball, fill_cylinder, replace_material, clear"
                ))
            }
        };

        self.game.doc.set_props(
            "Edit terrain (MCP)",
            vec![(terrain_ref, ustr("SmoothGrid"), Variant::BinaryString(BinaryString::from(vox.encode())))],
            None,
        );
        Ok(ToolResult::json(&json!({ "summary": summary, "voxels": vox.count() })))
    }

    pub(super) fn mcp_create_animation(&mut self, a: &J) -> R<ToolResult> {
        let parent = self.mcp_resolve(s(a, "parent").unwrap_or("ReplicatedStorage"))?;
        let name = s(a, "name").unwrap_or("CustomAnimation");
        let rig = if s(a, "rig").is_some_and(|r| r.eq_ignore_ascii_case("r6")) {
            RigType::R6
        } else {
            RigType::R15
        };

        let seq_builder = if let Some(preset) = s(a, "preset") {
            templates::build_preset_animation(name, preset, rig)?
        } else {
            let kfs = a
                .get("keyframes")
                .and_then(|v| v.as_array())
                .ok_or("Provide either \"preset\" (idle/walk/run/jump/wave/punch/slash/dance) or \"keyframes\" array")?;
            let looped = flag(a, "loop", true);
            let priority = num(a, "priority").unwrap_or(3.0) as u32;
            let mut seq = InstanceBuilder::new("KeyframeSequence")
                .with_name(name)
                .with_property("Loop", Variant::Bool(looped))
                .with_property("Priority", Enum::from_u32(priority));
            for (idx, kf) in kfs.iter().enumerate() {
                let time = num(kf, "time").unwrap_or(idx as f64 * 0.25) as f32;
                let mut root_pose = InstanceBuilder::new("Pose")
                    .with_name("HumanoidRootPart")
                    .with_property("CFrame", CFrame::identity())
                    .with_property("Weight", 1.0f32);
                if let Some(poses) = kf.get("poses").and_then(|p| p.as_array()) {
                    for pose in poses {
                        let part = req(pose, "part")?;
                        let pos = vec3_arg(pose, "position").unwrap_or([0.0, 0.0, 0.0]);
                        let rot = vec3_arg(pose, "rotation").unwrap_or([0.0, 0.0, 0.0]);
                        let weight = num(pose, "weight").unwrap_or(1.0) as f32;
                        let es = easing_style_id(s(pose, "easing_style").unwrap_or("cubic"));
                        let ed = easing_dir_id(s(pose, "easing_direction").unwrap_or("inout"));
                        root_pose.add_child(
                            InstanceBuilder::new("Pose")
                                .with_name(part)
                                .with_property("CFrame", templates::cframe_from_pos_euler_deg(pos, rot))
                                .with_property("Weight", weight)
                                .with_property("EasingStyle", Enum::from_u32(es))
                                .with_property("EasingDirection", Enum::from_u32(ed)),
                        );
                    }
                }
                let mut kf_b = InstanceBuilder::new("Keyframe")
                    .with_name(s(kf, "name").map(String::from).unwrap_or_else(|| format!("Keyframe_{idx}")))
                    .with_property("Time", time)
                    .with_child(root_pose);
                if let Some(markers) = kf.get("markers").and_then(|m| m.as_array()) {
                    for m in markers {
                        let mn = req(m, "name")?;
                        let mv = s(m, "value").unwrap_or("");
                        kf_b.add_child(
                            InstanceBuilder::new("KeyframeMarker")
                                .with_name(mn)
                                .with_property("Value", Variant::String(mv.to_string())),
                        );
                    }
                }
                seq.add_child(kf_b);
            }
            seq
        };

        let r = self.game.doc.insert(parent, seq_builder);
        Ok(ToolResult::json(&json!({
            "path": tree::path(&self.game.doc, r),
            "id": tree::id(r),
            "name": name,
        })))
    }

    pub(super) fn mcp_spawn_rig(&mut self, a: &J) -> R<ToolResult> {
        let parent = self.mcp_resolve(s(a, "parent").unwrap_or("Workspace"))?;
        let name = s(a, "name").unwrap_or("Rig");
        let rig = if s(a, "rig").is_some_and(|r| r.eq_ignore_ascii_case("r6")) {
            RigType::R6
        } else {
            RigType::R15
        };
        let pos = vec3_arg(a, "position").unwrap_or([0.0, 0.0, 0.0]);
        let anchored = flag(a, "anchored", false);
        let ai = s(a, "ai").unwrap_or("none");
        let colors = a.get("colors").and_then(|c| c.as_object()).map(|o| {
            let rgb = |k: &str, def: [u8; 3]| -> [u8; 3] {
                o.get(k)
                    .and_then(|v| v.as_array())
                    .filter(|arr| arr.len() >= 3)
                    .and_then(|arr| Some([arr[0].as_u64()? as u8, arr[1].as_u64()? as u8, arr[2].as_u64()? as u8]))
                    .unwrap_or(def)
            };
            let d = BodyColors::default();
            BodyColors {
                head: rgb("head", d.head),
                torso: rgb("torso", d.torso),
                left_arm: rgb("left_arm", d.left_arm),
                right_arm: rgb("right_arm", d.right_arm),
                left_leg: rgb("left_leg", d.left_leg),
                right_leg: rgb("right_leg", d.right_leg),
            }
        });

        let r = templates::spawn_rig(&mut self.game.doc, parent, name, rig, pos, anchored, colors, ai);
        Ok(ToolResult::json(&json!({
            "path": tree::path(&self.game.doc, r),
            "id": tree::id(r),
            "rig": if rig == RigType::R6 { "R6" } else { "R15" },
            "position": pos,
            "ai": ai,
        })))
    }

    pub(super) fn mcp_apply_lighting_preset(&mut self, a: &J) -> R<ToolResult> {
        let preset = req(a, "preset")?;
        let msg = templates::apply_lighting_preset(&mut self.game.doc, preset)?;
        Ok(ToolResult::text(msg))
    }

    pub(super) fn mcp_spawn_game_template(&mut self, a: &J) -> R<ToolResult> {
        let template = req(a, "template")?;
        let clear = flag(a, "clear_workspace", false);
        let msg = templates::spawn_game_template(&mut self.game.doc, template, clear)?;
        Ok(ToolResult::text(msg))
    }
}
