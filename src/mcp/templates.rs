//! Reusable builders for fast game creation: lighting presets, procedural
//! animations (`KeyframeSequence`), R15/R6 character & NPC rigs, and complete
//! playable game starter templates (FPS Arena, Obby, Horror Chase, Combat
//! Arena, Tycoon). Used by MCP tools (`src/mcp/extra.rs`) and Studio UI.

use glam::Vec3;
use rbx_dom_weak::types::{
    CFrame, Color3, Color3uint8, Enum, Matrix3, Ref, UDim, UDim2, Variant, Vector3,
};
use rbx_dom_weak::{ustr, InstanceBuilder};
use rustudio_core::doc::Document;
use rustudio_engine::rig::{self, BodyColors, RigType, Scales};

/// Constructs a Roblox `CFrame` from position and XYZ Euler degrees (YXZ order).
pub fn cframe_from_pos_euler_deg(pos: [f32; 3], rot_deg: [f32; 3]) -> CFrame {
    let m = glam::Mat3::from_euler(
        glam::EulerRot::YXZ,
        rot_deg[1].to_radians(),
        rot_deg[0].to_radians(),
        rot_deg[2].to_radians(),
    );
    CFrame::new(
        Vector3::new(pos[0], pos[1], pos[2]),
        Matrix3::new(
            Vector3::new(m.x_axis.x, m.y_axis.x, m.z_axis.x),
            Vector3::new(m.x_axis.y, m.y_axis.y, m.z_axis.y),
            Vector3::new(m.x_axis.z, m.y_axis.z, m.z_axis.z),
        ),
    )
}

pub fn part_builder(
    name: &str,
    pos: [f32; 3],
    size: [f32; 3],
    rgb: [u8; 3],
    material: u32,
    anchored: bool,
) -> InstanceBuilder {
    InstanceBuilder::new("Part")
        .with_name(name)
        .with_property("Anchored", anchored)
        .with_property("Size", Vector3::new(size[0], size[1], size[2]))
        .with_property("CFrame", cframe_from_pos_euler_deg(pos, [0.0, 0.0, 0.0]))
        .with_property("Color", Color3uint8::new(rgb[0], rgb[1], rgb[2]))
        .with_property("Material", Enum::from_u32(material))
        .with_property("TopSurface", Enum::from_u32(0))
        .with_property("BottomSurface", Enum::from_u32(0))
}

