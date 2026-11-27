//! Native Animation Editor (`Tab::AnimationEditor`, wired to `clip_editor`):
//! works natively on Android and Desktop without requiring Roblox's closed-source
//! BuiltInPlugins. Supports R15, R6, and custom Motor6D rigs, interactive
//! timeline scrubbing, live 3D viewport posing, per-joint rotation/position
//! sliders, L/R pose mirroring, easing curves, KeyframeMarkers (events),
//! built-in procedural animation presets, and saving directly as a Roblox
//! `KeyframeSequence` instance in the place.

use std::collections::{BTreeMap, HashMap};
use std::time::Instant;

use egui::{Color32, Pos2, RichText, Sense, Stroke, Vec2};
use glam::{EulerRot, Mat3, Quat};
use rbx_dom_weak::types::{CFrame, Enum, Matrix3, Ref, Variant, Vector3};
use rbx_dom_weak::{ustr, InstanceBuilder};
use rustudio_core::doc::{self, Document};
use rustudio_core::math::Xform;
use rustudio_engine::rig::{self, BodyColors, RigType, Scales};
use rustudio_engine::theme;

use crate::app::Editor;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EasingStyle {
    Linear,
    Constant,
    Elastic,
    Cubic,
    Bounce,
}

impl EasingStyle {
    pub const ALL: [EasingStyle; 5] = [
        EasingStyle::Linear,
        EasingStyle::Cubic,
        EasingStyle::Constant,
        EasingStyle::Bounce,
        EasingStyle::Elastic,
    ];

    pub fn label(self) -> &'static str {
        match self {
            EasingStyle::Linear => "Linear",
            EasingStyle::Constant => "Constant",
            EasingStyle::Elastic => "Elastic",
            EasingStyle::Cubic => "Cubic",
            EasingStyle::Bounce => "Bounce",
        }
    }

    pub fn enum_id(self) -> u32 {
        match self {
            EasingStyle::Linear => 0,
            EasingStyle::Constant => 1,
            EasingStyle::Elastic => 2,
            EasingStyle::Cubic => 3,
            EasingStyle::Bounce => 4,
        }
    }

    pub fn from_enum_id(v: u32) -> Self {
        match v {
            1 => EasingStyle::Constant,
            2 => EasingStyle::Elastic,
            3 | 5 => EasingStyle::Cubic,
            4 => EasingStyle::Bounce,
            _ => EasingStyle::Linear,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EasingDir {
    In,
    Out,
    InOut,
}

impl EasingDir {
    pub const ALL: [EasingDir; 3] = [EasingDir::In, EasingDir::Out, EasingDir::InOut];

    pub fn label(self) -> &'static str {
        match self {
            EasingDir::In => "In",
            EasingDir::Out => "Out",
            EasingDir::InOut => "InOut",
        }
    }

    pub fn enum_id(self) -> u32 {
        match self {
            EasingDir::In => 0,
            EasingDir::Out => 1,
            EasingDir::InOut => 2,
        }
    }

    pub fn from_enum_id(v: u32) -> Self {
        match v {
            0 => EasingDir::In,
            1 => EasingDir::Out,
            _ => EasingDir::InOut,
        }
    }
}

#[derive(Debug, Clone)]
pub struct JointPose {
    pub pos: [f32; 3],
    pub rot_deg: [f32; 3],
    pub weight: f32,
    pub style: EasingStyle,
    pub dir: EasingDir,
}

impl Default for JointPose {
    fn default() -> Self {
        JointPose {
            pos: [0.0; 3],
            rot_deg: [0.0; 3],
            weight: 1.0,
            style: EasingStyle::Cubic,
            dir: EasingDir::InOut,
        }
    }
}

#[derive(Debug, Clone)]
pub struct KeyframeData {
    pub time: f32,
    pub name: String,
    pub poses: BTreeMap<String, JointPose>,
    pub markers: Vec<(String, String)>,
}

pub struct AnimEditorState {
    pub rig: Option<Ref>,
    pub clip_ref: Option<Ref>,
    pub clip_name: String,
    pub looped: bool,
    pub priority: u32,
    pub length: f32,
    pub cursor: f32,
    pub playing: bool,
    pub speed: f32,
    pub last_tick: Option<Instant>,
    pub keyframes: Vec<KeyframeData>,
    pub selected_kf: usize,
    pub selected_joint: String,
    pub new_marker_name: String,
    pub new_marker_val: String,
    pub status: Option<String>,
    /// Saved original part CFrames before previewing a pose so Reset restores cleanly.
    rest_cframes: HashMap<Ref, CFrame>,
    rest_rig: Option<Ref>,
}

impl Default for AnimEditorState {
    fn default() -> Self {
        AnimEditorState {
            rig: None,
            clip_ref: None,
            clip_name: "NewAnimation".into(),
            looped: true,
            priority: 3,
            length: 1.0,
            cursor: 0.0,
            playing: false,
            speed: 1.0,
            last_tick: None,
            keyframes: vec![
                KeyframeData {
                    time: 0.0,
                    name: "Start".into(),
                    poses: BTreeMap::new(),
                    markers: Vec::new(),
                },
                KeyframeData {
                    time: 0.5,
                    name: "Mid".into(),
                    poses: BTreeMap::new(),
                    markers: Vec::new(),
                },
                KeyframeData {
                    time: 1.0,
                    name: "End".into(),
                    poses: BTreeMap::new(),
                    markers: Vec::new(),
                },
            ],
            selected_kf: 0,
            selected_joint: "UpperTorso".into(),
            new_marker_name: String::new(),
            new_marker_val: String::new(),
            status: None,
            rest_cframes: HashMap::new(),
            rest_rig: None,
        }
    }
}

fn eval_ease(t: f32, style: EasingStyle, dir: EasingDir) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let ease_in = |x: f32| -> f32 {
        match style {
            EasingStyle::Linear => x,
            EasingStyle::Constant => if x >= 1.0 { 1.0 } else { 0.0 },
            EasingStyle::Cubic => x * x * x,
            EasingStyle::Bounce => {
                let y = 1.0 - x;
                1.0 - bounce_out(y)
            }
            EasingStyle::Elastic => {
                if x <= 0.0 || x >= 1.0 {
                    x
                } else {
                    -(2.0f32.powf(10.0 * x - 10.0)) * ((x * 10.0 - 10.75) * (2.0 * std::f32::consts::PI / 3.0)).sin()
                }
            }
        }
    };
    match dir {
        EasingDir::In => ease_in(t),
        EasingDir::Out => 1.0 - ease_in(1.0 - t),
        EasingDir::InOut => {
            if t < 0.5 {
                0.5 * ease_in(t * 2.0)
            } else {
                1.0 - 0.5 * ease_in((1.0 - t) * 2.0)
            }
        }
    }
}

