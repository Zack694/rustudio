//! The tools the MCP server offers: name, group, whether a call changes
//! something, the description an agent reads and the input schema. Every
//! tool is always listed (a client may cache the list); a call to one whose
//! group is off in the panel fails with a message saying which switch.

use serde_json::{json, Value as J};

use super::settings::Group;

pub struct ToolDef {
    pub name: &'static str,
    pub group: Group,
    /// Changes the place, the session or the files (confirmation applies).
    pub write: bool,
    /// Writes the edit DataModel: refused during Play, and needs the live
    /// group when the open place is a Team Create session.
    pub doc_write: bool,
    pub description: &'static str,
    /// The `properties` of the input object schema, as JSON text.
    pub props: &'static str,
    pub required: &'static [&'static str],
}

const PATH: &str = r##"{"type": "string", "description": "An instance path as GetFullName gives it (\"Workspace.Map.Door\", \"game.ServerScriptService.Main\"; a name with dots or brackets is written [\"a.b\"]), or an id from get_tree (\"#1a2b…\")"}"##;

macro_rules! tool {
    ($name:literal, $group:ident, $write:literal, $doc:literal, $desc:literal, $props:expr, [$($req:literal),*]) => {
        ToolDef { name: $name, group: Group::$group, write: $write, doc_write: $doc, description: $desc, props: $props, required: &[$($req),*] }
    };
}