/// Applies one of the built-in Roblox Lighting + PostFX presets to `doc`.
/// Returns a summary of updated Lighting properties and created effects.
pub fn apply_lighting_preset(doc: &mut Document, preset: &str) -> Result<String, String> {
    let lighting = doc
        .find_child(doc.dom.root_ref(), "Lighting")
        .ok_or("Lighting service is missing")?;

    #[derive(Clone, Copy)]
    struct Preset {
        clock_time: f32,
        brightness: f32,
        exposure: f32,
        ambient: [f32; 3],
        outdoor_ambient: [f32; 3],
        fog_end: f32,
        fog_color: [f32; 3],
        atmo_density: f32,
        atmo_haze: f32,
        atmo_glare: f32,
        atmo_color: [f32; 3],
        atmo_decay: [f32; 3],
        bloom_intensity: f32,
        bloom_size: f32,
        bloom_threshold: f32,
        cc_brightness: f32,
        cc_contrast: f32,
        cc_saturation: f32,
        cc_tint: [f32; 3],
        sun_rays: f32,
    }

    let key = preset.trim().to_ascii_lowercase();
    let p = match key.as_str() {
        "daylight" | "day" | "sunny" => Preset {
            clock_time: 14.0,
            brightness: 2.5,
            exposure: 0.1,
            ambient: [0.28, 0.30, 0.34],
            outdoor_ambient: [0.48, 0.52, 0.58],
            fog_end: 100000.0,
            fog_color: [0.75, 0.82, 0.92],
            atmo_density: 0.28,
            atmo_haze: 1.0,
            atmo_glare: 0.15,
            atmo_color: [0.78, 0.85, 0.95],
            atmo_decay: [0.42, 0.50, 0.62],
            bloom_intensity: 0.6,
            bloom_size: 20.0,
            bloom_threshold: 1.8,
            cc_brightness: 0.02,
            cc_contrast: 0.08,
            cc_saturation: 0.08,
            cc_tint: [1.0, 0.99, 0.96],
            sun_rays: 0.12,
        },
        "sunset" | "golden" | "dusk" => Preset {
            clock_time: 17.7,
            brightness: 2.2,
            exposure: 0.15,
            ambient: [0.24, 0.16, 0.18],
            outdoor_ambient: [0.48, 0.32, 0.28],
            fog_end: 8000.0,
            fog_color: [0.95, 0.55, 0.35],
            atmo_density: 0.38,
            atmo_haze: 2.2,
            atmo_glare: 0.55,
            atmo_color: [0.98, 0.62, 0.38],
            atmo_decay: [0.48, 0.22, 0.20],
            bloom_intensity: 0.9,
            bloom_size: 28.0,
            bloom_threshold: 1.4,
            cc_brightness: 0.0,
            cc_contrast: 0.15,
            cc_saturation: 0.18,
            cc_tint: [1.0, 0.90, 0.82],
            sun_rays: 0.25,
        },
        "night" | "moonlight" => Preset {
            clock_time: 1.5,
            brightness: 1.0,
            exposure: 0.0,
            ambient: [0.08, 0.10, 0.16],
            outdoor_ambient: [0.14, 0.18, 0.28],
            fog_end: 2500.0,
            fog_color: [0.06, 0.08, 0.14],
            atmo_density: 0.42,
            atmo_haze: 1.8,
            atmo_glare: 0.0,
            atmo_color: [0.12, 0.16, 0.26],
            atmo_decay: [0.04, 0.05, 0.10],
            bloom_intensity: 1.0,
            bloom_size: 24.0,
            bloom_threshold: 1.2,
            cc_brightness: -0.04,
            cc_contrast: 0.12,
            cc_saturation: -0.08,
            cc_tint: [0.85, 0.91, 1.0],
            sun_rays: 0.0,
        },
        "horror" | "dark" | "scary" => Preset {
            clock_time: 0.0,
            brightness: 0.25,
            exposure: -0.35,
            ambient: [0.02, 0.02, 0.03],
            outdoor_ambient: [0.03, 0.035, 0.045],
            fog_end: 140.0,
            fog_color: [0.02, 0.025, 0.03],
            atmo_density: 0.62,
            atmo_haze: 6.0,
            atmo_glare: 0.0,
            atmo_color: [0.05, 0.06, 0.07],
            atmo_decay: [0.01, 0.01, 0.015],
            bloom_intensity: 0.85,
            bloom_size: 18.0,
            bloom_threshold: 1.3,
            cc_brightness: -0.06,
            cc_contrast: 0.28,
            cc_saturation: -0.35,
            cc_tint: [0.88, 0.93, 0.90],
            sun_rays: 0.0,
        },
        "cyberpunk" | "neon" => Preset {
            clock_time: 22.5,
            brightness: 0.9,
            exposure: 0.1,
            ambient: [0.14, 0.06, 0.22],
            outdoor_ambient: [0.10, 0.14, 0.28],
            fog_end: 900.0,
            fog_color: [0.12, 0.04, 0.20],
            atmo_density: 0.45,
            atmo_haze: 3.5,
            atmo_glare: 0.2,
            atmo_color: [0.32, 0.10, 0.48],
            atmo_decay: [0.08, 0.04, 0.16],
            bloom_intensity: 1.45,
            bloom_size: 32.0,
            bloom_threshold: 0.9,
            cc_brightness: 0.0,
            cc_contrast: 0.22,
            cc_saturation: 0.28,
            cc_tint: [0.95, 0.90, 1.0],
            sun_rays: 0.0,
        },
        "scifi" | "sci-fi" | "space" => Preset {
            clock_time: 10.5,
            brightness: 1.8,
            exposure: 0.05,
            ambient: [0.16, 0.22, 0.28],
            outdoor_ambient: [0.22, 0.30, 0.38],
            fog_end: 4000.0,
            fog_color: [0.18, 0.28, 0.38],
            atmo_density: 0.32,
            atmo_haze: 1.5,
            atmo_glare: 0.25,
            atmo_color: [0.40, 0.68, 0.92],
            atmo_decay: [0.12, 0.22, 0.36],
            bloom_intensity: 1.1,
            bloom_size: 24.0,
            bloom_threshold: 1.25,
            cc_brightness: 0.0,
            cc_contrast: 0.14,
            cc_saturation: 0.05,
            cc_tint: [0.92, 0.97, 1.0],
            sun_rays: 0.15,
        },
        "studio" | "bright" | "default" => Preset {
            clock_time: 14.5,
            brightness: 3.0,
            exposure: 0.0,
            ambient: [0.35, 0.35, 0.35],
            outdoor_ambient: [0.55, 0.55, 0.55],
            fog_end: 100000.0,
            fog_color: [0.75, 0.75, 0.75],
            atmo_density: 0.2,
            atmo_haze: 0.5,
            atmo_glare: 0.0,
            atmo_color: [0.85, 0.88, 0.92],
            atmo_decay: [0.55, 0.60, 0.68],
            bloom_intensity: 0.4,
            bloom_size: 16.0,
            bloom_threshold: 2.0,
            cc_brightness: 0.0,
            cc_contrast: 0.0,
            cc_saturation: 0.0,
            cc_tint: [1.0, 1.0, 1.0],
            sun_rays: 0.05,
        },
        other => {
            return Err(format!(
                "Unknown lighting preset {other:?}. Choose one of: daylight, sunset, night, horror, cyberpunk, scifi, studio"
            ));
        }
    };

    let c3 = |a: [f32; 3]| Variant::Color3(Color3::new(a[0], a[1], a[2]));
    doc.set_props(
        format!("Apply Lighting preset ({key})"),
        vec![
            (lighting, ustr("ClockTime"), Variant::Float32(p.clock_time)),
            (lighting, ustr("Brightness"), Variant::Float32(p.brightness)),
            (lighting, ustr("ExposureCompensation"), Variant::Float32(p.exposure)),
            (lighting, ustr("Ambient"), c3(p.ambient)),
            (lighting, ustr("OutdoorAmbient"), c3(p.outdoor_ambient)),
            (lighting, ustr("FogEnd"), Variant::Float32(p.fog_end)),
            (lighting, ustr("FogColor"), c3(p.fog_color)),
            (lighting, ustr("GlobalShadows"), Variant::Bool(true)),
        ],
        None,
    );

    // Upsert Atmosphere, BloomEffect, ColorCorrectionEffect, SunRaysEffect under Lighting.
    upsert_lighting_child(
        doc,
        lighting,
        "Atmosphere",
        "Atmosphere",
        vec![
            ("Density", Variant::Float32(p.atmo_density)),
            ("Haze", Variant::Float32(p.atmo_haze)),
            ("Glare", Variant::Float32(p.atmo_glare)),
            ("Color", c3(p.atmo_color)),
            ("Decay", c3(p.atmo_decay)),
        ],
    );
    upsert_lighting_child(
        doc,
        lighting,
        "BloomEffect",
        "Bloom",
        vec![
            ("Enabled", Variant::Bool(true)),
            ("Intensity", Variant::Float32(p.bloom_intensity)),
            ("Size", Variant::Float32(p.bloom_size)),
            ("Threshold", Variant::Float32(p.bloom_threshold)),
        ],
    );
    upsert_lighting_child(
        doc,
        lighting,
        "ColorCorrectionEffect",
        "ColorCorrection",
        vec![
            ("Enabled", Variant::Bool(true)),
            ("Brightness", Variant::Float32(p.cc_brightness)),
            ("Contrast", Variant::Float32(p.cc_contrast)),
            ("Saturation", Variant::Float32(p.cc_saturation)),
            ("TintColor", c3(p.cc_tint)),
        ],
    );
    upsert_lighting_child(
        doc,
        lighting,
        "SunRaysEffect",
        "SunRays",
        vec![
            ("Enabled", Variant::Bool(p.sun_rays > 0.0)),
            ("Intensity", Variant::Float32(p.sun_rays)),
            ("Spread", Variant::Float32(0.8)),
        ],
    );

    Ok(format!("Applied {key:?} lighting preset (Lighting + Atmosphere + Bloom + ColorCorrection + SunRays)"))
}

fn upsert_lighting_child(
    doc: &mut Document,
    lighting: Ref,
    class: &str,
    name: &str,
    props: Vec<(&str, Variant)>,
) {
    if let Some(existing) = doc.find_child(lighting, class) {
        let updates = props.into_iter().map(|(k, v)| (existing, ustr(k), v)).collect();
        doc.set_props(format!("Update {name}"), updates, None);
    } else {
        let mut b = InstanceBuilder::new(class).with_name(name);
        for (k, v) in props {
            b.add_property(ustr(k), v);
        }
        doc.insert(lighting, b);
    }
}