fn bounce_out(x: f32) -> f32 {
    let n1 = 7.5625;
    let d1 = 2.75;
    if x < 1.0 / d1 {
        n1 * x * x
    } else if x < 2.0 / d1 {
        let t = x - 1.5 / d1;
        n1 * t * t + 0.75
    } else if x < 2.5 / d1 {
        let t = x - 2.25 / d1;
        n1 * t * t + 0.9375
    } else {
        let t = x - 2.625 / d1;
        n1 * t * t + 0.984375
    }
}

fn cf_from_pose(p: &JointPose) -> CFrame {
    let m = Mat3::from_euler(
        EulerRot::YXZ,
        p.rot_deg[1].to_radians(),
        p.rot_deg[0].to_radians(),
        p.rot_deg[2].to_radians(),
    );
    CFrame::new(
        Vector3::new(p.pos[0], p.pos[1], p.pos[2]),
        Matrix3::new(
            Vector3::new(m.x_axis.x, m.y_axis.x, m.z_axis.x),
            Vector3::new(m.x_axis.y, m.y_axis.y, m.z_axis.y),
            Vector3::new(m.x_axis.z, m.y_axis.z, m.z_axis.z),
        ),
    )
}

fn pose_from_cf(cf: &CFrame) -> ([f32; 3], [f32; 3]) {
    let xf = Xform::from_cframe(cf);
    let (ry, rx, rz) = xf.rot.to_euler(EulerRot::YXZ);
    (
        [xf.pos.x, xf.pos.y, xf.pos.z],
        [rx.to_degrees(), ry.to_degrees(), rz.to_degrees()],
    )
}

fn mirror_joint_name(name: &str) -> Option<String> {
    if let Some(rest) = name.strip_prefix("Left") {
        Some(format!("Right{rest}"))
    } else if let Some(rest) = name.strip_prefix("Right") {
        Some(format!("Left{rest}"))
    } else {
        None
    }
}

fn list_rigs(doc: &Document) -> Vec<(Ref, String)> {
    let mut out = Vec::new();
    for inst in doc.dom.descendants() {
        if inst.class.as_str() != "Model" {
            continue;
        }
        let has_hum = inst.children().iter().any(|&c| {
            doc.class_of(c)
                .is_some_and(|cl| matches!(cl.as_str(), "Humanoid" | "AnimationController"))
        });
        if has_hum {
            out.push((inst.referent(), inst.name.clone()));
        }
    }
    out
}

fn list_clips(doc: &Document) -> Vec<(Ref, String)> {
    doc.dom
        .descendants()
        .filter(|i| i.class.as_str() == "KeyframeSequence")
        .map(|i| (i.referent(), i.name.clone()))
        .collect()
}

fn rig_joint_names(doc: &Document, model: Ref) -> Vec<String> {
    let mut names = Vec::new();
    for r in doc.subtree(model) {
        let Some(inst) = doc.dom.get_by_ref(r) else { continue };
        if rig::is_animated_joint(inst.class.as_str()) {
            if let Some(Variant::Ref(p1)) = doc.prop(r, "Part1") {
                if let Some(part) = doc.dom.get_by_ref(p1) {
                    if !names.contains(&part.name) {
                        names.push(part.name.clone());
                    }
                }
            }
        }
    }
    if names.is_empty() {
        for fallback in [
            "LowerTorso", "UpperTorso", "Head",
            "LeftUpperArm", "LeftLowerArm", "LeftHand",
            "RightUpperArm", "RightLowerArm", "RightHand",
            "LeftUpperLeg", "LeftLowerLeg", "LeftFoot",
            "RightUpperLeg", "RightLowerLeg", "RightFoot",
        ] {
            names.push(fallback.into());
        }
    }
    names
}

fn is_r6_rig(doc: &Document, model: Ref) -> bool {
    doc.dom
        .get_by_ref(model)
        .is_some_and(|m| m.children().iter().any(|&c| doc.dom.get_by_ref(c).is_some_and(|i| i.name == "Torso")))
}

impl AnimEditorState {
    fn sort_keyframes(&mut self) {
        self.keyframes.sort_by(|a, b| a.time.partial_cmp(&b.time).unwrap_or(std::cmp::Ordering::Equal));
        if self.keyframes.is_empty() {
            self.keyframes.push(KeyframeData {
                time: 0.0,
                name: "Start".into(),
                poses: BTreeMap::new(),
                markers: Vec::new(),
            });
        }
        self.length = self.keyframes.last().map(|k| k.time.max(0.25)).unwrap_or(1.0);
        self.selected_kf = self.selected_kf.min(self.keyframes.len().saturating_sub(1));
    }

