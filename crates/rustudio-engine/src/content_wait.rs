//! Script loads that yield while their asset downloads, as Studio's `*Async`
//! loads do (the calling thread waits; every other script, the physics and
//! the frame keep running). The asset is fetched on the asset workers, the
//! thread polls with `task.wait()`, then the method itself runs with the
//! bytes already on disk. Before, the method fetched inline: one uncached
//! CreateMeshPartAsync stalled the whole session for the download.

use super::*;

/// Methods that load the content named by their first argument.
const LOADS: &[&str] = &["CreateMeshPartAsync", "CreateEditableImageAsync", "CreateEditableMeshAsync"];

/// The content id `v` names: a string, an asset number, a Content, or (an
/// Instance) its video content (VideoPlayer / VideoFrame:LoadAsync).
fn content_id(lua: &Lua, v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.to_str().ok()?.to_string()),
        Value::Integer(n) if *n > 0 => Some(format!("rbxassetid://{n}")),
        Value::Number(n) if *n > 0.0 && n.fract() == 0.0 => Some(format!("rbxassetid://{}", *n as u64)),
        _ => {
            if let Some(c) = opt_ud::<editable::LContent>(v) {
                return c.0.as_uri().map(str::to_string);
            }
            let r = opt_inst(v)?;
            let w = world(lua).ok()?;
            let d = w.d();
            ["VideoContent", "Video"].into_iter().find_map(|k| match d.prop(r, k) {
                Some(Variant::Content(c)) => c.as_uri().filter(|u| !u.is_empty()).map(str::to_string),
                Some(Variant::ContentId(c)) => Some(c.as_str().to_string()).filter(|u| !u.is_empty()),
                _ => None,
            })
        }
    }
}

const WRAP: &str = r#"
local pending = ...
return function(f, ofSelf, settle)
    return function(self, c, ...)
        -- (ofSelf nil: nothing to wait for. A thread that can't yield gets
        -- the inline fetch.)
        if ofSelf ~= nil and coroutine.isyieldable() then
            local what = if ofSelf then self else c
            while pending(what) do task.wait() end
        end
        if settle then
            local r = table.pack(f(self, c, ...))
            task.wait()
            return table.unpack(r, 1, r.n)
        end
        return f(self, c, ...)
    end
end
"#;

pub(crate) fn install(lua: &Lua) -> mlua::Result<()> {
    let pending = lua.create_function(|lua, v: Value| {
        Ok(content_id(lua, &v).is_some_and(|id| rustudio_core::assets::script_fetch_pending(&id)))
    })?;
    let wrap: Function = lua.load(WRAP).set_name("=rustudio.content_wait").call(pending)?;
    lua.set_named_registry_value("rs_content_wrap", wrap)?;
    for name in LOADS {
        let y = yielding(lua, raw_method(lua, name)?, Some(false), false)?;
        if let Some(mut c) = lua.app_data_mut::<Cache>() {
            c.overrides.insert(name, y);
        }
    }
    Ok(())
}

/// The method's own (Rust) function, named as Studio names it.
pub(crate) fn raw_method(lua: &Lua, name: &'static str) -> mlua::Result<Function> {
    let raw = lua.create_function(move |lua, args: MultiValue| {
        rustudio_core::bench::api_timed("call", name, || checked(lua, call_method(lua, name, args)))
    })?;
    crate::cmeta::method(lua, name, Some(raw), None, None)
}

/// `f` behind a wait for the content it loads (`Some(false)`: its first
/// argument; `Some(true)`: the instance's own video; `None`: no wait), then
/// (`settle`) a frame's yield after it as Studio's *Async members give.
pub(crate) fn yielding(lua: &Lua, f: Function, of_self: Option<bool>, settle: bool) -> mlua::Result<Function> {
    lua.named_registry_value::<Function>("rs_content_wrap")?.call((f, of_self, settle))
}