/// Spawns a ready-to-animate R15 or R6 character rig (with optional patrol/chase AI script).
pub fn spawn_rig(
    doc: &mut Document,
    parent: Ref,
    name: &str,
    rig_type: RigType,
    pos: [f32; 3],
    anchored: bool,
    colors: Option<BodyColors>,
    ai_mode: &str,
) -> Ref {
    let mut builder = rig::build_default(
        name,
        rig_type,
        &colors.unwrap_or_default(),
        &Scales::default(),
        InstanceBuilder::new("Humanoid")
            .with_name("Humanoid")
            .with_property("WalkSpeed", 16.0f32)
            .with_property("JumpPower", 50.0f32)
            .with_property("MaxHealth", 100.0f32)
            .with_property("Health", 100.0f32),
    );
    match ai_mode {
        "patrol" => {
            builder.add_child(
                InstanceBuilder::new("Script")
                    .with_name("PatrolAI")
                    .with_property("Source", Variant::String(PATROL_AI_SCRIPT.into())),
            );
        }
        "chase" => {
            builder.add_child(
                InstanceBuilder::new("Script")
                    .with_name("ChaseAI")
                    .with_property("Source", Variant::String(CHASE_AI_SCRIPT.into())),
            );
        }
        _ => {}
    }

    let model_ref = doc.insert(parent, builder);

    // Offset all rig parts to `pos` (standing on pos.y) and apply anchored setting on HRP.
    let offset = Vec3::new(pos[0], pos[1] + 3.0, pos[2]);
    let subtree = doc.subtree(model_ref);
    let mut updates = Vec::new();
    for r in subtree {
        let Some(inst) = doc.dom.get_by_ref(r) else { continue };
        if rustudio_core::doc::is_a(inst.class.as_str(), "BasePart") {
            if let Some(Variant::CFrame(mut cf)) = inst.properties.get(&ustr("CFrame")).cloned() {
                cf.position = Vector3::new(
                    cf.position.x + offset.x,
                    cf.position.y + offset.y,
                    cf.position.z + offset.z,
                );
                updates.push((r, ustr("CFrame"), Variant::CFrame(cf)));
            }
            if inst.name == "HumanoidRootPart" {
                updates.push((r, ustr("Anchored"), Variant::Bool(anchored)));
            }
        }
    }
    if !updates.is_empty() {
        doc.set_props("Position spawned rig", updates, None);
    }
    model_ref
}