    /// Samples interpolated joint `Xform` per Part1 name at `time`.
    fn sample_at(&self, time: f32) -> HashMap<String, Xform> {
        let mut joints: Vec<String> = Vec::new();
        for kf in &self.keyframes {
            for k in kf.poses.keys() {
                if !joints.contains(k) {
                    joints.push(k.clone());
                }
            }
        }
        let mut out = HashMap::new();
        for j in joints {
            let keys: Vec<(f32, &JointPose)> = self
                .keyframes
                .iter()
                .filter_map(|kf| kf.poses.get(&j).map(|p| (kf.time, p)))
                .collect();
            if keys.is_empty() {
                continue;
            }
            let xf = if time <= keys[0].0 || keys.len() == 1 {
                Xform::from_cframe(&cf_from_pose(keys[0].1))
            } else if time >= keys.last().unwrap().0 {
                Xform::from_cframe(&cf_from_pose(keys.last().unwrap().1))
            } else {
                let mut idx = 0;
                for i in 0..keys.len() - 1 {
                    if time >= keys[i].0 && time <= keys[i + 1].0 {
                        idx = i;
                        break;
                    }
                }
                let (t0, p0) = keys[idx];
                let (t1, p1) = keys[idx + 1];
                let raw = ((time - t0) / (t1 - t0).max(1e-5)).clamp(0.0, 1.0);
                let alpha = eval_ease(raw, p0.style, p0.dir);
                let x0 = Xform::from_cframe(&cf_from_pose(p0));
                let x1 = Xform::from_cframe(&cf_from_pose(p1));
                let q0 = Quat::from_mat3(&x0.rot);
                let q1 = Quat::from_mat3(&x1.rot);
                Xform {
                    pos: x0.pos.lerp(x1.pos, alpha),
                    rot: Mat3::from_quat(q0.slerp(q1, alpha)),
                }
            };
            out.insert(j, xf);
        }
        out
    }