pub static TOOLS: &[ToolDef] = &[
    // ---- read
    tool!("session_info", Read, false, false,
        "This rustudio window: pid, title, the open place (file, Roblox place, live Team Create session), mode (edit / play / run and the side shown), whether it has unsaved changes, instance count, which MCP tool groups are on, and the other rustudio windows with MCP on (their URLs).",
        "{}", []),
    tool!("get_tree", Read, false, false,
        "Lists instances under a path, paged: path, class, id and child count each. Without filters it lists `depth` levels (default 1); with `class` / `name` it searches the whole subtree. Use `offset` with the returned `next_offset` for more.",
        r#"{"path": {"type": "string", "description": "Where to start (default \"game\"); a path or an id"},
            "depth": {"type": "integer", "minimum": 1, "maximum": 64},
            "class": {"type": "string", "description": "Only instances of this class or a subclass (IsA), e.g. \"LuaSourceContainer\", \"BasePart\""},
            "name": {"type": "string", "description": "Only names containing this (case-insensitive)"},
            "offset": {"type": "integer", "minimum": 0},
            "limit": {"type": "integer", "minimum": 1, "maximum": 2000, "description": "Default 200"}}"#, []),
    tool!("get_properties", Read, false, false,
        "An instance's class, path, properties (as project-file JSON: [x, y, z], enum names, …), attributes and tags. Lists the properties the instance has stored, or the ones named in `properties` (with defaults). Script Source is left out unless named (its length is given): use get_source.",
        r#"{"path": PATH, "properties": {"type": "array", "items": {"type": "string"}}}"#, ["path"]),
    tool!("get_source", Read, false, false,
        "A script's Source, optionally a line range (1-based, inclusive). Lines come back numbered.",
        r#"{"path": PATH, "start_line": {"type": "integer", "minimum": 1}, "end_line": {"type": "integer", "minimum": 1}}"#, ["path"]),
    tool!("grep_scripts", Read, false, false,
        "Searches every script's Source for a substring (case-sensitive); returns path:line: text for each match.",
        r#"{"pattern": {"type": "string"}, "limit": {"type": "integer", "minimum": 1, "maximum": 2000, "description": "Default 200 matches"}}"#, ["pattern"]),
    tool!("read_output", Read, false, false,
        "The Output panel's lines since a cursor (each line has a `seq`; pass the returned `next` back as `since` to tail). Filter by level and text. The first call without `since` returns the last `limit` lines.",
        r#"{"since": {"type": "integer", "minimum": 0}, "levels": {"type": "array", "items": {"enum": ["print", "info", "warn", "error"]}}, "contains": {"type": "string"}, "limit": {"type": "integer", "minimum": 1, "maximum": 5000, "description": "Default 200"}}"#, []),
    tool!("screenshot", Read, false, false,
        "Captures a PNG screenshot once everything in sight has loaded (or after 20 s). Default mode \"viewport\" includes both the 3D scene and all 2D ScreenGui / PlayerGui / HUD overlays; \"window\" captures the entire Studio window; \"3d\" captures the raw 3D framebuffer only. Always saved inside `/storage/emulated/0/rustudio/screenshots/` on Android (or `<Pictures>/rustudio/` on desktop) unless `path` overrides it.",
        r#"{"path": {"type": "string", "description": "Optional custom absolute path to save the PNG (defaults to the rustudio screenshots folder)"},
            "mode": {"enum": ["viewport", "window", "3d"], "description": "Default \"viewport\" (3D + 2D GUI overlays). Use \"window\" for the entire Studio UI or \"3d\" for raw 3D only."}}"#, []),
    tool!("inspect_gui", Read, false, false,
        "Lists all ScreenGui, BillboardGui, SurfaceGui and GuiObject instances with visibility, text, image, ZIndex, and live laid-out screen coordinates (abs_pos, abs_size, click_center ready for send_input click, text_bounds). Works in Edit and Play modes.",
        r#"{"path": {"type": "string", "description": "Root path to inspect (default \"game\")"},
            "visible_only": {"type": "boolean", "description": "Default false"},
            "limit": {"type": "integer", "minimum": 1, "maximum": 2000}}"#, []),
    tool!("describe_class", Read, false, false,
        "Queries Roblox api-dump.json reflection database: with `class`, returns its superclass chain, creatable/service flags, properties, functions, events and callbacks; without `class`, lists/searches Roblox classes by `query` or `base` superclass.",
        r#"{"class": {"type": "string"},
            "inherited": {"type": "boolean", "description": "Include inherited members (default false)"},
            "include_hidden": {"type": "boolean", "description": "Include hidden/not-browsable members (default false)"},
            "query": {"type": "string", "description": "Substring filter when listing classes"},
            "base": {"type": "string", "description": "Only classes inheriting from this base class"},
            "creatable_only": {"type": "boolean"},
            "limit": {"type": "integer", "minimum": 1, "maximum": 2000}}"#, []),
    tool!("list_enums", Read, false, false,
        "Queries Roblox api-dump.json enums: with `enum_name`, returns all items and integer values; otherwise lists enum names matching `query`.",
        r#"{"enum_name": {"type": "string"},
            "query": {"type": "string"},
            "limit": {"type": "integer", "minimum": 1, "maximum": 1000}}"#, []),
    tool!("search_toolbox", Read, false, false,
        "Searches the Roblox Creator Store (Toolbox) across models, audio, decals, meshparts, plugins, fonts, videos, animations, or packages (requires Roblox sign-in). Returns asset IDs, names, creators, prices, votes, and pagination cursor.",
        r#"{"query": {"type": "string"},
            "kind": {"enum": ["model", "audio", "decal", "meshpart", "plugin", "font", "video", "animation", "package"], "description": "Default \"model\""},
            "sort": {"enum": ["relevance", "ratings", "newest"]},
            "free_only": {"type": "boolean"},
            "include_unverified": {"type": "boolean"},
            "creator": {"type": "string"},
            "category_path": {"type": "string"},
            "section": {"type": "string", "description": "Optional home swimlane (trending, priced, essential)"},
            "cursor": {"type": "string"}}"#, []),
    tool!("browse_inventory", Read, false, false,
        "Browses your signed-in Roblox Inventory, Creations, or Recent assets (optionally for a group_id).",
        r#"{"source": {"enum": ["inventory", "creations", "recent"], "description": "Default \"inventory\""},
            "kind": {"enum": ["model", "audio", "decal", "meshpart", "plugin", "font", "video", "animation", "package"]},
            "group_id": {"type": "integer"},
            "query": {"type": "string"},
            "cursor": {"type": "string"}}"#, []),
    tool!("get_asset_details", Read, false, false,
        "Fetches Roblox Creator Store metadata (name, description, creator, votes, price, triangles, scripts, audio info) for 1 to 30 asset IDs.",
        r#"{"asset_id": {"type": "integer"},
            "asset_ids": {"type": "array", "items": {"type": "integer"}}}"#, []),
    tool!("list_plugins", Read, false, false,
        "The Studio plugins loaded in this window: id, name, file, whether running, errors.",
        "{}", []),
    tool!("file_sync_status", Read, false, false,
        "File Sync (the File Sync button): the Rojo / Argon project attached to this window's place, if any — project file, place, mapped paths and whether the place has them, changes waiting for an OK (with the reason: threshold, live guard, attach preview), live script commits being followed (sent / conflict), the last sync, the error, the settings that matter, and the last `log_lines` log entries (each change with its file and, in live places, sent / confirmed / failed).",
        r#"{"log_lines": {"type": "integer", "minimum": 0, "maximum": 500, "description": "Default 30"}}"#, []),
    tool!("file_sync_diff", Read, false, false,
        "Compares every file of the attached project with the open place and lists what \"files win\" would change now (create / set / delete, with the file each comes from); changes nothing. Use it to see whether the window has what's on disk. source_diffs adds line diffs of script sources.",
        r#"{"source_diffs": {"type": "boolean", "description": "Default false"}, "diff_lines": {"type": "integer", "minimum": 1, "maximum": 5000, "description": "Lines per script diff, default 200"}}"#, []),
    tool!("team_create_status", Read, false, false,
        "The live Team Create session of this window, if any: joining / connected, place and universe, instance count, edits that could not be sent (unsynced) and why, the edits sent in the last 10 s (a disconnect may still drop those), script drafts (path, submitted, conflicting) and script commits waiting for the server. The server doesn't acknowledge plain edits: a rejected one ends the connection, and edits that can't be sent are warned about in the Output (team_create_wait collects them).",
        "{}", []),
    // ---- edit
    tool!("run_luau", Edit, true, false,
        "Runs Luau like the Command Bar and returns the chunk's return values (as JSON; Instances as full names) and the Output printed while it ran. target \"edit\" (default) runs in the edit DataModel (needs no Play; on a live Team Create place its writes reach the server and need the live group); \"server\" / \"client\" run in the Play DataModels (Play group). The chunk may yield (task.wait); it is waited for up to `timeout` seconds.",
        r#"{"code": {"type": "string"}, "target": {"enum": ["edit", "server", "client"]}, "timeout": {"type": "number", "minimum": 0.1, "maximum": 600, "description": "Seconds, default 15"}}"#, ["code"]),
    tool!("set_properties", Edit, true, true,
        "Sets properties, attributes and tags of one instance as one undo step. Values are project-file JSON (Rojo): true, 4, \"Neon\", [1, 2, 3], [r, g, b] (0-1, or 0-255 when a part's Color has a component above 1), {\"Vector3\": [1,2,3]} for an explicit type. Name renames. Attributes: {name: value or null to remove}. Everything is checked before anything changes.",
        r#"{"path": PATH, "properties": {"type": "object"}, "attributes": {"type": "object"}, "tags": {"type": "array", "items": {"type": "string"}, "description": "Replaces the instance's tags"}}"#, ["path"]),
    tool!("set_source", Edit, true, true,
        "Replaces a script's Source (one undo step). On a live Team Create place this makes a private draft; commit it with team_create_commit_drafts.",
        r#"{"path": PATH, "source": {"type": "string"}}"#, ["path", "source"]),
    tool!("create_instance", Edit, true, true,
        "Creates an instance under `parent` with optional name and properties (project-file JSON values). Returns its path and id.",
        r#"{"parent": PATH, "class": {"type": "string"}, "name": {"type": "string"}, "properties": {"type": "object"}}"#, ["parent", "class"]),
    tool!("clone_instance", Edit, true, true,
        "Copies an instance (with its descendants) under `parent` (default: its own parent). Returns the copy's path and id.",
        r#"{"path": PATH, "parent": PATH}"#, ["path"]),
    tool!("delete_instances", Edit, true, true,
        "Deletes instances (and their descendants) in one undo step. Services and the DataModel can't be deleted.",
        r#"{"paths": {"type": "array", "items": PATH}}"#, ["paths"]),
    tool!("move_instance", Edit, true, true,
        "Moves an instance under a new parent and/or renames it.",
        r#"{"path": PATH, "parent": PATH, "name": {"type": "string"}}"#, ["path"]),
    tool!("insert_model", Edit, true, true,
        "Inserts the instances of a .rbxm / .rbxmx file under `parent` (default Workspace).",
        r#"{"file": {"type": "string", "description": "Absolute path"}, "parent": PATH}"#, ["file"]),
    tool!("insert_asset", Edit, true, true,
        "Downloads a Roblox asset by `asset_id` (from Creator Store, Inventory, or public catalog) and inserts it directly into the place (Models/Packages/MeshParts/Decals/Audio/Animations/Videos), optionally positioned at `position` [x, y, z].",
        r#"{"asset_id": {"type": "integer"},
            "parent": PATH,
            "position": {"type": "array", "items": {"type": "number"}},
            "kind": {"enum": ["model", "audio", "decal", "meshpart", "plugin", "font", "video", "animation", "package"]}}"#, ["asset_id"]),
    tool!("download_asset", Places, true, false,
        "Downloads the raw bytes of a Roblox asset by `asset_id` to an absolute file `path` on disk.",
        r#"{"asset_id": {"type": "integer"},
            "path": {"type": "string", "description": "Absolute destination file path"}}"#, ["asset_id", "path"]),
    tool!("upload_asset", Places, true, false,
        "Uploads a local file (PNG/JPG/BMP/TGA image/decal, OGG/MP3/WAV/FLAC audio, RBXM/RBXMX model, FBX/OBJ/GLTF/GLB mesh) to Roblox using the signed-in account and returns the new `asset_id` and `rbxassetid://` URI.",
        r#"{"path": {"type": "string", "description": "Absolute path of the local file to upload"},
            "name": {"type": "string"},
            "description": {"type": "string"},
            "asset_type": {"enum": ["Image", "Decal", "Audio", "Model", "Animation", "Mesh"]},
            "group_id": {"type": "integer"}}"#, ["path"]),
    tool!("publish_place", Places, true, false,
        "Saves or publishes the open place directly to Roblox (`universe_id` and `place_id` default to the open cloud place if linked).",
        r#"{"universe_id": {"type": "integer"},
            "place_id": {"type": "integer"},
            "publish": {"type": "boolean", "description": "true to Publish (default), false to Save"}}"#, []),
    tool!("build_tree", Edit, true, true,
        "Creates an entire nested hierarchy of Roblox instances (Parts, Models, ScreenGuis, Scripts with `source`, Lights, Sounds, ParticleEmitters, Constraints, Attributes, Tags) in one call. Pass `node` (object) or `nodes` (array) with `{class, name?, properties?, source?, attributes?, tags?, children?: [...]}`.",
        r#"{"parent": PATH,
            "node": {"type": "object"},
            "nodes": {"type": "array", "items": {"type": "object"}}}"#, []),
    tool!("batch_edit", Edit, true, true,
        "Executes a list of edit operations in one call. Each item in `ops` has `op`: \"create\" | \"set_properties\" | \"set_source\" | \"clone\" | \"move\" | \"delete\" | \"build_tree\", plus that operation usual fields.",
        r#"{"ops": {"type": "array", "items": {"type": "object"}}}"#, ["ops"]),
    tool!("edit_terrain", Edit, true, true,
        "Edits Workspace.Terrain voxels in edit mode with undo support: `op` is \"fill_block\", \"fill_ball\", \"fill_cylinder\", \"replace_material\", or \"clear\".",
        r#"{"op": {"enum": ["fill_block", "fill_ball", "fill_cylinder", "replace_material", "clear"]},
            "center": {"type": "array", "items": {"type": "number"}},
            "size": {"type": "array", "items": {"type": "number"}},
            "radius": {"type": "number"},
            "height": {"type": "number"},
            "material": {"type": "string", "description": "Grass, Rock, Sand, Water, Snow, Asphalt, Mud, Cobblestone, Ice, Air, ..."},
            "from_material": {"type": "string"},
            "to_material": {"type": "string"}}"#, ["op"]),
    tool!("create_animation", Edit, true, true,
        "Creates a KeyframeSequence animation instance in the place (default parent ReplicatedStorage), either from a built-in `preset` (\"idle\", \"walk\", \"run\", \"jump\", \"wave\", \"punch\", \"slash\", \"dance\") or custom `keyframes`: [{time, poses: [{part, position?, rotation?, weight?, easing_style?, easing_direction?}], markers?: [{name, value}]}].",
        r#"{"name": {"type": "string"},
            "parent": PATH,
            "rig": {"enum": ["R15", "R6"]},
            "preset": {"enum": ["idle", "walk", "run", "jump", "wave", "punch", "slash", "dance"]},
            "loop": {"type": "boolean"},
            "priority": {"type": "integer"},
            "keyframes": {"type": "array", "items": {"type": "object"}}}"#, []),
    tool!("spawn_rig", Edit, true, true,
        "Spawns a complete R15 or R6 character rig / NPC dummy (with Humanoid, Motor6Ds, Attachments, BodyColors) at `position` [x, y, z], with optional AI script (`ai`: \"none\", \"patrol\", \"chase\").",
        r#"{"name": {"type": "string"},
            "parent": PATH,
            "rig": {"enum": ["R15", "R6"]},
            "position": {"type": "array", "items": {"type": "number"}},
            "anchored": {"type": "boolean"},
            "ai": {"enum": ["none", "patrol", "chase"]},
            "colors": {"type": "object"}}"#, []),
    tool!("apply_lighting_preset", Edit, true, true,
        "Applies a complete Roblox Lighting + Atmosphere + Bloom + ColorCorrection + SunRays preset in one call: \"daylight\", \"sunset\", \"night\", \"horror\", \"cyberpunk\", \"scifi\", or \"studio\".",
        r#"{"preset": {"enum": ["daylight", "sunset", "night", "horror", "cyberpunk", "scifi", "studio"]}}"#, ["preset"]),
    tool!("spawn_game_template", Edit, true, true,
        "Builds a complete playable starter game (map, lighting, spawns, HUD ScreenGui, Server + Client Luau scripts) into the open place: \"fps_arena\", \"obby\", \"horror_chase\", \"combat_arena\", or \"tycoon\".",
        r#"{"template": {"enum": ["fps_arena", "obby", "horror_chase", "combat_arena", "tycoon"]},
            "clear_workspace": {"type": "boolean", "description": "Remove existing Workspace parts first (default false)"}}"#, ["template"]),
    tool!("apply_project", Edit, true, true,
        "Pushes a Rojo / Argon project into the open place: reads the project file (or the folder's default.project.json), compares every instance it maps with the place, and creates what's missing, sets what differs (script Source included) and, with delete_extra, deletes what the project's folders no longer have. dry_run (the default) only reports the plan. Applied as one undo step; on a live Team Create place script changes become drafts to commit.",
        r#"{"project": {"type": "string", "description": "A *.project.json or a folder with one"}, "dry_run": {"type": "boolean", "description": "Default true"}, "delete_extra": {"type": "boolean", "description": "Delete place instances under mapped folders that the project doesn't have (default false)"}}"#, ["project"]),
    tool!("file_sync_attach", Edit, true, true,
        "Attaches a Rojo / Argon project to the open place (any place: a file, a Roblox place, a live Team Create session) as the File Sync button does: files saved on disk from then on reach the place (and the Team Create server). on_attach: \"preview\" (default: the differences wait, see file_sync_status; file_sync_apply makes the files win), \"files\" (applied at once, still waiting over the threshold / in live places), \"place\" (the place is kept; only later file changes sync). remember (default true) attaches it again when this place opens.",
        r#"{"project": {"type": "string", "description": "A *.project.json or a folder with one (absolute)"}, "on_attach": {"enum": ["preview", "files", "place"]}, "remember": {"type": "boolean"}}"#, ["project"]),
    tool!("file_sync_apply", Edit, true, true,
        "Applies the File Sync changes waiting for an OK (planned again against the files and the place as they are now), or with all=true makes the whole place match every file now; skip=true drops the waiting changes instead. Returns the log of what changed. In a live place, script sources are committed and followed (file_sync_status).",
        r#"{"all": {"type": "boolean"}, "skip": {"type": "boolean"}}"#, []),
    tool!("file_sync_detach", Edit, true, false,
        "Detaches the File Sync project (files stop syncing). forget=true also stops it attaching again when this place opens.",
        r#"{"forget": {"type": "boolean"}}"#, []),
    tool!("undo", Edit, true, true, "Undoes the last change (as Ctrl+Z).", "{}", []),
    tool!("redo", Edit, true, true, "Redoes the last undone change (as Ctrl+Y).", "{}", []),
    tool!("load_plugin", Edit, true, false,
        "Loads a Studio plugin file (.rbxm / .rbxmx / .lua / .luau) into the open window, as if it were in the Plugins folder (it runs at once).",
        r#"{"file": {"type": "string", "description": "Absolute path"}}"#, ["file"]),
    tool!("reload_plugin", Edit, true, false,
        "Reads a loaded plugin's file again and restarts it (by id or name from list_plugins).",
        r#"{"plugin": {"type": ["string", "integer"]}}"#, ["plugin"]),
    tool!("unload_plugin", Edit, true, false,
        "Stops and removes a loaded plugin (by id or name).",
        r#"{"plugin": {"type": ["string", "integer"]}}"#, ["plugin"]),
    tool!("ui_action", Edit, true, false,
        "Runs an editor menu / ribbon action by id, as clicking it: e.g. \"save\", \"test\", \"stop\", \"group\", \"panel_output\", or a plugin's \"plugin-button:TOOLBAR/BUTTON\" / \"plugin-action:ID\".",
        r#"{"action": {"type": "string"}}"#, ["action"]),
    // ---- play
    tool!("play", Play, true, false,
        "Starts a test session and waits until it runs: mode \"play\" (Play: server + your client, the default) or \"run\" (server only). Returns the session's state.",
        r#"{"mode": {"enum": ["play", "run"]}}"#, []),
    tool!("stop", Play, true, false, "Stops Play / Run and waits until the editor is back in edit mode.", "{}", []),
    tool!("send_input", Play, true, false,
        "Sends keyboard and mouse input to the window as a person would (in Play the game gets it). Each step: {\"key\": \"W\", \"hold\": 0.5} (egui key names: A-Z, Num0-9, Space, Enter, Escape, Tab, ArrowUp…, F1…), {\"click\": [x, y]} (3D view pixels), {\"type\": \"text\"}, {\"wheel\": notches}, {\"move\": [dx, dy]} (raw mouse motion), with optional \"at\" seconds from now. Waits until the last step has happened.",
        r#"{"steps": {"type": "array", "items": {"type": "object"}}}"#, ["steps"]),
    tool!("set_camera", Play, true, false,
        "Moves the editor camera (edit mode or the server view): position [x, y, z], yaw and pitch in degrees, optional vertical fov.",
        r#"{"position": {"type": "array", "items": {"type": "number"}}, "yaw": {"type": "number"}, "pitch": {"type": "number"}, "fov": {"type": "number"}}"#, ["position"]),
    // ---- places
    tool!("open_place", Places, true, false,
        "Opens a place or model file (.rbxl / .rbxlx / .rbxm / .rbxmx) or a project folder / *.project.json, replacing the open place. Refuses when the open place has unsaved changes unless discard_changes is true.",
        r#"{"file": {"type": "string"}, "discard_changes": {"type": "boolean"}}"#, ["file"]),
    tool!("open_roblox_place", Places, true, false,
        "Opens a place from Roblox (as File > Open from Roblox), optionally a version or as a local copy, and waits until it has loaded.",
        r#"{"place_id": {"type": "integer"}, "version": {"type": "integer"}, "local_copy": {"type": "boolean"}, "discard_changes": {"type": "boolean"}}"#, ["place_id"]),
    tool!("join_team_create", Places, true, false,
        "Joins a Team Create session (universe and place ids) and waits until its DataModel is in (up to 3 minutes).",
        r#"{"universe_id": {"type": "integer"}, "place_id": {"type": "integer"}, "discard_changes": {"type": "boolean"}}"#, ["universe_id", "place_id"]),
    tool!("save_place", Places, true, false,
        "Writes the open place to a file (.rbxl binary or .rbxlx XML by extension). By default a copy: the window keeps its place (a live Team Create session stays live). copy=false makes the file the place's own (as File > Save As), not allowed for live places.",
        r#"{"file": {"type": "string", "description": "Absolute path"}, "copy": {"type": "boolean", "description": "Default true"}}"#, ["file"]),
    // ---- live
    tool!("team_create_commit_drafts", Live, true, false,
        "Commits live script drafts to the Team Create server (all of them, or the given scripts), as the script tab's Commit button. Reports what was sent; the server confirms commits asynchronously (see team_create_status).",
        r#"{"paths": {"type": "array", "items": PATH}}"#, []),
    tool!("team_create_discard_drafts", Live, true, false,
        "Throws away live script drafts (all, or the given scripts) and puts the server's Source back.",
        r#"{"paths": {"type": "array", "items": PATH}}"#, []),
    tool!("team_create_wait", Live, false, false,
        "Waits until the live session has had no edit sent for `quiet` seconds (default 3) and no pending script commits, or `timeout` (default 30). Reports whether the connection is still up, edits that failed or were not sent since `since` (a seq from team_create_status), and the drafts left. Use after a batch of writes before verifying.",
        r#"{"quiet": {"type": "number", "minimum": 0, "maximum": 60}, "timeout": {"type": "number", "minimum": 1, "maximum": 600}}"#, []),
];

pub fn find(name: &str) -> Option<&'static ToolDef> {
    TOOLS.iter().find(|t| t.name == name)
}

/// The `tools/list` entries.
pub fn list() -> Vec<J> {
    TOOLS
        .iter()
        .map(|t| {
            let props: J = serde_json::from_str(&t.props.replace("PATH", PATH)).expect("tool schema JSON");
            let mut schema = json!({ "type": "object", "properties": props });
            if !t.required.is_empty() {
                schema["required"] = json!(t.required);
            }
            json!({
                "name": t.name,
                "description": format!("[{}] {}", t.group.key(), t.description),
                "inputSchema": schema,
                "annotations": {
                    "readOnlyHint": !t.write,
                    "destructiveHint": t.write,
                    "openWorldHint": matches!(t.group, Group::Live | Group::Places),
                },
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_schema_parses_and_names_are_unique() {
        let list = list();
        assert_eq!(list.len(), TOOLS.len());
        let mut names: Vec<&str> = TOOLS.iter().map(|t| t.name).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), TOOLS.len());
        for t in &list {
            let props = t["inputSchema"]["properties"].as_object().unwrap();
            for r in t["inputSchema"]["required"].as_array().into_iter().flatten() {
                assert!(props.contains_key(r.as_str().unwrap()), "{}: required {r} isn't a property", t["name"]);
            }
        }
        // Read tools never write; live tools are the live group's.
        assert!(TOOLS.iter().filter(|t| t.group == Group::Read).all(|t| !t.write && !t.doc_write));
        assert!(find("team_create_commit_drafts").is_some_and(|t| t.group == Group::Live));
    }
}