/// Builds a preset `KeyframeSequence` (`idle`, `walk`, `run`, `jump`, `wave`, `punch`, `slash`, `dance`)
/// ready for `Animator:LoadAnimation()` or editing in the Animation Editor.
pub fn build_preset_animation(name: &str, preset: &str, rig: RigType) -> Result<InstanceBuilder, String> {
    let key = preset.trim().to_ascii_lowercase();
    let r6 = rig == RigType::R6;

    // Helper: keyframe with a list of (part_name, pos_offset, rot_deg_xyz) poses.
    let make_kf = |time: f32, poses: &[(&str, [f32; 3], [f32; 3])], markers: &[(&str, &str)]| -> InstanceBuilder {
        let mut root_pose = InstanceBuilder::new("Pose")
            .with_name("HumanoidRootPart")
            .with_property("CFrame", CFrame::identity())
            .with_property("Weight", 1.0f32)
            .with_property("EasingStyle", Enum::from_u32(0))
            .with_property("EasingDirection", Enum::from_u32(0));
        for &(part, off, rot) in poses {
            root_pose.add_child(
                InstanceBuilder::new("Pose")
                    .with_name(part)
                    .with_property("CFrame", cframe_from_pos_euler_deg(off, rot))
                    .with_property("Weight", 1.0f32)
                    .with_property("EasingStyle", Enum::from_u32(2)) // Cubic
                    .with_property("EasingDirection", Enum::from_u32(2)), // InOut
            );
        }
        let mut kf = InstanceBuilder::new("Keyframe")
            .with_name(format!("Keyframe_{:.2}", time))
            .with_property("Time", time)
            .with_child(root_pose);
        for &(mname, mval) in markers {
            kf.add_child(
                InstanceBuilder::new("KeyframeMarker")
                    .with_name(mname)
                    .with_property("Value", Variant::String(mval.to_string())),
            );
        }
        kf
    };

    let (looped, priority, frames): (bool, u32, Vec<InstanceBuilder>) = match key.as_str() {
        "idle" => {
            let torso = if r6 { "Torso" } else { "UpperTorso" };
            let r_arm = if r6 { "Right Arm" } else { "RightUpperArm" };
            let l_arm = if r6 { "Left Arm" } else { "LeftUpperArm" };
            (true, 1, vec![
                make_kf(0.0, &[(torso, [0.0, 0.0, 0.0], [0.0, 0.0, 0.0]), (r_arm, [0.0, 0.0, 0.0], [0.0, 0.0, -4.0]), (l_arm, [0.0, 0.0, 0.0], [0.0, 0.0, 4.0])], &[]),
                make_kf(1.0, &[(torso, [0.0, 0.08, 0.0], [2.0, 0.0, 0.0]), (r_arm, [0.0, 0.0, 0.0], [3.0, 0.0, -7.0]), (l_arm, [0.0, 0.0, 0.0], [3.0, 0.0, 7.0])], &[]),
                make_kf(2.0, &[(torso, [0.0, 0.0, 0.0], [0.0, 0.0, 0.0]), (r_arm, [0.0, 0.0, 0.0], [0.0, 0.0, -4.0]), (l_arm, [0.0, 0.0, 0.0], [0.0, 0.0, 4.0])], &[]),
            ])
        }
        "walk" => {
            let r_arm = if r6 { "Right Arm" } else { "RightUpperArm" };
            let l_arm = if r6 { "Left Arm" } else { "LeftUpperArm" };
            let r_leg = if r6 { "Right Leg" } else { "RightUpperLeg" };
            let l_leg = if r6 { "Left Leg" } else { "LeftUpperLeg" };
            (true, 2, vec![
                make_kf(0.0, &[(r_arm, [0.0, 0.0, 0.0], [30.0, 0.0, 0.0]), (l_arm, [0.0, 0.0, 0.0], [-30.0, 0.0, 0.0]), (r_leg, [0.0, 0.0, 0.0], [-35.0, 0.0, 0.0]), (l_leg, [0.0, 0.0, 0.0], [35.0, 0.0, 0.0])], &[("Footstep", "Left")]),
                make_kf(0.35, &[(r_arm, [0.0, 0.0, 0.0], [-30.0, 0.0, 0.0]), (l_arm, [0.0, 0.0, 0.0], [30.0, 0.0, 0.0]), (r_leg, [0.0, 0.0, 0.0], [35.0, 0.0, 0.0]), (l_leg, [0.0, 0.0, 0.0], [-35.0, 0.0, 0.0])], &[("Footstep", "Right")]),
                make_kf(0.7, &[(r_arm, [0.0, 0.0, 0.0], [30.0, 0.0, 0.0]), (l_arm, [0.0, 0.0, 0.0], [-30.0, 0.0, 0.0]), (r_leg, [0.0, 0.0, 0.0], [-35.0, 0.0, 0.0]), (l_leg, [0.0, 0.0, 0.0], [35.0, 0.0, 0.0])], &[]),
            ])
        }
        "run" => {
            let torso = if r6 { "Torso" } else { "UpperTorso" };
            let r_arm = if r6 { "Right Arm" } else { "RightUpperArm" };
            let l_arm = if r6 { "Left Arm" } else { "LeftUpperArm" };
            let r_leg = if r6 { "Right Leg" } else { "RightUpperLeg" };
            let l_leg = if r6 { "Left Leg" } else { "LeftUpperLeg" };
            (true, 2, vec![
                make_kf(0.0, &[(torso, [0.0, 0.0, 0.0], [-14.0, 8.0, 0.0]), (r_arm, [0.0, 0.0, 0.0], [58.0, 0.0, 0.0]), (l_arm, [0.0, 0.0, 0.0], [-52.0, 0.0, 0.0]), (r_leg, [0.0, 0.0, 0.0], [-55.0, 0.0, 0.0]), (l_leg, [0.0, 0.0, 0.0], [55.0, 0.0, 0.0])], &[("Footstep", "Left")]),
                make_kf(0.24, &[(torso, [0.0, 0.15, 0.0], [-14.0, -8.0, 0.0]), (r_arm, [0.0, 0.0, 0.0], [-52.0, 0.0, 0.0]), (l_arm, [0.0, 0.0, 0.0], [58.0, 0.0, 0.0]), (r_leg, [0.0, 0.0, 0.0], [55.0, 0.0, 0.0]), (l_leg, [0.0, 0.0, 0.0], [-55.0, 0.0, 0.0])], &[("Footstep", "Right")]),
                make_kf(0.48, &[(torso, [0.0, 0.0, 0.0], [-14.0, 8.0, 0.0]), (r_arm, [0.0, 0.0, 0.0], [58.0, 0.0, 0.0]), (l_arm, [0.0, 0.0, 0.0], [-52.0, 0.0, 0.0]), (r_leg, [0.0, 0.0, 0.0], [-55.0, 0.0, 0.0]), (l_leg, [0.0, 0.0, 0.0], [55.0, 0.0, 0.0])], &[]),
            ])
        }
        "jump" => {
            let r_arm = if r6 { "Right Arm" } else { "RightUpperArm" };
            let l_arm = if r6 { "Left Arm" } else { "LeftUpperArm" };
            let r_leg = if r6 { "Right Leg" } else { "RightUpperLeg" };
            let l_leg = if r6 { "Left Leg" } else { "LeftUpperLeg" };
            (false, 3, vec![
                make_kf(0.0, &[(r_arm, [0.0, 0.0, 0.0], [-20.0, 0.0, 0.0]), (l_arm, [0.0, 0.0, 0.0], [-20.0, 0.0, 0.0]), (r_leg, [0.0, 0.0, 0.0], [25.0, 0.0, 0.0]), (l_leg, [0.0, 0.0, 0.0], [25.0, 0.0, 0.0])], &[]),
                make_kf(0.25, &[(r_arm, [0.0, 0.0, 0.0], [140.0, 0.0, -18.0]), (l_arm, [0.0, 0.0, 0.0], [140.0, 0.0, 18.0]), (r_leg, [0.0, 0.0, 0.0], [-25.0, 0.0, 0.0]), (l_leg, [0.0, 0.0, 0.0], [18.0, 0.0, 0.0])], &[]),
                make_kf(0.65, &[(r_arm, [0.0, 0.0, 0.0], [10.0, 0.0, 0.0]), (l_arm, [0.0, 0.0, 0.0], [10.0, 0.0, 0.0]), (r_leg, [0.0, 0.0, 0.0], [0.0, 0.0, 0.0]), (l_leg, [0.0, 0.0, 0.0], [0.0, 0.0, 0.0])], &[]),
            ])
        }
        "wave" => {
            let r_arm = if r6 { "Right Arm" } else { "RightUpperArm" };
            let r_low = if r6 { "Head" } else { "RightLowerArm" };
            (true, 3, vec![
                make_kf(0.0, &[(r_arm, [0.0, 0.0, 0.0], [150.0, 0.0, -25.0]), (r_low, [0.0, 0.0, 0.0], [0.0, 0.0, -20.0])], &[]),
                make_kf(0.25, &[(r_arm, [0.0, 0.0, 0.0], [150.0, 0.0, -5.0]), (r_low, [0.0, 0.0, 0.0], [0.0, 0.0, 22.0])], &[]),
                make_kf(0.5, &[(r_arm, [0.0, 0.0, 0.0], [150.0, 0.0, -25.0]), (r_low, [0.0, 0.0, 0.0], [0.0, 0.0, -20.0])], &[]),
            ])
        }
        "punch" => {
            let torso = if r6 { "Torso" } else { "UpperTorso" };
            let r_arm = if r6 { "Right Arm" } else { "RightUpperArm" };
            let l_arm = if r6 { "Left Arm" } else { "LeftUpperArm" };
            (false, 3, vec![
                make_kf(0.0, &[(torso, [0.0, 0.0, 0.0], [0.0, -25.0, 0.0]), (r_arm, [0.0, 0.0, 0.0], [-30.0, 0.0, 0.0]), (l_arm, [0.0, 0.0, 0.0], [50.0, 0.0, 0.0])], &[]),
                make_kf(0.14, &[(torso, [0.0, 0.0, 0.0], [-8.0, 32.0, 0.0]), (r_arm, [0.0, 0.0, 0.0], [95.0, -10.0, 0.0]), (l_arm, [0.0, 0.0, 0.0], [-20.0, 0.0, 0.0])], &[("Hit", "RightFist")]),
                make_kf(0.38, &[(torso, [0.0, 0.0, 0.0], [0.0, 0.0, 0.0]), (r_arm, [0.0, 0.0, 0.0], [0.0, 0.0, 0.0]), (l_arm, [0.0, 0.0, 0.0], [0.0, 0.0, 0.0])], &[]),
            ])
        }
        "slash" => {
            let torso = if r6 { "Torso" } else { "UpperTorso" };
            let r_arm = if r6 { "Right Arm" } else { "RightUpperArm" };
            (false, 3, vec![
                make_kf(0.0, &[(torso, [0.0, 0.0, 0.0], [0.0, -40.0, 0.0]), (r_arm, [0.0, 0.0, 0.0], [135.0, -30.0, -35.0])], &[("SwingStart", "1")]),
                make_kf(0.16, &[(torso, [0.0, 0.0, 0.0], [-10.0, 42.0, 0.0]), (r_arm, [0.0, 0.0, 0.0], [25.0, 45.0, 25.0])], &[("Hit", "Blade")]),
                make_kf(0.45, &[(torso, [0.0, 0.0, 0.0], [0.0, 0.0, 0.0]), (r_arm, [0.0, 0.0, 0.0], [0.0, 0.0, 0.0])], &[]),
            ])
        }
        "dance" => {
            let torso = if r6 { "Torso" } else { "UpperTorso" };
            let r_arm = if r6 { "Right Arm" } else { "RightUpperArm" };
            let l_arm = if r6 { "Left Arm" } else { "LeftUpperArm" };
            let r_leg = if r6 { "Right Leg" } else { "RightUpperLeg" };
            let l_leg = if r6 { "Left Leg" } else { "LeftUpperLeg" };
            (true, 3, vec![
                make_kf(0.0, &[(torso, [0.0, 0.0, 0.0], [0.0, 15.0, 12.0]), (r_arm, [0.0, 0.0, 0.0], [115.0, 0.0, -30.0]), (l_arm, [0.0, 0.0, 0.0], [45.0, 0.0, 25.0]), (r_leg, [0.0, 0.0, 0.0], [20.0, 0.0, -8.0]), (l_leg, [0.0, 0.0, 0.0], [-10.0, 0.0, 8.0])], &[]),
                make_kf(0.4, &[(torso, [0.0, 0.2, 0.0], [0.0, -15.0, -12.0]), (r_arm, [0.0, 0.0, 0.0], [45.0, 0.0, -25.0]), (l_arm, [0.0, 0.0, 0.0], [115.0, 0.0, 30.0]), (r_leg, [0.0, 0.0, 0.0], [-10.0, 0.0, -8.0]), (l_leg, [0.0, 0.0, 0.0], [20.0, 0.0, 8.0])], &[]),
                make_kf(0.8, &[(torso, [0.0, 0.0, 0.0], [0.0, 15.0, 12.0]), (r_arm, [0.0, 0.0, 0.0], [115.0, 0.0, -30.0]), (l_arm, [0.0, 0.0, 0.0], [45.0, 0.0, 25.0]), (r_leg, [0.0, 0.0, 0.0], [20.0, 0.0, -8.0]), (l_leg, [0.0, 0.0, 0.0], [-10.0, 0.0, 8.0])], &[]),
            ])
        }
        other => {
            return Err(format!(
                "Unknown animation preset {other:?}. Choose one of: idle, walk, run, jump, wave, punch, slash, dance"
            ));
        }
    };

    let mut seq = InstanceBuilder::new("KeyframeSequence")
        .with_name(name)
        .with_property("Loop", Variant::Bool(looped))
        .with_property("Priority", Enum::from_u32(priority));
    for kf in frames {
        seq.add_child(kf);
    }
    Ok(seq)
}