    fn load_preset(&mut self, preset: &str, r6: bool) {
        self.clip_name = format!("Preset_{preset}");
        self.clip_ref = None;
        let p = |pos: [f32; 3], rot: [f32; 3]| JointPose { pos, rot_deg: rot, ..Default::default() };
        let kf = |time: f32, name: &str, list: &[(&str, [f32; 3], [f32; 3])], markers: &[(&str, &str)]| {
            let mut poses = BTreeMap::new();
            for &(j, pos, rot) in list {
                poses.insert(j.to_string(), p(pos, rot));
            }
            KeyframeData {
                time,
                name: name.into(),
                poses,
                markers: markers.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect(),
            }
        };
        let torso = if r6 { "Torso" } else { "UpperTorso" };
        let ra = if r6 { "Right Arm" } else { "RightUpperArm" };
        let la = if r6 { "Left Arm" } else { "LeftUpperArm" };
        let rl = if r6 { "Right Leg" } else { "RightUpperLeg" };
        let ll = if r6 { "Left Leg" } else { "LeftUpperLeg" };
        let rla = if r6 { "Head" } else { "RightLowerArm" };

        let (looped, frames) = match preset {
            "Idle" => (true, vec![
                kf(0.0, "Start", &[(torso, [0.0; 3], [0.0; 3]), (ra, [0.0; 3], [0.0, 0.0, -4.0]), (la, [0.0; 3], [0.0, 0.0, 4.0])], &[]),
                kf(1.0, "Breathe", &[(torso, [0.0, 0.08, 0.0], [2.5, 0.0, 0.0]), (ra, [0.0; 3], [3.0, 0.0, -8.0]), (la, [0.0; 3], [3.0, 0.0, 8.0])], &[]),
                kf(2.0, "End", &[(torso, [0.0; 3], [0.0; 3]), (ra, [0.0; 3], [0.0, 0.0, -4.0]), (la, [0.0; 3], [0.0, 0.0, 4.0])], &[]),
            ]),
            "Walk" => (true, vec![
                kf(0.0, "StepL", &[(ra, [0.0; 3], [32.0, 0.0, 0.0]), (la, [0.0; 3], [-32.0, 0.0, 0.0]), (rl, [0.0; 3], [-36.0, 0.0, 0.0]), (ll, [0.0; 3], [36.0, 0.0, 0.0])], &[("Footstep", "Left")]),
                kf(0.35, "StepR", &[(ra, [0.0; 3], [-32.0, 0.0, 0.0]), (la, [0.0; 3], [32.0, 0.0, 0.0]), (rl, [0.0; 3], [36.0, 0.0, 0.0]), (ll, [0.0; 3], [-36.0, 0.0, 0.0])], &[("Footstep", "Right")]),
                kf(0.70, "Loop", &[(ra, [0.0; 3], [32.0, 0.0, 0.0]), (la, [0.0; 3], [-32.0, 0.0, 0.0]), (rl, [0.0; 3], [-36.0, 0.0, 0.0]), (ll, [0.0; 3], [36.0, 0.0, 0.0])], &[]),
            ]),
            "Run" => (true, vec![
                kf(0.0, "SprintL", &[(torso, [0.0; 3], [-14.0, 8.0, 0.0]), (ra, [0.0; 3], [58.0, 0.0, 0.0]), (la, [0.0; 3], [-52.0, 0.0, 0.0]), (rl, [0.0; 3], [-56.0, 0.0, 0.0]), (ll, [0.0; 3], [56.0, 0.0, 0.0])], &[("Footstep", "Left")]),
                kf(0.24, "SprintR", &[(torso, [0.0, 0.14, 0.0], [-14.0, -8.0, 0.0]), (ra, [0.0; 3], [-52.0, 0.0, 0.0]), (la, [0.0; 3], [58.0, 0.0, 0.0]), (rl, [0.0; 3], [56.0, 0.0, 0.0]), (ll, [0.0; 3], [-56.0, 0.0, 0.0])], &[("Footstep", "Right")]),
                kf(0.48, "Loop", &[(torso, [0.0; 3], [-14.0, 8.0, 0.0]), (ra, [0.0; 3], [58.0, 0.0, 0.0]), (la, [0.0; 3], [-52.0, 0.0, 0.0]), (rl, [0.0; 3], [-56.0, 0.0, 0.0]), (ll, [0.0; 3], [56.0, 0.0, 0.0])], &[]),
            ]),
            "Jump" => (false, vec![
                kf(0.0, "Crouch", &[(ra, [0.0; 3], [-20.0, 0.0, 0.0]), (la, [0.0; 3], [-20.0, 0.0, 0.0]), (rl, [0.0; 3], [22.0, 0.0, 0.0]), (ll, [0.0; 3], [22.0, 0.0, 0.0])], &[]),
                kf(0.25, "Apex", &[(ra, [0.0; 3], [140.0, 0.0, -18.0]), (la, [0.0; 3], [140.0, 0.0, 18.0]), (rl, [0.0; 3], [-24.0, 0.0, 0.0]), (ll, [0.0; 3], [18.0, 0.0, 0.0])], &[]),
                kf(0.65, "Land", &[(ra, [0.0; 3], [0.0; 3]), (la, [0.0; 3], [0.0; 3]), (rl, [0.0; 3], [0.0; 3]), (ll, [0.0; 3], [0.0; 3])], &[]),
            ]),
            "Wave" => (true, vec![
                kf(0.0, "WaveA", &[(ra, [0.0; 3], [150.0, 0.0, -25.0]), (rla, [0.0; 3], [0.0, 0.0, -20.0])], &[]),
                kf(0.25, "WaveB", &[(ra, [0.0; 3], [150.0, 0.0, -5.0]), (rla, [0.0; 3], [0.0, 0.0, 22.0])], &[]),
                kf(0.50, "WaveC", &[(ra, [0.0; 3], [150.0, 0.0, -25.0]), (rla, [0.0; 3], [0.0, 0.0, -20.0])], &[]),
            ]),
            "Punch" => (false, vec![
                kf(0.0, "Windup", &[(torso, [0.0; 3], [0.0, -25.0, 0.0]), (ra, [0.0; 3], [-30.0, 0.0, 0.0]), (la, [0.0; 3], [50.0, 0.0, 0.0])], &[]),
                kf(0.14, "Impact", &[(torso, [0.0; 3], [-8.0, 32.0, 0.0]), (ra, [0.0; 3], [95.0, -10.0, 0.0]), (la, [0.0; 3], [-20.0, 0.0, 0.0])], &[("Hit", "Fist")]),
                kf(0.38, "Recover", &[(torso, [0.0; 3], [0.0; 3]), (ra, [0.0; 3], [0.0; 3]), (la, [0.0; 3], [0.0; 3])], &[]),
            ]),
            "Slash" => (false, vec![
                kf(0.0, "Raise", &[(torso, [0.0; 3], [0.0, -40.0, 0.0]), (ra, [0.0; 3], [135.0, -30.0, -35.0])], &[("SwingStart", "1")]),
                kf(0.16, "Strike", &[(torso, [0.0; 3], [-10.0, 42.0, 0.0]), (ra, [0.0; 3], [25.0, 45.0, 25.0])], &[("Hit", "Blade")]),
                kf(0.45, "Recover", &[(torso, [0.0; 3], [0.0; 3]), (ra, [0.0; 3], [0.0; 3])], &[]),
            ]),
            _ => (true, vec![
                kf(0.0, "DanceA", &[(torso, [0.0; 3], [0.0, 15.0, 12.0]), (ra, [0.0; 3], [115.0, 0.0, -30.0]), (la, [0.0; 3], [45.0, 0.0, 25.0]), (rl, [0.0; 3], [20.0, 0.0, -8.0]), (ll, [0.0; 3], [-10.0, 0.0, 8.0])], &[]),
                kf(0.40, "DanceB", &[(torso, [0.0, 0.2, 0.0], [0.0, -15.0, -12.0]), (ra, [0.0; 3], [45.0, 0.0, -25.0]), (la, [0.0; 3], [115.0, 0.0, 30.0]), (rl, [0.0; 3], [-10.0, 0.0, -8.0]), (ll, [0.0; 3], [20.0, 0.0, 8.0])], &[]),
                kf(0.80, "Loop", &[(torso, [0.0; 3], [0.0, 15.0, 12.0]), (ra, [0.0; 3], [115.0, 0.0, -30.0]), (la, [0.0; 3], [45.0, 0.0, 25.0]), (rl, [0.0; 3], [20.0, 0.0, -8.0]), (ll, [0.0; 3], [-10.0, 0.0, 8.0])], &[]),
            ]),
        };
        self.looped = looped;
        self.keyframes = frames;
        self.cursor = 0.0;
        self.selected_kf = 0;
        self.sort_keyframes();
        self.status = Some(format!("Loaded {preset} preset"));
    }