/// Builds a complete playable starter game into `doc` (`fps_arena`, `obby`, `horror_chase`, `combat_arena`, `tycoon`).
pub fn spawn_game_template(doc: &mut Document, template: &str, clear_workspace: bool) -> Result<String, String> {
    let ws = doc.workspace().ok_or("Workspace is missing")?;
    let sss = doc
        .find_child(doc.dom.root_ref(), "ServerScriptService")
        .ok_or("ServerScriptService is missing")?;
    let sgui = doc
        .find_child(doc.dom.root_ref(), "StarterGui")
        .ok_or("StarterGui is missing")?;
    let rs = doc
        .find_child(doc.dom.root_ref(), "ReplicatedStorage")
        .ok_or("ReplicatedStorage is missing")?;

    if clear_workspace {
        let to_remove: Vec<Ref> = doc
            .dom
            .get_by_ref(ws)
            .map(|w| {
                w.children()
                    .iter()
                    .copied()
                    .filter(|&c| {
                        doc.class_of(c)
                            .is_some_and(|cl| cl.as_str() != "Camera" && cl.as_str() != "Terrain")
                    })
                    .collect()
            })
            .unwrap_or_default();
        if !to_remove.is_empty() {
            doc.remove(&to_remove);
        }
    }

    let key = template.trim().to_ascii_lowercase();
    match key.as_str() {
        "obby" => {
            apply_lighting_preset(doc, "daylight")?;
            let mut map = InstanceBuilder::new("Folder").with_name("ObbyCourse");
            map.add_child(
                InstanceBuilder::new("SpawnLocation")
                    .with_name("Stage0_Start")
                    .with_property("Anchored", true)
                    .with_property("Size", Vector3::new(16.0, 2.0, 16.0))
                    .with_property("CFrame", cframe_from_pos_euler_deg([0.0, 1.0, 0.0], [0.0, 0.0, 0.0]))
                    .with_property("Color", Color3uint8::new(66, 179, 98)),
            );
            let colors = [[235, 87, 87], [242, 153, 74], [242, 201, 76], [39, 174, 96], [47, 128, 237], [155, 81, 224]];
            for i in 1..=14 {
                let z = -(i as f32) * 14.0;
                let y = 2.0 + (i as f32) * 1.4;
                let x = ((i as f32) * 0.9).sin() * 8.0;
                let c = colors[i % colors.len()];
                let mut plat = part_builder(&format!("Platform_{i}"), [x, y, z], [8.0, 1.5, 8.0], c, 256, true);
                if i % 4 == 0 {
                    plat.add_child(
                        InstanceBuilder::new("Part")
                            .with_name("KillBrick")
                            .with_property("Anchored", true)
                            .with_property("Size", Vector3::new(8.0, 0.6, 1.8))
                            .with_property("CFrame", cframe_from_pos_euler_deg([x, y + 1.05, z], [0.0, 0.0, 0.0]))
                            .with_property("Color", Color3uint8::new(255, 40, 40))
                            .with_property("Material", Enum::from_u32(288)), // Neon
                    );
                }
                map.add_child(plat);
                if i == 5 || i == 10 || i == 14 {
                    map.add_child(
                        part_builder(&format!("Checkpoint_{i}"), [x, y + 0.85, z], [4.0, 0.3, 4.0], [255, 220, 60], 288, true),
                    );
                }
            }
            doc.insert(ws, map);
            doc.insert(
                sss,
                InstanceBuilder::new("Script")
                    .with_name("ObbyServer")
                    .with_property("Source", Variant::String(OBBY_SERVER_SCRIPT.into())),
            );
            doc.insert(sgui, make_hud_gui("ObbyHUD", "STAGE 1 / 14", "Jump across the platforms and avoid red Neon bricks!"));
            Ok("Spawned playable 'obby' template (14 stages, checkpoints, kill bricks, leaderstats, HUD).".into())
        }
        "fps_arena" | "fps" => {
            apply_lighting_preset(doc, "scifi")?;
            let mut arena = InstanceBuilder::new("Folder").with_name("FPSArena");
            arena.add_child(part_builder("Floor", [0.0, -1.0, 0.0], [160.0, 2.0, 160.0], [54, 62, 72], 816, true));
            for (n, p, s) in [
                ("WallN", [0.0, 12.0, -80.0], [160.0, 26.0, 4.0]),
                ("WallS", [0.0, 12.0, 80.0], [160.0, 26.0, 4.0]),
                ("WallE", [80.0, 12.0, 0.0], [4.0, 26.0, 160.0]),
                ("WallW", [-80.0, 12.0, 0.0], [4.0, 26.0, 160.0]),
            ] {
                arena.add_child(part_builder(n, p, s, [42, 48, 56], 816, true));
            }
            for (i, &(x, z, w, h, d)) in [
                (-28.0, -24.0, 14.0, 8.0, 6.0),
                (28.0, -24.0, 14.0, 8.0, 6.0),
                (-28.0, 24.0, 14.0, 8.0, 6.0),
                (28.0, 24.0, 14.0, 8.0, 6.0),
                (0.0, 0.0, 18.0, 10.0, 18.0),
                (-50.0, 0.0, 8.0, 6.0, 26.0),
                (50.0, 0.0, 8.0, 6.0, 26.0),
            ]
            .iter()
            .enumerate()
            {
                arena.add_child(part_builder(&format!("Cover_{i}"), [x, h * 0.5, z], [w, h, d], [82, 92, 108], 1088, true));
                arena.add_child(part_builder(&format!("NeonTrim_{i}"), [x, h + 0.3, z], [w + 0.2, 0.4, d + 0.2], [0, 210, 255], 288, true));
            }
            arena.add_child(
                InstanceBuilder::new("SpawnLocation")
                    .with_name("PlayerSpawn")
                    .with_property("Anchored", true)
                    .with_property("Size", Vector3::new(10.0, 1.0, 10.0))
                    .with_property("CFrame", cframe_from_pos_euler_deg([0.0, 0.5, 58.0], [0.0, 0.0, 0.0]))
                    .with_property("Color", Color3uint8::new(0, 180, 255)),
            );
            doc.insert(ws, arena);
            // Spawn 4 target bots across the arena.
            for (i, pos) in [[-22.0, 0.0, -45.0], [22.0, 0.0, -45.0], [-42.0, 0.0, -10.0], [42.0, 0.0, -10.0]].iter().enumerate() {
                spawn_rig(doc, ws, &format!("Bot_{}", i + 1), RigType::R15, *pos, false, None, "patrol");
            }
            doc.insert(rs, InstanceBuilder::new("RemoteEvent").with_name("FireShot"));
            doc.insert(
                sss,
                InstanceBuilder::new("Script")
                    .with_name("FPSServer")
                    .with_property("Source", Variant::String(FPS_SERVER_SCRIPT.into())),
            );
            let mut hud = make_hud_gui("FPSHUD", "AMMO: 30 / 30  |  KILLS: 0", "Click to shoot bots  |  [R] Reload");
            hud.add_child(
                InstanceBuilder::new("Frame")
                    .with_name("Crosshair")
                    .with_property("Size", UDim2::new(UDim::new(0.0, 8), UDim::new(0.0, 8)))
                    .with_property("Position", UDim2::new(UDim::new(0.5, -4), UDim::new(0.5, -4)))
                    .with_property("BackgroundColor3", Color3::new(0.0, 1.0, 0.85))
                    .with_property("BorderSizePixel", 0i32),
            );
            hud.add_child(
                InstanceBuilder::new("LocalScript")
                    .with_name("FPSClient")
                    .with_property("Source", Variant::String(FPS_CLIENT_SCRIPT.into())),
            );
            doc.insert(sgui, hud);
            Ok("Spawned playable 'fps_arena' template (sci-fi cover arena, 4 patrolling bots, raycast blaster, crosshair HUD, kills/reload).".into())
        }
        "horror_chase" | "horror" => {
            apply_lighting_preset(doc, "horror")?;
            let mut map = InstanceBuilder::new("Folder").with_name("HorrorFacility");
            map.add_child(part_builder("Floor", [0.0, -1.0, 0.0], [120.0, 2.0, 120.0], [32, 34, 36], 816, true));
            map.add_child(part_builder("Ceiling", [0.0, 15.0, 0.0], [120.0, 2.0, 120.0], [22, 24, 26], 816, true));
            for (n, p, s) in [
                ("WallN", [0.0, 7.0, -60.0], [120.0, 16.0, 2.0]),
                ("WallS", [0.0, 7.0, 60.0], [120.0, 16.0, 2.0]),
                ("WallE", [60.0, 7.0, 0.0], [2.0, 16.0, 120.0]),
                ("WallW", [-60.0, 7.0, 0.0], [2.0, 16.0, 120.0]),
                ("HallWall1", [-18.0, 7.0, -12.0], [2.0, 16.0, 68.0]),
                ("HallWall2", [18.0, 7.0, 12.0], [2.0, 16.0, 68.0]),
            ] {
                map.add_child(part_builder(n, p, s, [44, 46, 48], 816, true));
            }
            for (i, z) in [-40.0, 0.0, 40.0].iter().enumerate() {
                let mut lamp = part_builder(&format!("CeilingLamp_{i}"), [0.0, 13.7, *z], [4.0, 0.4, 2.0], [255, 210, 160], 288, true);
                lamp.add_child(
                    InstanceBuilder::new("PointLight")
                        .with_name("LampLight")
                        .with_property("Brightness", 1.8f32)
                        .with_property("Range", 34.0f32)
                        .with_property("Color", Color3::new(1.0, 0.85, 0.65)),
                );
                map.add_child(lamp);
            }
            map.add_child(
                InstanceBuilder::new("SpawnLocation")
                    .with_name("PlayerSpawn")
                    .with_property("Anchored", true)
                    .with_property("Size", Vector3::new(8.0, 1.0, 8.0))
                    .with_property("CFrame", cframe_from_pos_euler_deg([0.0, 0.5, 48.0], [0.0, 0.0, 0.0]))
                    .with_property("Color", Color3uint8::new(50, 55, 60)),
            );
            doc.insert(ws, map);
            let stalker_colors = BodyColors {
                head: [215, 200, 180],
                torso: [180, 165, 145],
                left_arm: [175, 160, 140],
                right_arm: [175, 160, 140],
                left_leg: [165, 150, 135],
                right_leg: [165, 150, 135],
            };
            spawn_rig(doc, ws, "Stalker", RigType::R15, [0.0, 0.0, -45.0], false, Some(stalker_colors), "chase");
            doc.insert(sgui, make_hud_gui("HorrorHUD", "SURVIVE THE FACILITY", "Stay moving — the Stalker is hunting you."));
            Ok("Spawned playable 'horror_chase' template (dark corridor facility, flickering-style lamps, Stalker chase NPC, horror lighting + HUD).".into())
        }
        "combat_arena" | "combat" => {
            apply_lighting_preset(doc, "cyberpunk")?;
            let mut arena = InstanceBuilder::new("Folder").with_name("CombatArena");
            arena.add_child(part_builder("ArenaFloor", [0.0, -1.0, 0.0], [110.0, 2.0, 110.0], [28, 24, 42], 1280, true));
            arena.add_child(part_builder("CenterRing", [0.0, 0.05, 0.0], [32.0, 0.2, 32.0], [255, 45, 175], 288, true));
            for (i, &(x, z)) in [(-38.0, -38.0), (38.0, -38.0), (-38.0, 38.0), (38.0, 38.0)].iter().enumerate() {
                arena.add_child(part_builder(&format!("Pillar_{i}"), [x, 12.0, z], [6.0, 24.0, 6.0], [42, 36, 66], 1088, true));
                arena.add_child(part_builder(&format!("PillarGlow_{i}"), [x, 24.5, z], [7.0, 1.0, 7.0], [0, 240, 255], 288, true));
            }
            arena.add_child(
                InstanceBuilder::new("SpawnLocation")
                    .with_name("FighterSpawn")
                    .with_property("Anchored", true)
                    .with_property("Size", Vector3::new(10.0, 1.0, 10.0))
                    .with_property("CFrame", cframe_from_pos_euler_deg([0.0, 0.5, 32.0], [0.0, 0.0, 0.0]))
                    .with_property("Color", Color3uint8::new(120, 60, 220)),
            );
            doc.insert(ws, arena);
            spawn_rig(doc, ws, "TrainingDummy", RigType::R15, [0.0, 0.0, -14.0], false, None, "none");
            spawn_rig(doc, ws, "ArenaBoss", RigType::R15, [0.0, 0.0, -34.0], false, None, "chase");
            // Add a Slash animation ready to use under ReplicatedStorage
            if let Ok(slash) = build_preset_animation("SlashAnim", "slash", RigType::R15) {
                doc.insert(rs, slash);
            }
            doc.insert(rs, InstanceBuilder::new("RemoteEvent").with_name("MeleeAttack"));
            doc.insert(
                sss,
                InstanceBuilder::new("Script")
                    .with_name("CombatServer")
                    .with_property("Source", Variant::String(COMBAT_SERVER_SCRIPT.into())),
            );
            let mut hud = make_hud_gui("CombatHUD", "CYBER ARENA", "Click to Slash  |  [Q] Dash");
            hud.add_child(
                InstanceBuilder::new("LocalScript")
                    .with_name("CombatClient")
                    .with_property("Source", Variant::String(COMBAT_CLIENT_SCRIPT.into())),
            );
            doc.insert(sgui, hud);
            Ok("Spawned playable 'combat_arena' template (neon arena, TrainingDummy + ArenaBoss, SlashAnim, MeleeAttack + Dash scripts, HUD).".into())
        }
        "tycoon" => {
            apply_lighting_preset(doc, "sunset")?;
            let mut tycoon = InstanceBuilder::new("Folder").with_name("TycoonPlot");
            tycoon.add_child(part_builder("Baseplate", [0.0, -1.0, 0.0], [180.0, 2.0, 180.0], [74, 148, 78], 1280, true));
            tycoon.add_child(part_builder("PlotFloor", [0.0, 0.25, 0.0], [64.0, 0.5, 64.0], [185, 190, 196], 816, true));
            tycoon.add_child(part_builder("Conveyor", [0.0, 1.0, -10.0], [6.0, 1.0, 32.0], [45, 48, 54], 1088, true));
            tycoon.add_child(part_builder("Dropper", [0.0, 6.0, -24.0], [5.0, 4.0, 5.0], [235, 168, 45], 1088, true));
            tycoon.add_child(part_builder("Collector", [0.0, 1.8, 7.0], [8.0, 2.6, 4.0], [52, 211, 153], 288, true));
            tycoon.add_child(part_builder("BuyUpgraderButton", [14.0, 0.8, 12.0], [6.0, 0.6, 6.0], [56, 189, 248], 288, true));
            tycoon.add_child(
                InstanceBuilder::new("SpawnLocation")
                    .with_name("PlotSpawn")
                    .with_property("Anchored", true)
                    .with_property("Size", Vector3::new(10.0, 1.0, 10.0))
                    .with_property("CFrame", cframe_from_pos_euler_deg([0.0, 1.0, 24.0], [0.0, 0.0, 0.0]))
                    .with_property("Color", Color3uint8::new(240, 190, 50)),
            );
            doc.insert(ws, tycoon);
            doc.insert(
                sss,
                InstanceBuilder::new("Script")
                    .with_name("TycoonServer")
                    .with_property("Source", Variant::String(TYCOON_SERVER_SCRIPT.into())),
            );
            doc.insert(sgui, make_hud_gui("TycoonHUD", "CASH: $0", "Step on the blue pad ($50) to upgrade your dropper!"));
            Ok("Spawned playable 'tycoon' template (dropper, conveyor, collector, upgrade pad, Cash leaderstats, HUD).".into())
        }
        other => Err(format!(
            "Unknown game template {other:?}. Choose one of: fps_arena, obby, horror_chase, combat_arena, tycoon"
        )),
    }
}

fn make_hud_gui(name: &str, title: &str, subtitle: &str) -> InstanceBuilder {
    let mut panel = InstanceBuilder::new("Frame")
        .with_name("TopBanner")
        .with_property("Size", UDim2::new(UDim::new(0.0, 380), UDim::new(0.0, 64)))
        .with_property("Position", UDim2::new(UDim::new(0.5, -190), UDim::new(0.0, 14)))
        .with_property("BackgroundColor3", Color3::new(0.08, 0.09, 0.13))
        .with_property("BackgroundTransparency", 0.2f32)
        .with_property("BorderSizePixel", 0i32);
    panel.add_child(InstanceBuilder::new("UICorner").with_name("Corner").with_property("CornerRadius", UDim::new(0.0, 10)));
    panel.add_child(
        InstanceBuilder::new("TextLabel")
            .with_name("Title")
            .with_property("Size", UDim2::new(UDim::new(1.0, -16), UDim::new(0.0, 30)))
            .with_property("Position", UDim2::new(UDim::new(0.0, 8), UDim::new(0.0, 6)))
            .with_property("BackgroundTransparency", 1.0f32)
            .with_property("Text", Variant::String(title.into()))
            .with_property("TextColor3", Color3::new(1.0, 1.0, 1.0))
            .with_property("TextSize", 20.0f32),
    );
    panel.add_child(
        InstanceBuilder::new("TextLabel")
            .with_name("Subtitle")
            .with_property("Size", UDim2::new(UDim::new(1.0, -16), UDim::new(0.0, 22)))
            .with_property("Position", UDim2::new(UDim::new(0.0, 8), UDim::new(0.0, 36)))
            .with_property("BackgroundTransparency", 1.0f32)
            .with_property("Text", Variant::String(subtitle.into()))
            .with_property("TextColor3", Color3::new(0.76, 0.82, 0.92))
            .with_property("TextSize", 14.0f32),
    );
    InstanceBuilder::new("ScreenGui")
        .with_name(name)
        .with_property("ResetOnSpawn", false)
        .with_child(panel)
}