    fn load_from_dom(&mut self, doc: &Document, seq_ref: Ref) {
        let Some(seq) = doc.dom.get_by_ref(seq_ref) else { return };
        self.clip_ref = Some(seq_ref);
        self.clip_name = seq.name.clone();
        self.looped = !matches!(doc.prop(seq_ref, "Loop"), Some(Variant::Bool(false)));
        if let Some(Variant::Enum(e)) = doc.prop(seq_ref, "Priority") {
            self.priority = e.to_u32();
        }
        let mut loaded = Vec::new();
        for &kf_ref in seq.children() {
            let Some(kf_inst) = doc.dom.get_by_ref(kf_ref) else { continue };
            if kf_inst.class.as_str() != "Keyframe" {
                continue;
            }
            let time = doc::f32_of(doc.prop(kf_ref, "Time")).unwrap_or(0.0);
            let mut poses = BTreeMap::new();
            let mut markers = Vec::new();
            let mut stack: Vec<Ref> = kf_inst.children().to_vec();
            while let Some(r) = stack.pop() {
                let Some(inst) = doc.dom.get_by_ref(r) else { continue };
                stack.extend_from_slice(inst.children());
                if inst.class.as_str() == "Pose" && inst.name != "HumanoidRootPart" {
                    let (pos, rot_deg) = match doc.prop(r, "CFrame") {
                        Some(Variant::CFrame(cf)) => pose_from_cf(&cf),
                        _ => ([0.0; 3], [0.0; 3]),
                    };
                    let weight = doc::f32_of(doc.prop(r, "Weight")).unwrap_or(1.0);
                    let style = doc::enum_u32(doc.prop(r, "EasingStyle")).map(EasingStyle::from_enum_id).unwrap_or(EasingStyle::Cubic);
                    let dir = doc::enum_u32(doc.prop(r, "EasingDirection")).map(EasingDir::from_enum_id).unwrap_or(EasingDir::InOut);
                    poses.insert(inst.name.clone(), JointPose { pos, rot_deg, weight, style, dir });
                } else if inst.class.as_str() == "KeyframeMarker" {
                    let val = match doc.prop(r, "Value") {
                        Some(Variant::String(s)) => s,
                        _ => String::new(),
                    };
                    markers.push((inst.name.clone(), val));
                }
            }
            loaded.push(KeyframeData { time, name: kf_inst.name.clone(), poses, markers });
        }
        self.keyframes = loaded;
        self.cursor = 0.0;
        self.selected_kf = 0;
        self.sort_keyframes();
        self.status = Some(format!("Loaded '{}'", self.clip_name));
    }

    fn build_keyframe_sequence(&self) -> InstanceBuilder {
        let mut seq = InstanceBuilder::new("KeyframeSequence")
            .with_name(if self.clip_name.trim().is_empty() { "Animation" } else { self.clip_name.trim() })
            .with_property("Loop", Variant::Bool(self.looped))
            .with_property("Priority", Enum::from_u32(self.priority));
        for kf in &self.keyframes {
            let mut root = InstanceBuilder::new("Pose")
                .with_name("HumanoidRootPart")
                .with_property("CFrame", CFrame::identity())
                .with_property("Weight", 1.0f32);
            for (joint, jp) in &kf.poses {
                root.add_child(
                    InstanceBuilder::new("Pose")
                        .with_name(joint.clone())
                        .with_property("CFrame", cf_from_pose(jp))
                        .with_property("Weight", jp.weight)
                        .with_property("EasingStyle", Enum::from_u32(jp.style.enum_id()))
                        .with_property("EasingDirection", Enum::from_u32(jp.dir.enum_id())),
                );
            }
            let mut kfb = InstanceBuilder::new("Keyframe")
                .with_name(kf.name.clone())
                .with_property("Time", kf.time)
                .with_child(root);
            for (mn, mv) in &kf.markers {
                kfb.add_child(
                    InstanceBuilder::new("KeyframeMarker")
                        .with_name(mn.clone())
                        .with_property("Value", Variant::String(mv.clone())),
                );
            }
            seq.add_child(kfb);
        }
        seq
    }
}

impl Editor {
    fn anim_apply_live_pose(&mut self) {
        if self.game.play.is_some() {
            return;
        }
        let Some(model) = self.anim_ed.rig else { return };
        if !self.game.doc.exists(model) {
            return;
        }
        // Snapshot rest CFrames once per rig so Reset Rest Pose works cleanly.
        if self.anim_ed.rest_rig != Some(model) {
            self.anim_ed.rest_cframes.clear();
            for r in self.game.doc.subtree(model) {
                if let Some(Variant::CFrame(cf)) = self.game.doc.prop(r, "CFrame") {
                    self.anim_ed.rest_cframes.insert(r, cf);
                }
            }
            self.anim_ed.rest_rig = Some(model);
        }

        let rig_data = rig::Rig::read(&self.game.doc, model);
        if rig_data.root.is_none() {
            return;
        }
        let Some(root_xf) = doc::xform_of(&self.game.doc, rig_data.root) else { return };

        let sampled = self.anim_ed.sample_at(self.anim_ed.cursor);
        let mut transforms: HashMap<Ref, Xform> = HashMap::new();
        for j in &rig_data.joints {
            if let Some(part1) = self.game.doc.dom.get_by_ref(j.part1) {
                if let Some(&xf) = sampled.get(&part1.name) {
                    transforms.insert(j.motor, xf);
                }
            }
        }
        let solved = rig_data.solve(root_xf, &transforms);
        rig::write_pose(&mut self.game.doc, &solved, None);
    }

    fn anim_restore_rest_pose(&mut self) {
        if self.game.play.is_some() {
            return;
        }
        let Some(model) = self.anim_ed.rig else { return };
        if self.anim_ed.rest_rig != Some(model) {
            return;
        }
        for (&r, &cf) in &self.anim_ed.rest_cframes {
            if self.game.doc.exists(r) {
                self.game.doc.set_prop_live(r, ustr("CFrame"), Variant::CFrame(cf));
            }
        }
    }

    /// Native Animation Editor dock panel.
    pub fn anim_editor_ui(&mut self, ui: &mut egui::Ui) {
        // Advance playback timer while playing.
        if self.anim_ed.playing {
            let now = Instant::now();
            let dt = self.anim_ed.last_tick.map(|t| t.elapsed().as_secs_f32()).unwrap_or(0.0).min(0.1);
            self.anim_ed.last_tick = Some(now);
            self.anim_ed.cursor += dt * self.anim_ed.speed;
            if self.anim_ed.cursor > self.anim_ed.length {
                if self.anim_ed.looped {
                    self.anim_ed.cursor = 0.0;
                } else {
                    self.anim_ed.cursor = self.anim_ed.length;
                    self.anim_ed.playing = false;
                }
            }
            self.anim_apply_live_pose();
            ui.ctx().request_repaint();
        } else {
            self.anim_ed.last_tick = None;
        }

        let rigs = list_rigs(&self.game.doc);
        let clips = list_clips(&self.game.doc);
        if self.anim_ed.rig.is_some_and(|r| !self.game.doc.exists(r)) {
            self.anim_ed.rig = None;
        }
        if self.anim_ed.rig.is_none() {
            if let Some(&(r, _)) = rigs.first() {
                self.anim_ed.rig = Some(r);
            }
        }

        let mut pose_dirty = false;

        // Top toolbar: Rig, Clip, Presets, Save, Transport.
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Rig:").strong());
            let cur_rig_label = self
                .anim_ed
                .rig
                .and_then(|r| self.game.doc.dom.get_by_ref(r).map(|i| i.name.clone()))
                .unwrap_or_else(|| "(None)".into());
            egui::ComboBox::from_id_salt("anim_ed_rig")
                .selected_text(cur_rig_label)
                .width(120.0)
                .show_ui(ui, |ui| {
                    for (r, name) in &rigs {
                        if ui.selectable_label(self.anim_ed.rig == Some(*r), name).clicked() {
                            self.anim_restore_rest_pose();
                            self.anim_ed.rig = Some(*r);
                            pose_dirty = true;
                        }
                    }
                });
            if ui.small_button("+ R15").clicked() && self.game.play.is_none() {
                if let Some(ws) = self.game.doc.workspace() {
                    let b = rig::build_default("AnimRigR15", RigType::R15, &BodyColors::default(), &Scales::default(), InstanceBuilder::new("Humanoid"));
                    let r = self.game.doc.insert(ws, b);
                    self.anim_ed.rig = Some(r);
                    self.anim_ed.rest_rig = None;
                    self.anim_ed.status = Some("Spawned AnimRigR15".into());
                }
            }
            if ui.small_button("+ R6").clicked() && self.game.play.is_none() {
                if let Some(ws) = self.game.doc.workspace() {
                    let b = rig::build_default("AnimRigR6", RigType::R6, &BodyColors::default(), &Scales::default(), InstanceBuilder::new("Humanoid"));
                    let r = self.game.doc.insert(ws, b);
                    self.anim_ed.rig = Some(r);
                    self.anim_ed.rest_rig = None;
                    self.anim_ed.status = Some("Spawned AnimRigR6".into());
                }
            }

            ui.separator();
            ui.label(RichText::new("Clip:").strong());
            ui.add(egui::TextEdit::singleline(&mut self.anim_ed.clip_name).desired_width(120.0));
            egui::ComboBox::from_id_salt("anim_ed_load")
                .selected_text("Load…")
                .width(80.0)
                .show_ui(ui, |ui| {
                    for (r, name) in &clips {
                        if ui.selectable_label(self.anim_ed.clip_ref == Some(*r), name).clicked() {
                            let r_copy = *r;
                            self.anim_ed.load_from_dom(&self.game.doc, r_copy);
                            pose_dirty = true;
                        }
                    }
                });

            let r6 = self.anim_ed.rig.is_some_and(|m| is_r6_rig(&self.game.doc, m));
            egui::ComboBox::from_id_salt("anim_ed_preset")
                .selected_text("Presets…")
                .width(88.0)
                .show_ui(ui, |ui| {
                    for p in ["Idle", "Walk", "Run", "Jump", "Wave", "Punch", "Slash", "Dance"] {
                        if ui.selectable_label(false, p).clicked() {
                            self.anim_ed.load_preset(p, r6);
                            pose_dirty = true;
                        }
                    }
                });

            if ui.button("Save to Place").clicked() && self.game.play.is_none() {
                let parent = self
                    .game
                    .doc
                    .find_child(self.game.doc.dom.root_ref(), "ReplicatedStorage")
                    .or_else(|| self.game.doc.workspace())
                    .unwrap_or_else(|| self.game.doc.dom.root_ref());
                let seq = self.anim_ed.build_keyframe_sequence();
                let r = self.game.doc.insert(parent, seq);
                self.anim_ed.clip_ref = Some(r);
                self.anim_ed.status = Some(format!("Saved '{}' to ReplicatedStorage", self.anim_ed.clip_name));
            }
            if ui.small_button("Copy Play Luau").clicked() {
                let code = format!(
                    "local Animator = script.Parent:WaitForChild(\"Humanoid\"):WaitForChild(\"Animator\")\n\
                     local seq = game:GetService(\"ReplicatedStorage\"):WaitForChild(\"{}\")\n\
                     local id = game:GetService(\"KeyframeSequenceProvider\"):RegisterKeyframeSequence(seq)\n\
                     local anim = Instance.new(\"Animation\")\n\
                     anim.AnimationId = id\n\
                     local track = Animator:LoadAnimation(anim)\n\
                     track:Play()\n",
                    self.anim_ed.clip_name
                );
                ui.ctx().copy_text(code);
                self.anim_ed.status = Some("Copied Luau playback snippet to clipboard".into());
            }
        });

        // Transport bar.
        ui.horizontal_wrapped(|ui| {
            let play_lbl = if self.anim_ed.playing { "⏸ Pause" } else { "▶ Play" };
            if ui.button(play_lbl).clicked() {
                self.anim_ed.playing = !self.anim_ed.playing;
                self.anim_ed.last_tick = Some(Instant::now());
            }
            if ui.small_button("⏹ Stop").clicked() {
                self.anim_ed.playing = false;
                self.anim_ed.cursor = 0.0;
                self.anim_restore_rest_pose();
            }
            ui.checkbox(&mut self.anim_ed.looped, "Loop");
            ui.label("Speed:");
            ui.add(egui::DragValue::new(&mut self.anim_ed.speed).speed(0.05).range(0.1..=4.0).suffix("x"));
            ui.label("Time:");
            if ui.add(egui::DragValue::new(&mut self.anim_ed.cursor).speed(0.02).range(0.0..=self.anim_ed.length.max(0.25)).suffix("s")).changed() {
                pose_dirty = true;
            }
            ui.separator();
            if ui.small_button("+ Keyframe").clicked() {
                let t = (self.anim_ed.cursor * 100.0).round() / 100.0;
                let base_poses = self
                    .anim_ed
                    .keyframes
                    .get(self.anim_ed.selected_kf)
                    .map(|k| k.poses.clone())
                    .unwrap_or_default();
                self.anim_ed.keyframes.push(KeyframeData {
                    time: t,
                    name: format!("KF_{:.2}", t),
                    poses: base_poses,
                    markers: Vec::new(),
                });
                self.anim_ed.sort_keyframes();
                if let Some(idx) = self.anim_ed.keyframes.iter().position(|k| (k.time - t).abs() < 1e-4) {
                    self.anim_ed.selected_kf = idx;
                }
                pose_dirty = true;
            }
            if ui.small_button("Delete KF").clicked()
                && self.anim_ed.keyframes.len() > 1
                && self.anim_ed.selected_kf < self.anim_ed.keyframes.len()
            {
                self.anim_ed.keyframes.remove(self.anim_ed.selected_kf);
                self.anim_ed.sort_keyframes();
                pose_dirty = true;
            }
            if let Some(msg) = &self.anim_ed.status {
                ui.label(RichText::new(msg).small().color(theme::TEXT_DIM));
            }
        });

        // Interactive Timeline Strip.
        let tl_h = 34.0;
        let (tl_rect, tl_resp) = ui.allocate_exact_size(Vec2::new(ui.available_width().max(120.0), tl_h), Sense::click_and_drag());
        let p = ui.painter();
        p.rect_filled(tl_rect, 4.0, theme::PANEL_DARK);
        p.rect_stroke(tl_rect, 4.0, Stroke::new(1.0, theme::DIVIDER), egui::StrokeKind::Inside);
        let dur = self.anim_ed.length.max(0.25);
        let pad_x = 10.0;
        let span_w = (tl_rect.width() - pad_x * 2.0).max(1.0);
        let x_of = |t: f32| tl_rect.left() + pad_x + (t / dur).clamp(0.0, 1.0) * span_w;

        // Tick labels
        for i in 0..=4 {
            let frac = i as f32 / 4.0;
            let x = tl_rect.left() + pad_x + frac * span_w;
            p.line_segment([Pos2::new(x, tl_rect.top()), Pos2::new(x, tl_rect.top() + 8.0)], Stroke::new(1.0, theme::TEXT_DIM));
            p.text(
                Pos2::new(x + 2.0, tl_rect.top() + 2.0),
                egui::Align2::LEFT_TOP,
                format!("{:.2}s", frac * dur),
                egui::FontId::proportional(10.0),
                theme::TEXT_DIM,
            );
        }

        // Keyframe diamonds
        for (i, kf) in self.anim_ed.keyframes.iter().enumerate() {
            let cx = x_of(kf.time);
            let cy = tl_rect.center().y + 5.0;
            let r = 6.0;
            let col = if i == self.anim_ed.selected_kf { theme::SELECT } else { Color32::from_rgb(235, 190, 60) };
            let pts = vec![
                Pos2::new(cx, cy - r),
                Pos2::new(cx + r, cy),
                Pos2::new(cx, cy + r),
                Pos2::new(cx - r, cy),
            ];
            p.add(egui::Shape::convex_polygon(pts, col, Stroke::new(1.0, Color32::BLACK)));
        }

        // Playhead line
        let ph_x = x_of(self.anim_ed.cursor);
        p.line_segment([Pos2::new(ph_x, tl_rect.top()), Pos2::new(ph_x, tl_rect.bottom())], Stroke::new(2.0, theme::ERROR));

        if tl_resp.clicked() || tl_resp.dragged() {
            if let Some(pos) = tl_resp.interact_pointer_pos() {
                let frac = ((pos.x - (tl_rect.left() + pad_x)) / span_w).clamp(0.0, 1.0);
                self.anim_ed.cursor = frac * dur;
                self.anim_ed.playing = false;
                // Snap selection to nearest keyframe if clicked close to one.
                if let Some((idx, _)) = self
                    .anim_ed
                    .keyframes
                    .iter()
                    .enumerate()
                    .min_by(|(_, a), (_, b)| (a.time - self.anim_ed.cursor).abs().partial_cmp(&(b.time - self.anim_ed.cursor).abs()).unwrap_or(std::cmp::Ordering::Equal))
                {
                    if (self.anim_ed.keyframes[idx].time - self.anim_ed.cursor).abs() < dur * 0.04 {
                        self.anim_ed.selected_kf = idx;
                    }
                }
                pose_dirty = true;
            }
        }

        ui.separator();

        // Joint list + Pose editor split.
        let joints = self
            .anim_ed
            .rig
            .map(|r| rig_joint_names(&self.game.doc, r))
            .unwrap_or_else(|| vec!["UpperTorso".into(), "Head".into(), "RightUpperArm".into(), "LeftUpperArm".into()]);
        if !joints.iter().any(|j| j == &self.anim_ed.selected_joint) {
            if let Some(first) = joints.first() {
                self.anim_ed.selected_joint = first.clone();
            }
        }

        ui.columns(2, |cols| {
            // Left: Joint picker + Keyframe list
            cols[0].label(RichText::new("Joints").strong());
            egui::ScrollArea::vertical().id_salt("anim_joints").max_height(150.0).show(&mut cols[0], |ui| {
                for j in &joints {
                    let has_key = self
                        .anim_ed
                        .keyframes
                        .get(self.anim_ed.selected_kf)
                        .is_some_and(|k| k.poses.contains_key(j));
                    let lbl = if has_key { format!("● {j}") } else { format!("  {j}") };
                    if ui.selectable_label(self.anim_ed.selected_joint == *j, lbl).clicked() {
                        self.anim_ed.selected_joint = j.clone();
                    }
                }
            });

            // Right: Selected Keyframe & Joint sliders
            let kf_idx = self.anim_ed.selected_kf;
            let joint_name = self.anim_ed.selected_joint.clone();
            if let Some(kf) = self.anim_ed.keyframes.get_mut(kf_idx) {
                cols[1].horizontal(|ui| {
                    ui.label(RichText::new(format!("KF #{kf_idx} — {joint_name}")).strong());
                    ui.label("t:");
                    let mut t = kf.time;
                    if ui.add(egui::DragValue::new(&mut t).speed(0.02).range(0.0..=30.0).suffix("s")).changed() {
                        kf.time = t;
                        pose_dirty = true;
                    }
                });
                let mut mirror_insert: Option<(String, JointPose)> = None;
                {
                    let jp = kf.poses.entry(joint_name.clone()).or_default();
                    cols[1].horizontal(|ui| {
                        ui.label("Rot°:");
                        pose_dirty |= ui.add(egui::DragValue::new(&mut jp.rot_deg[0]).speed(1.0).range(-180.0..=180.0).prefix("X ")).changed();
                        pose_dirty |= ui.add(egui::DragValue::new(&mut jp.rot_deg[1]).speed(1.0).range(-180.0..=180.0).prefix("Y ")).changed();
                        pose_dirty |= ui.add(egui::DragValue::new(&mut jp.rot_deg[2]).speed(1.0).range(-180.0..=180.0).prefix("Z ")).changed();
                    });
                    cols[1].horizontal(|ui| {
                        ui.label("Pos:");
                        pose_dirty |= ui.add(egui::DragValue::new(&mut jp.pos[0]).speed(0.05).prefix("X ")).changed();
                        pose_dirty |= ui.add(egui::DragValue::new(&mut jp.pos[1]).speed(0.05).prefix("Y ")).changed();
                        pose_dirty |= ui.add(egui::DragValue::new(&mut jp.pos[2]).speed(0.05).prefix("Z ")).changed();
                    });
                    cols[1].horizontal(|ui| {
                        egui::ComboBox::from_id_salt("anim_ease_style").selected_text(jp.style.label()).width(80.0).show_ui(ui, |ui| {
                            for st in EasingStyle::ALL {
                                if ui.selectable_label(jp.style == st, st.label()).clicked() {
                                    jp.style = st;
                                    pose_dirty = true;
                                }
                            }
                        });
                        egui::ComboBox::from_id_salt("anim_ease_dir").selected_text(jp.dir.label()).width(68.0).show_ui(ui, |ui| {
                            for d in EasingDir::ALL {
                                if ui.selectable_label(jp.dir == d, d.label()).clicked() {
                                    jp.dir = d;
                                    pose_dirty = true;
                                }
                            }
                        });
                        if ui.small_button("Reset").clicked() {
                            *jp = JointPose::default();
                            pose_dirty = true;
                        }
                        if let Some(mirror) = mirror_joint_name(&joint_name) {
                            if ui.small_button("Mirror L↔R").clicked() {
                                let mut cloned = jp.clone();
                                cloned.pos[0] = -cloned.pos[0];
                                cloned.rot_deg[1] = -cloned.rot_deg[1];
                                cloned.rot_deg[2] = -cloned.rot_deg[2];
                                mirror_insert = Some((mirror, cloned));
                                pose_dirty = true;
                            }
                        }
                    });
                }
                if let Some((mirror, cloned)) = mirror_insert {
                    kf.poses.insert(mirror, cloned);
                }
                // KeyframeMarkers (Events)
                cols[1].horizontal(|ui| {
                    ui.label("Event:");
                    ui.add(egui::TextEdit::singleline(&mut self.anim_ed.new_marker_name).hint_text("Name (e.g. Hit)").desired_width(80.0));
                    ui.add(egui::TextEdit::singleline(&mut self.anim_ed.new_marker_val).hint_text("Value").desired_width(60.0));
                    if ui.small_button("+").clicked() && !self.anim_ed.new_marker_name.trim().is_empty() {
                        kf.markers.push((self.anim_ed.new_marker_name.trim().to_string(), self.anim_ed.new_marker_val.clone()));
                        self.anim_ed.new_marker_name.clear();
                        self.anim_ed.new_marker_val.clear();
                    }
                });
                if !kf.markers.is_empty() {
                    let mut remove_idx = None;
                    cols[1].horizontal_wrapped(|ui| {
                        for (mi, (mn, mv)) in kf.markers.iter().enumerate() {
                            if ui.small_button(format!("× {mn}:{mv}")).clicked() {
                                remove_idx = Some(mi);
                            }
                        }
                    });
                    if let Some(mi) = remove_idx {
                        kf.markers.remove(mi);
                    }
                }
            }
        });

        if pose_dirty {
            self.anim_ed.sort_keyframes();
            self.anim_apply_live_pose();
        }
    }
}