const PATROL_AI_SCRIPT: &str = r#"local model = script.Parent
local hum = model:WaitForChild("Humanoid")
local root = model:WaitForChild("HumanoidRootPart")
local origin = root.Position
local radius = 18

while hum.Health > 0 do
	local angle = math.random() * math.pi * 2
	local dist = 6 + math.random() * radius
	local target = origin + Vector3.new(math.cos(angle) * dist, 0, math.sin(angle) * dist)
	hum:MoveTo(target)
	hum.MoveToFinished:Wait()
	task.wait(0.8 + math.random() * 1.2)
end
"#;

const CHASE_AI_SCRIPT: &str = r#"local Players = game:GetService("Players")
local model = script.Parent
local hum = model:WaitForChild("Humanoid")
local root = model:WaitForChild("HumanoidRootPart")
hum.WalkSpeed = 14

while hum.Health > 0 do
	local nearestRoot, nearestDist = nil, 140
	for _, p in ipairs(Players:GetPlayers()) do
		local char = p.Character
		local r = char and char:FindFirstChild("HumanoidRootPart")
		local h = char and char:FindFirstChildOfClass("Humanoid")
		if r and h and h.Health > 0 then
			local d = (r.Position - root.Position).Magnitude
			if d < nearestDist then
				nearestDist = d
				nearestRoot = r
			end
		end
	end
	if nearestRoot then
		hum:MoveTo(nearestRoot.Position)
		if nearestDist < 4.5 then
			local h = nearestRoot.Parent:FindFirstChildOfClass("Humanoid")
			if h then h:TakeDamage(20) end
			task.wait(0.75)
		end
	end
	task.wait(0.15)
end
"#;

const OBBY_SERVER_SCRIPT: &str = r#"local Players = game:GetService("Players")
local course = workspace:WaitForChild("ObbyCourse")

Players.PlayerAdded:Connect(function(player)
	local ls = Instance.new("Folder")
	ls.Name = "leaderstats"
	ls.Parent = player
	local stage = Instance.new("IntValue")
	stage.Name = "Stage"
	stage.Value = 0
	stage.Parent = ls
end)

for _, obj in ipairs(course:GetDescendants()) do
	if obj:IsA("BasePart") and obj.Name == "KillBrick" then
		obj.Touched:Connect(function(hit)
			local hum = hit.Parent and hit.Parent:FindFirstChildOfClass("Humanoid")
			if hum then hum.Health = 0 end
		end)
	elseif obj:IsA("BasePart") and string.sub(obj.Name, 1, 11) == "Checkpoint_" then
		local num = tonumber(string.sub(obj.Name, 12)) or 0
		obj.Touched:Connect(function(hit)
			local player = Players:GetPlayerFromCharacter(hit.Parent)
			if player and player:FindFirstChild("leaderstats") then
				local st = player.leaderstats:FindFirstChild("Stage")
				if st and num > st.Value then
					st.Value = num
				end
			end
		end)
	end
end
"#;

const FPS_SERVER_SCRIPT: &str = r#"local Players = game:GetService("Players")
local ReplicatedStorage = game:GetService("ReplicatedStorage")
local fireShot = ReplicatedStorage:WaitForChild("FireShot")

Players.PlayerAdded:Connect(function(player)
	local ls = Instance.new("Folder")
	ls.Name = "leaderstats"
	ls.Parent = player
	local kills = Instance.new("IntValue")
	kills.Name = "Kills"
	kills.Value = 0
	kills.Parent = ls
end)

fireShot.OnServerEvent:Connect(function(player, origin, dir)
	if typeof(origin) ~= "Vector3" or typeof(dir) ~= "Vector3" then return end
	local params = RaycastParams.new()
	params.FilterType = Enum.RaycastFilterType.Exclude
	params.FilterDescendantsInstances = player.Character and {player.Character} or {}
	local res = workspace:Raycast(origin, dir.Unit * 300, params)
	if res and res.Instance then
		local model = res.Instance:FindFirstAncestorOfClass("Model")
		local hum = model and model:FindFirstChildOfClass("Humanoid")
		if hum and hum.Health > 0 then
			hum:TakeDamage(34)
			if hum.Health <= 0 then
				local ls = player:FindFirstChild("leaderstats")
				local kills = ls and ls:FindFirstChild("Kills")
				if kills then kills.Value += 1 end
			end
		end
	end
end)
"#;

const FPS_CLIENT_SCRIPT: &str = r#"local Players = game:GetService("Players")
local UserInputService = game:GetService("UserInputService")
local ReplicatedStorage = game:GetService("ReplicatedStorage")
local fireShot = ReplicatedStorage:WaitForChild("FireShot")
local title = script.Parent:WaitForChild("TopBanner"):WaitForChild("Title")

local ammo = 30
local reloading = false

local function refresh()
	local ls = Players.LocalPlayer:FindFirstChild("leaderstats")
	local k = ls and ls:FindFirstChild("Kills") and ls.Kills.Value or 0
	title.Text = reloading and "RELOADING..." or ("AMMO: " .. ammo .. " / 30  |  KILLS: " .. k)
end

UserInputService.InputBegan:Connect(function(input, gp)
	if gp then return end
	if input.UserInputType == Enum.UserInputType.MouseButton1 and not reloading and ammo > 0 then
		ammo -= 1
		local cam = workspace.CurrentCamera
		if cam then
			fireShot:FireServer(cam.CFrame.Position, cam.CFrame.LookVector)
		end
		refresh()
	elseif input.KeyCode == Enum.KeyCode.R and not reloading and ammo < 30 then
		reloading = true
		refresh()
		task.wait(1.1)
		ammo = 30
		reloading = false
		refresh()
	end
end)
"#;

const COMBAT_SERVER_SCRIPT: &str = r#"local ReplicatedStorage = game:GetService("ReplicatedStorage")
local melee = ReplicatedStorage:WaitForChild("MeleeAttack")

melee.OnServerEvent:Connect(function(player)
	local char = player.Character
	local root = char and char:FindFirstChild("HumanoidRootPart")
	if not root then return end
	for _, m in ipairs(workspace:GetChildren()) do
		if m:IsA("Model") and m ~= char then
			local hum = m:FindFirstChildOfClass("Humanoid")
			local er = m:FindFirstChild("HumanoidRootPart")
			if hum and er and hum.Health > 0 and (er.Position - root.Position).Magnitude <= 9 then
				hum:TakeDamage(25)
			end
		end
	end
end)
"#;

const COMBAT_CLIENT_SCRIPT: &str = r#"local Players = game:GetService("Players")
local UserInputService = game:GetService("UserInputService")
local ReplicatedStorage = game:GetService("ReplicatedStorage")
local melee = ReplicatedStorage:WaitForChild("MeleeAttack")
local cooldown = 0

UserInputService.InputBegan:Connect(function(input, gp)
	if gp then return end
	if input.UserInputType == Enum.UserInputType.MouseButton1 and os.clock() - cooldown > 0.35 then
		cooldown = os.clock()
		melee:FireServer()
	elseif input.KeyCode == Enum.KeyCode.Q then
		local char = Players.LocalPlayer.Character
		local root = char and char:FindFirstChild("HumanoidRootPart")
		if root then
			root.AssemblyLinearVelocity = root.CFrame.LookVector * 65 + Vector3.new(0, 12, 0)
		end
	end
end)
"#;

const TYCOON_SERVER_SCRIPT: &str = r#"local Players = game:GetService("Players")
local plot = workspace:WaitForChild("TycoonPlot")
local dropper = plot:WaitForChild("Dropper")
local collector = plot:WaitForChild("Collector")
local upgrader = plot:WaitForChild("BuyUpgraderButton")

local dropValue = 10
local level = 1

Players.PlayerAdded:Connect(function(player)
	local ls = Instance.new("Folder")
	ls.Name = "leaderstats"
	ls.Parent = player
	local cash = Instance.new("IntValue")
	cash.Name = "Cash"
	cash.Value = 0
	cash.Parent = ls
end)

upgrader.Touched:Connect(function(hit)
	local p = Players:GetPlayerFromCharacter(hit.Parent)
	local cash = p and p:FindFirstChild("leaderstats") and p.leaderstats:FindFirstChild("Cash")
	local cost = level * 50
	if cash and cash.Value >= cost then
		cash.Value -= cost
		level += 1
		dropValue = level * 15
	end
end)

task.spawn(function()
	while true do
		task.wait(1.5)
		local ore = Instance.new("Part")
		ore.Name = "Ore"
		ore.Size = Vector3.new(1.6, 1.6, 1.6)
		ore.Position = dropper.Position - Vector3.new(0, 3, 0)
		ore.Color = Color3.fromRGB(255, 210, 60)
		ore.Material = Enum.Material.Neon
		ore.Parent = plot
		ore.AssemblyLinearVelocity = Vector3.new(0, 0, 12)
		task.delay(4, function()
			if ore.Parent then ore:Destroy() end
			for _, p in ipairs(Players:GetPlayers()) do
				local cash = p:FindFirstChild("leaderstats") and p.leaderstats:FindFirstChild("Cash")
				if cash then cash.Value += dropValue end
			end
		end)
	end
end)
"#;
