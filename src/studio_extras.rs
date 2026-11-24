//! Native "Studio Pro" Hub (`Tab::StudioTools`) + Live Performance HUD overlay:
//! brings together native Lighting & PostFX Studio, VFX Particle/Light Creator,
//! Device Emulator (Phone/Tablet/Desktop viewport framing for GUI design),
//! Fast Game Starter Templates + 12 Ready-to-Run Luau Game System Scripts,
//! Quick Rig & Part Layout utilities, and a Roblox-style Shift+F1 FPS/Stats HUD.

use egui::{Color32, RichText};
use rbx_dom_weak::types::{
    CFrame, Color3, Color3uint8, Enum, Matrix3, Ref, Variant, Vector2, Vector3,
};
use rbx_dom_weak::{ustr, InstanceBuilder};
use rustudio_core::doc::{self, Document};
use rustudio_engine::rig::{self, BodyColors, RigType, Scales};
use rustudio_engine::theme;

use crate::app::Editor;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProTab {
    #[default]
    Lighting,
    Vfx,
    DeviceEmu,
    FastGames,
    BuildTools,
}

#[derive(Default)]
pub struct StudioExtrasState {
    pub tab: ProTab,
    pub show_perf_hud: bool,
    pub emu_preset: usize,
    pub status: Option<String>,
    pub grid_cols: i32,
    pub grid_spacing: f32,
}

const EMU_PRESETS: &[(&str, Option<[u32; 2]>)] = &[
    ("Free Fit (Default)", None),
    ("Phone Landscape 16:9 (960×540)", Some([960, 540])),
    ("Phone Wide 20:9 (1080×486)", Some([1080, 486])),
    ("Phone Portrait 9:16 (540×960)", Some([540, 960])),
    ("Tablet / iPad 4:3 (1024×768)", Some([1024, 768])),
    ("HD Desktop 720p (1280×720)", Some([1280, 720])),
    ("Full HD 1080p (1920×1080)", Some([1920, 1080])),
];

struct ScriptPreset {
    title: &'static str,
    class: &'static str,
    service: &'static str,
    name: &'static str,
    summary: &'static str,
    source: &'static str,
}

const SCRIPT_LIBRARY: &[ScriptPreset] = &[
    ScriptPreset {
        title: "Leaderstats + Cash",
        class: "Script",
        service: "ServerScriptService",
        name: "LeaderstatsSystem",
        summary: "Creates leaderstats with Cash, Level & Kills for every player.",
        source: r#"local Players = game:GetService("Players")

Players.PlayerAdded:Connect(function(player)
	local ls = Instance.new("Folder")
	ls.Name = "leaderstats"
	ls.Parent = player

	local cash = Instance.new("IntValue")
	cash.Name = "Cash"
	cash.Value = 100
	cash.Parent = ls

	local kills = Instance.new("IntValue")
	kills.Name = "Kills"
	kills.Value = 0
	kills.Parent = ls
end)
"#,
    },
    ScriptPreset {
        title: "Sprint + Stamina HUD",
        class: "LocalScript",
        service: "StarterGui",
        name: "SprintStaminaClient",
        summary: "Hold LeftShift to sprint with smooth FOV & stamina regen.",
        source: r#"local Players = game:GetService("Players")
local RunService = game:GetService("RunService")
local UIS = game:GetService("UserInputService")

local player = Players.LocalPlayer
local stamina = 100
local sprinting = false

UIS.InputBegan:Connect(function(input, gp)
	if not gp and input.KeyCode == Enum.KeyCode.LeftShift then sprinting = true end
end)
UIS.InputEnded:Connect(function(input)
	if input.KeyCode == Enum.KeyCode.LeftShift then sprinting = false end
end)

RunService.Heartbeat:Connect(function(dt)
	local char = player.Character
	local hum = char and char:FindFirstChildOfClass("Humanoid")
	local cam = workspace.CurrentCamera
	if not hum then return end
	if sprinting and stamina > 0 and hum.MoveDirection.Magnitude > 0.1 then
		stamina = math.max(0, stamina - 28 * dt)
		hum.WalkSpeed = 26
		if cam then cam.FieldOfView = cam.FieldOfView + (82 - cam.FieldOfView) * math.min(1, dt * 8) end
	else
		stamina = math.min(100, stamina + 18 * dt)
		hum.WalkSpeed = 16
		if cam then cam.FieldOfView = cam.FieldOfView + (70 - cam.FieldOfView) * math.min(1, dt * 8) end
	end
end)
"#,
    },
    ScriptPreset {
        title: "Double Jump + Q Dash",
        class: "LocalScript",
        service: "StarterGui",
        name: "MovementAbilities",
        summary: "Adds mid-air double jump + Q forward dash burst.",
        source: r#"local Players = game:GetService("Players")
local UIS = game:GetService("UserInputService")

local player = Players.LocalPlayer
local jumpedOnce = false
local canDouble = false
local lastDash = 0

local function bind(char)
	local hum = char:WaitForChild("Humanoid")
	hum.StateChanged:Connect(function(_, new)
		if new == Enum.HumanoidStateType.Landed then
			jumpedOnce = false
			canDouble = false
		elseif new == Enum.HumanoidStateType.Freefall and not jumpedOnce then
			jumpedOnce = true
			canDouble = true
		end
	end)
end

if player.Character then bind(player.Character) end
player.CharacterAdded:Connect(bind)

UIS.InputBegan:Connect(function(input, gp)
	if gp then return end
	local char = player.Character
	local hum = char and char:FindFirstChildOfClass("Humanoid")
	local root = char and char:FindFirstChild("HumanoidRootPart")
	if not hum or not root then return end
	if input.KeyCode == Enum.KeyCode.Space and canDouble then
		canDouble = false
		hum:ChangeState(Enum.HumanoidStateType.Jumping)
	elseif input.KeyCode == Enum.KeyCode.Q and os.clock() - lastDash > 1.2 then
		lastDash = os.clock()
		root.AssemblyLinearVelocity = root.CFrame.LookVector * 70 + Vector3.new(0, 14, 0)
	end
end)
"#,
    },
    ScriptPreset {
        title: "NPC Pathfinding Chase AI",
        class: "Script",
        service: "Workspace",
        name: "PathfindingChaseAI",
        summary: "Drop inside any NPC rig Model to chase players around obstacles using PathfindingService.",
        source: r#"local Players = game:GetService("Players")
local PathfindingService = game:GetService("PathfindingService")

local model = script.Parent
local hum = model:WaitForChild("Humanoid")
local root = model:WaitForChild("HumanoidRootPart")
hum.WalkSpeed = 15

local path = PathfindingService:CreatePath({ AgentRadius = 2, AgentHeight = 5, AgentCanJump = true })

while hum.Health > 0 do
	local target, best = nil, 180
	for _, p in ipairs(Players:GetPlayers()) do
		local c = p.Character
		local r = c and c:FindFirstChild("HumanoidRootPart")
		local h = c and c:FindFirstChildOfClass("Humanoid")
		if r and h and h.Health > 0 then
			local d = (r.Position - root.Position).Magnitude
			if d < best then best, target = d, r end
		end
	end
	if target then
		if best < 5 then
			local h = target.Parent:FindFirstChildOfClass("Humanoid")
			if h then h:TakeDamage(20) end
			task.wait(0.7)
		else
			local ok = pcall(function() path:ComputeAsync(root.Position, target.Position) end)
			if ok and path.Status == Enum.PathStatus.Success then
				local wps = path:GetWaypoints()
				local wp = wps[2] or wps[1]
				if wp then
					if wp.Action == Enum.PathWaypointAction.Jump then hum.Jump = true end
					hum:MoveTo(wp.Position)
				end
			else
				hum:MoveTo(target.Position)
			end
		end
	end
	task.wait(0.2)
end
"#,
    },
    ScriptPreset {
        title: "Day / Night Cycle",
        class: "Script",
        service: "ServerScriptService",
        name: "DayNightCycle",
        summary: "Smoothly advances Lighting.ClockTime over time.",
        source: r#"local Lighting = game:GetService("Lighting")
local RunService = game:GetService("RunService")
local HOURS_PER_SECOND = 0.08

RunService.Heartbeat:Connect(function(dt)
	Lighting.ClockTime = (Lighting.ClockTime + dt * HOURS_PER_SECOND) % 24
end)
"#,
    },
    ScriptPreset {
        title: "Interactive Door (Prompt)",
        class: "Script",
        service: "Workspace",
        name: "InteractiveDoorSetup",
        summary: "Spawns an interactive sliding door with ProximityPrompt & TweenService.",
        source: r#"local TweenService = game:GetService("TweenService")

local door = Instance.new("Part")
door.Name = "SlidingDoor"
door.Anchored = true
door.Size = Vector3.new(6, 9, 1)
door.Position = Vector3.new(0, 4.5, -12)
door.Color = Color3.fromRGB(65, 85, 110)
door.Material = Enum.Material.Metal
door.Parent = workspace

local prompt = Instance.new("ProximityPrompt")
prompt.ActionText = "Open Door"
prompt.ObjectText = "Security Door"
prompt.HoldDuration = 0
prompt.MaxActivationDistance = 12
prompt.Parent = door

local closedCF = door.CFrame
local openCF = closedCF * CFrame.new(5.5, 0, 0)
local isOpen = false

prompt.Triggered:Connect(function()
	isOpen = not isOpen
	prompt.ActionText = isOpen and "Close Door" or "Open Door"
	TweenService:Create(door, TweenInfo.new(0.45, Enum.EasingStyle.Quad, Enum.EasingDirection.Out), {
		CFrame = isOpen and openCF or closedCF
	}):Play()
end)
"#,
    },
    ScriptPreset {
        title: "Kill Brick + Respawn",
        class: "Script",
        service: "ServerScriptService",
        name: "TaggedKillBricks",
        summary: "Any BasePart tagged 'KillBrick' via Tag Editor kills touching Humanoids.",
        source: r#"local CollectionService = game:GetService("CollectionService")

local function hook(part)
	if not part:IsA("BasePart") then return end
	part.Touched:Connect(function(hit)
		local hum = hit.Parent and hit.Parent:FindFirstChildOfClass("Humanoid")
		if hum and hum.Health > 0 then hum.Health = 0 end
	end)
end

for _, p in ipairs(CollectionService:GetTagged("KillBrick")) do hook(p) end
CollectionService:GetInstanceAddedSignal("KillBrick"):Connect(hook)
"#,
    },
    ScriptPreset {
        title: "Round / Match Loop",
        class: "Script",
        service: "ServerScriptService",
        name: "RoundManager",
        summary: "Intermission -> Active Match -> Winner cycle with ReplicatedStorage status StringValue.",
        source: r#"local Players = game:GetService("Players")
local RS = game:GetService("ReplicatedStorage")

local status = Instance.new("StringValue")
status.Name = "RoundStatus"
status.Parent = RS

while true do
	for i = 10, 1, -1 do
		status.Value = "Intermission: " .. i .. "s"
		task.wait(1)
	end
	status.Value = "MATCH LIVE!"
	for i = 60, 1, -1 do
		status.Value = "Time Left: " .. i .. "s (" .. #Players:GetPlayers() .. " players)"
		task.wait(1)
	end
	status.Value = "Round Over!"
	task.wait(3)
end
"#,
    },
    ScriptPreset {
        title: "Raycast Gun System",
        class: "Script",
        service: "ServerScriptService",
        name: "GunSystemServer",
        summary: "Server-validated raycast damage handler for a ReplicatedStorage 'FireGun' RemoteEvent.",
        source: r#"local RS = game:GetService("ReplicatedStorage")
local fire = RS:FindFirstChild("FireGun") or Instance.new("RemoteEvent")
fire.Name = "FireGun"
fire.Parent = RS

fire.OnServerEvent:Connect(function(player, origin, dir)
	if typeof(origin) ~= "Vector3" or typeof(dir) ~= "Vector3" then return end
	local params = RaycastParams.new()
	params.FilterType = Enum.RaycastFilterType.Exclude
	params.FilterDescendantsInstances = player.Character and {player.Character} or {}
	local hit = workspace:Raycast(origin, dir.Unit * 350, params)
	if hit and hit.Instance then
		local model = hit.Instance:FindFirstAncestorOfClass("Model")
		local hum = model and model:FindFirstChildOfClass("Humanoid")
		if hum and hum.Health > 0 then
			local dmg = (hit.Instance.Name == "Head") and 60 or 25
			hum:TakeDamage(dmg)
		end
	end
end)
"#,
    },
    ScriptPreset {
        title: "Shop + Currency GUI",
        class: "LocalScript",
        service: "StarterGui",
        name: "QuickShopGui",
        summary: "Builds a clean bottom-right Shop toggle button & upgrade popup ScreenGui.",
        source: r#"local Players = game:GetService("Players")
local sg = Instance.new("ScreenGui")
sg.Name = "QuickShop"
sg.ResetOnSpawn = false
sg.Parent = Players.LocalPlayer:WaitForChild("PlayerGui")

local btn = Instance.new("TextButton")
btn.Size = UDim2.fromOffset(110, 40)
btn.Position = UDim2.new(1, -124, 1, -56)
btn.BackgroundColor3 = Color3.fromRGB(35, 130, 240)
btn.TextColor3 = Color3.new(1, 1, 1)
btn.TextSize = 18
btn.Text = "SHOP"
btn.Parent = sg
Instance.new("UICorner", btn).CornerRadius = UDim.new(0, 8)

local frame = Instance.new("Frame")
frame.Size = UDim2.fromOffset(300, 200)
frame.Position = UDim2.new(0.5, -150, 0.5, -100)
frame.BackgroundColor3 = Color3.fromRGB(24, 28, 38)
frame.Visible = false
frame.Parent = sg
Instance.new("UICorner", frame).CornerRadius = UDim.new(0, 12)

local title = Instance.new("TextLabel")
title.Size = UDim2.new(1, 0, 0, 38)
title.BackgroundTransparency = 1
title.TextColor3 = Color3.new(1, 1, 1)
title.TextSize = 20
title.Text = "SPEED UPGRADE SHOP"
title.Parent = frame

local buy = Instance.new("TextButton")
buy.Size = UDim2.new(0.8, 0, 0, 44)
buy.Position = UDim2.new(0.1, 0, 0.45, 0)
buy.BackgroundColor3 = Color3.fromRGB(46, 185, 110)
buy.TextColor3 = Color3.new(1, 1, 1)
buy.TextSize = 16
buy.Text = "+4 WalkSpeed Boost"
buy.Parent = frame
Instance.new("UICorner", buy).CornerRadius = UDim.new(0, 8)

btn.MouseButton1Click:Connect(function() frame.Visible = not frame.Visible end)
buy.MouseButton1Click:Connect(function()
	local hum = Players.LocalPlayer.Character and Players.LocalPlayer.Character:FindFirstChildOfClass("Humanoid")
	if hum then hum.WalkSpeed += 4 end
end)
"#,
    },
];

fn cf_at(p: [f32; 3]) -> CFrame {
    CFrame::new(Vector3::new(p[0], p[1], p[2]), Matrix3::identity())
}

fn quick_part(name: &str, pos: [f32; 3], size: [f32; 3], rgb: [u8; 3], mat: u32) -> InstanceBuilder {
    InstanceBuilder::new("Part")
        .with_name(name)
        .with_property("Anchored", true)
        .with_property("Size", Vector3::new(size[0], size[1], size[2]))
        .with_property("CFrame", cf_at(pos))
        .with_property("Color", Color3uint8::new(rgb[0], rgb[1], rgb[2]))
        .with_property("Material", Enum::from_u32(mat))
        .with_property("TopSurface", Enum::from_u32(0))
        .with_property("BottomSurface", Enum::from_u32(0))
}

fn apply_lighting_preset_local(doc: &mut Document, preset: &str) {
    let Some(lighting) = doc.find_child(doc.dom.root_ref(), "Lighting") else { return };
    let (clock, bright, exp, amb, out_amb, fog_end, atmo_d, bloom_i, cc_sat) = match preset {
        "Daylight" => (14.0, 2.5, 0.1, [0.28, 0.30, 0.34], [0.48, 0.52, 0.58], 100000.0, 0.28, 0.6, 0.08),
        "Sunset" => (17.7, 2.2, 0.15, [0.24, 0.16, 0.18], [0.48, 0.32, 0.28], 8000.0, 0.38, 0.9, 0.18),
        "Night" => (1.5, 1.0, 0.0, [0.08, 0.10, 0.16], [0.14, 0.18, 0.28], 2500.0, 0.42, 1.0, -0.08),
        "Horror" => (0.0, 0.25, -0.35, [0.02, 0.02, 0.03], [0.03, 0.035, 0.045], 140.0, 0.62, 0.85, -0.35),
        "Cyberpunk" => (22.5, 0.9, 0.1, [0.14, 0.06, 0.22], [0.10, 0.14, 0.28], 900.0, 0.45, 1.45, 0.28),
        "Sci-Fi" => (10.5, 1.8, 0.05, [0.16, 0.22, 0.28], [0.22, 0.30, 0.38], 4000.0, 0.32, 1.1, 0.05),
        _ => (14.5, 3.0, 0.0, [0.35, 0.35, 0.35], [0.55, 0.55, 0.55], 100000.0, 0.2, 0.4, 0.0),
    };
    let c3 = |a: [f32; 3]| Variant::Color3(Color3::new(a[0], a[1], a[2]));
    doc.set_props(
        format!("Lighting preset {preset}"),
        vec![
            (lighting, ustr("ClockTime"), Variant::Float32(clock)),
            (lighting, ustr("Brightness"), Variant::Float32(bright)),
            (lighting, ustr("ExposureCompensation"), Variant::Float32(exp)),
            (lighting, ustr("Ambient"), c3(amb)),
            (lighting, ustr("OutdoorAmbient"), c3(out_amb)),
            (lighting, ustr("FogEnd"), Variant::Float32(fog_end)),
            (lighting, ustr("GlobalShadows"), Variant::Bool(true)),
        ],
        None,
    );
    for (cls, name, props) in [
        ("Atmosphere", "Atmosphere", vec![("Density", Variant::Float32(atmo_d))]),
        ("BloomEffect", "Bloom", vec![("Enabled", Variant::Bool(true)), ("Intensity", Variant::Float32(bloom_i))]),
        ("ColorCorrectionEffect", "ColorCorrection", vec![("Enabled", Variant::Bool(true)), ("Saturation", Variant::Float32(cc_sat))]),
    ] {
        if let Some(ex) = doc.find_child(lighting, cls) {
            doc.set_props(format!("Update {name}"), props.into_iter().map(|(k, v)| (ex, ustr(k), v)).collect(), None);
        } else {
            let mut b = InstanceBuilder::new(cls).with_name(name);
            for (k, v) in props {
                b.add_property(ustr(k), v);
            }
            doc.insert(lighting, b);
        }
    }
}

impl Editor {
    /// Draws the floating Performance / FPS / Memory Stats HUD when enabled (Window > Performance Summary > Stats).
    pub fn studio_perf_hud_overlay(&self, ctx: &egui::Context) {
        if !self.extras.show_perf_hud {
            return;
        }
        let vp = self.game.viewport_rect;
        if !vp.is_positive() {
            return;
        }
        let instances = self.game.doc.dom.descendants().count();
        let parts = self
            .game
            .doc
            .dom
            .descendants()
            .filter(|i| doc::is_a(i.class.as_str(), "BasePart"))
            .count();
        let mode = if self.game.play.is_some() { "PLAY" } else { "EDIT" };
        let fps = self.fps;
        let fps_col = if fps >= 50.0 {
            Color32::from_rgb(90, 225, 130)
        } else if fps >= 28.0 {
            Color32::from_rgb(245, 200, 75)
        } else {
            theme::ERROR
        };
        egui::Area::new(egui::Id::new("studio_perf_hud"))
            .fixed_pos(egui::pos2(vp.left() + 8.0, vp.top() + 8.0))
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(Color32::from_rgba_unmultiplied(14, 16, 22, 215))
                    .corner_radius(6.0)
                    .inner_margin(egui::Margin::symmetric(8, 5))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(format!("{fps:4.0} FPS")).monospace().strong().color(fps_col));
                            ui.label(RichText::new(format!("· {:.1} ms", self.last_work_ms)).monospace().small());
                            ui.label(RichText::new(format!("· {parts} parts / {instances} inst")).monospace().small().color(theme::TEXT_DIM));
                            ui.label(RichText::new(format!("· {mode}")).monospace().small().color(theme::SELECT));
                        });
                    });
            });
    }

    /// Native Studio Pro Hub dock panel (`Tab::StudioTools`).
    pub fn studio_tools_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            for (t, label) in [
                (ProTab::Lighting, "☀ Lighting"),
                (ProTab::Vfx, "✨ VFX"),
                (ProTab::DeviceEmu, "📱 Device"),
                (ProTab::FastGames, "⚡ Fast Games"),
                (ProTab::BuildTools, "🛠 Build"),
            ] {
                if ui.selectable_label(self.extras.tab == t, label).clicked() {
                    self.extras.tab = t;
                }
            }
        });
        if let Some(msg) = &self.extras.status {
            ui.label(RichText::new(msg).small().color(theme::SELECT));
        }
        ui.separator();

        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match self.extras.tab {
            ProTab::Lighting => self.studio_lighting_tab(ui),
            ProTab::Vfx => self.studio_vfx_tab(ui),
            ProTab::DeviceEmu => self.studio_device_tab(ui),
            ProTab::FastGames => self.studio_fast_games_tab(ui),
            ProTab::BuildTools => self.studio_build_tools_tab(ui),
        });
    }

    fn studio_lighting_tab(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("One-Click Atmosphere & Lighting Presets").strong());
        ui.horizontal_wrapped(|ui| {
            for p in ["Daylight", "Sunset", "Night", "Horror", "Cyberpunk", "Sci-Fi", "Studio"] {
                if ui.button(p).clicked() && self.game.play.is_none() {
                    apply_lighting_preset_local(&mut self.game.doc, p);
                    self.extras.status = Some(format!("Applied {p} lighting preset"));
                }
            }
        });
        ui.separator();
        let Some(lighting) = self.game.doc.find_child(self.game.doc.dom.root_ref(), "Lighting") else {
            ui.label("Lighting service not found.");
            return;
        };
        let mut clock = doc::f32_of(self.game.doc.prop(lighting, "ClockTime")).unwrap_or(14.0);
        let mut bright = doc::f32_of(self.game.doc.prop(lighting, "Brightness")).unwrap_or(2.0);
        let mut exp = doc::f32_of(self.game.doc.prop(lighting, "ExposureCompensation")).unwrap_or(0.0);
        let mut fog_end = doc::f32_of(self.game.doc.prop(lighting, "FogEnd")).unwrap_or(100000.0);
        let mut shadows = !matches!(self.game.doc.prop(lighting, "GlobalShadows"), Some(Variant::Bool(false)));

        let mut edits = Vec::new();
        if ui.add(egui::Slider::new(&mut clock, 0.0..=24.0).text("ClockTime")).changed() {
            edits.push((lighting, ustr("ClockTime"), Variant::Float32(clock)));
        }
        if ui.add(egui::Slider::new(&mut bright, 0.0..=8.0).text("Brightness")).changed() {
            edits.push((lighting, ustr("Brightness"), Variant::Float32(bright)));
        }
        if ui.add(egui::Slider::new(&mut exp, -2.0..=2.0).text("Exposure")).changed() {
            edits.push((lighting, ustr("ExposureCompensation"), Variant::Float32(exp)));
        }
        if ui.add(egui::Slider::new(&mut fog_end, 40.0..=100000.0).logarithmic(true).text("FogEnd")).changed() {
            edits.push((lighting, ustr("FogEnd"), Variant::Float32(fog_end)));
        }
        if ui.checkbox(&mut shadows, "GlobalShadows").changed() {
            edits.push((lighting, ustr("GlobalShadows"), Variant::Bool(shadows)));
        }
        if !edits.is_empty() && self.game.play.is_none() {
            self.game.doc.set_props("Adjust Lighting", edits, Some("lighting_slider".into()));
        }

        ui.separator();
        ui.label(RichText::new("Add PostFX to Lighting").strong());
        ui.horizontal_wrapped(|ui| {
            for cls in ["Atmosphere", "BloomEffect", "ColorCorrectionEffect", "DepthOfFieldEffect", "SunRaysEffect", "BlurEffect", "Sky", "Clouds"] {
                if ui.small_button(format!("+{cls}")).clicked() && self.game.play.is_none() {
                    let r = self.game.doc.insert(lighting, InstanceBuilder::new(cls).with_name(cls));
                    self.select(vec![r]);
                    self.extras.status = Some(format!("Added {cls} under Lighting"));
                }
            }
        });
    }

    fn studio_vfx_tab(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("One-Click VFX & Light Prefabs").strong());
        ui.label(RichText::new("Inserts into the selected Part (or creates an anchored emitter Part in Workspace).").small().color(theme::TEXT_DIM));
        let target_part = self
            .game
            .doc
            .selection
            .first()
            .copied()
            .filter(|r| self.game.doc.class_of(*r).is_some_and(|c| doc::is_a(c.as_str(), "BasePart")));

        let mut spawn_vfx: Option<&str> = None;
        ui.horizontal_wrapped(|ui| {
            for label in ["Fire", "Smoke", "Sparks", "Magic Aura", "PointLight", "SpotLight", "Neon Beam", "Trail", "Highlight"] {
                if ui.button(label).clicked() && self.game.play.is_none() {
                    spawn_vfx = Some(label);
                }
            }
        });

        if let Some(kind) = spawn_vfx {
            let parent = if let Some(p) = target_part {
                p
            } else if let Some(ws) = self.game.doc.workspace() {
                let at = self.game.cam.pos + self.game.cam.forward() * 14.0;
                self.game.doc.insert(ws, quick_part(&format!("{kind}Emitter"), [at.x, at.y, at.z], [2.0, 2.0, 2.0], [40, 44, 52], 272))
            } else {
                return;
            };
            let child = match kind {
                "Fire" => InstanceBuilder::new("Fire")
                    .with_name("Fire")
                    .with_property("Heat", 12.0f32)
                    .with_property("Size", 6.0f32)
                    .with_property("Color", Color3::new(1.0, 0.55, 0.1)),
                "Smoke" => InstanceBuilder::new("Smoke")
                    .with_name("Smoke")
                    .with_property("Opacity", 0.45f32)
                    .with_property("RiseVelocity", 4.0f32)
                    .with_property("Size", 3.5f32),
                "Sparks" => InstanceBuilder::new("ParticleEmitter")
                    .with_name("Sparks")
                    .with_property("Rate", 45.0f32)
                    .with_property("LightEmission", 1.0f32)
                    .with_property("SpreadAngle", Vector2::new(60.0, 60.0)),
                "Magic Aura" => InstanceBuilder::new("ParticleEmitter")
                    .with_name("MagicAura")
                    .with_property("Rate", 30.0f32)
                    .with_property("LightEmission", 0.9f32)
                    .with_property("SpreadAngle", Vector2::new(180.0, 180.0)),
                "PointLight" => InstanceBuilder::new("PointLight")
                    .with_name("PointLight")
                    .with_property("Brightness", 2.5f32)
                    .with_property("Range", 24.0f32)
                    .with_property("Shadows", true),
                "SpotLight" => InstanceBuilder::new("SpotLight")
                    .with_name("SpotLight")
                    .with_property("Brightness", 3.0f32)
                    .with_property("Range", 32.0f32)
                    .with_property("Angle", 60.0f32)
                    .with_property("Shadows", true),
                "Neon Beam" => InstanceBuilder::new("Beam")
                    .with_name("NeonBeam")
                    .with_property("LightEmission", 1.0f32)
                    .with_property("Width0", 0.6f32)
                    .with_property("Width1", 0.6f32),
                "Trail" => InstanceBuilder::new("Trail")
                    .with_name("SpeedTrail")
                    .with_property("LightEmission", 0.8f32)
                    .with_property("Lifetime", 0.4f32),
                _ => InstanceBuilder::new("Highlight")
                    .with_name("Highlight")
                    .with_property("FillColor", Color3::new(0.2, 0.7, 1.0))
                    .with_property("FillTransparency", 0.55f32)
                    .with_property("OutlineColor", Color3::new(1.0, 1.0, 1.0)),
            };
            let r = self.game.doc.insert(parent, child);
            self.select(vec![r]);
            self.extras.status = Some(format!("Added {kind} VFX"));
        }

        // Live sliders if a ParticleEmitter or Light is selected
        if let Some(&sel) = self.game.doc.selection.first() {
            let cls = self.game.doc.class_of(sel).map(|c| c.to_string()).unwrap_or_default();
            if cls == "ParticleEmitter" {
                ui.separator();
                ui.label(RichText::new("Selected ParticleEmitter").strong());
                let mut rate = doc::f32_of(self.game.doc.prop(sel, "Rate")).unwrap_or(20.0);
                let mut le = doc::f32_of(self.game.doc.prop(sel, "LightEmission")).unwrap_or(0.0);
                let mut drag = doc::f32_of(self.game.doc.prop(sel, "Drag")).unwrap_or(0.0);
                let mut edits = Vec::new();
                if ui.add(egui::Slider::new(&mut rate, 0.0..=300.0).text("Rate")).changed() {
                    edits.push((sel, ustr("Rate"), Variant::Float32(rate)));
                }
                if ui.add(egui::Slider::new(&mut le, 0.0..=1.0).text("LightEmission")).changed() {
                    edits.push((sel, ustr("LightEmission"), Variant::Float32(le)));
                }
                if ui.add(egui::Slider::new(&mut drag, 0.0..=20.0).text("Drag")).changed() {
                    edits.push((sel, ustr("Drag"), Variant::Float32(drag)));
                }
                if !edits.is_empty() && self.game.play.is_none() {
                    self.game.doc.set_props("Edit ParticleEmitter", edits, Some("vfx_slider".into()));
                }
            } else if matches!(cls.as_str(), "PointLight" | "SpotLight" | "SurfaceLight") {
                ui.separator();
                ui.label(RichText::new(format!("Selected {cls}")).strong());
                let mut br = doc::f32_of(self.game.doc.prop(sel, "Brightness")).unwrap_or(1.0);
                let mut range = doc::f32_of(self.game.doc.prop(sel, "Range")).unwrap_or(16.0);
                let mut sh = matches!(self.game.doc.prop(sel, "Shadows"), Some(Variant::Bool(true)));
                let mut edits = Vec::new();
                if ui.add(egui::Slider::new(&mut br, 0.0..=10.0).text("Brightness")).changed() {
                    edits.push((sel, ustr("Brightness"), Variant::Float32(br)));
                }
                if ui.add(egui::Slider::new(&mut range, 1.0..=60.0).text("Range")).changed() {
                    edits.push((sel, ustr("Range"), Variant::Float32(range)));
                }
                if ui.checkbox(&mut sh, "Shadows").changed() {
                    edits.push((sel, ustr("Shadows"), Variant::Bool(sh)));
                }
                if !edits.is_empty() && self.game.play.is_none() {
                    self.game.doc.set_props("Edit Light", edits, Some("light_slider".into()));
                }
            }
        }
    }

    fn studio_device_tab(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Device Emulator (GUI & Viewport Resolution)").strong());
        ui.label(RichText::new("Simulate phone, tablet, or desktop aspect ratios to verify ScreenGui scaling.").small().color(theme::TEXT_DIM));
        ui.add_space(4.0);
        for (idx, &(label, size)) in EMU_PRESETS.iter().enumerate() {
            if ui.selectable_label(self.extras.emu_preset == idx, label).clicked() {
                self.extras.emu_preset = idx;
                self.game.view_render_size = size;
                self.extras.status = Some(format!("Viewport set to {label}"));
            }
        }
        ui.separator();
        ui.checkbox(&mut self.extras.show_perf_hud, "Show Live FPS / Performance HUD Overlay");
    }

    fn studio_fast_games_tab(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("1-Click Playable Game Starters").strong());
        ui.label(RichText::new("Spawns map geometry, lighting, HUD & working Luau scripts straight into the open place.").small().color(theme::TEXT_DIM));
        let mut spawn_tpl: Option<&str> = None;
        ui.horizontal_wrapped(|ui| {
            for &(id, lbl) in &[
                ("fps_arena", "🔫 FPS Arena"),
                ("obby", "🏃 Obby Course"),
                ("horror_chase", "👻 Horror Chase"),
                ("combat_arena", "⚔ Combat Arena"),
                ("tycoon", "💰 Tycoon Starter"),
            ] {
                if ui.button(lbl).clicked() && self.game.play.is_none() {
                    spawn_tpl = Some(id);
                }
            }
        });
        if let Some(tpl) = spawn_tpl {
            self.spawn_quick_template(tpl);
        }

        ui.separator();
        ui.label(RichText::new("Ready-to-Run Luau Game Systems (1-Click Insert)").strong());
        for sp in SCRIPT_LIBRARY {
            ui.horizontal(|ui| {
                if ui.small_button("+ Add").clicked() && self.game.play.is_none() {
                    let parent = self
                        .game
                        .doc
                        .find_child(self.game.doc.dom.root_ref(), sp.service)
                        .or_else(|| self.game.doc.workspace())
                        .unwrap_or_else(|| self.game.doc.dom.root_ref());
                    let b = InstanceBuilder::new(sp.class)
                        .with_name(sp.name)
                        .with_property("Source", Variant::String(sp.source.into()));
                    let r = self.game.doc.insert(parent, b);
                    self.open_script.push(r);
                    self.select(vec![r]);
                    self.extras.status = Some(format!("Inserted {} into {}", sp.name, sp.service));
                }
                ui.vertical(|ui| {
                    ui.label(RichText::new(sp.title).strong());
                    ui.label(RichText::new(sp.summary).small().color(theme::TEXT_DIM));
                });
            });
            ui.add_space(2.0);
        }
    }

    fn studio_build_tools_tab(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Quick Rig & NPC Spawner").strong());
        ui.horizontal_wrapped(|ui| {
            for &(label, r6, ai) in &[
                ("Spawn R15 Rig", false, false),
                ("Spawn R6 Rig", true, false),
                ("Spawn Chase NPC", false, true),
            ] {
                if ui.button(label).clicked() && self.game.play.is_none() {
                    if let Some(ws) = self.game.doc.workspace() {
                        let rig_ty = if r6 { RigType::R6 } else { RigType::R15 };
                        let mut b = rig::build_default(
                            if ai { "ChaseNPC" } else if r6 { "RigR6" } else { "RigR15" },
                            rig_ty,
                            &BodyColors::default(),
                            &Scales::default(),
                            InstanceBuilder::new("Humanoid"),
                        );
                        if ai {
                            b.add_child(
                                InstanceBuilder::new("Script")
                                    .with_name("ChaseAI")
                                    .with_property("Source", Variant::String(SCRIPT_LIBRARY[3].source.into())),
                            );
                        }
                        let r = self.game.doc.insert(ws, b);
                        self.select(vec![r]);
                        self.extras.status = Some(format!("Spawned {label}"));
                    }
                }
            }
        });

        ui.separator();
        ui.label(RichText::new("Selection Transform & Grid Array Tools").strong());
        if self.extras.grid_cols <= 0 {
            self.extras.grid_cols = 4;
            self.extras.grid_spacing = 8.0;
        }
        ui.horizontal(|ui| {
            ui.label("Count:");
            ui.add(egui::DragValue::new(&mut self.extras.grid_cols).range(2..=32));
            ui.label("Spacing:");
            ui.add(egui::DragValue::new(&mut self.extras.grid_spacing).speed(0.5).range(1.0..=100.0).suffix(" studs"));
            if ui.button("Duplicate Row").clicked() && self.game.play.is_none() && !self.game.doc.selection.is_empty() {
                let sel = self.game.doc.selection.clone();
                if let Some(bytes) = self.game.doc.copy(&sel) {
                    let parent = self
                        .game
                        .doc
                        .dom
                        .get_by_ref(sel[0])
                        .map(|i| i.parent())
                        .or_else(|| self.game.doc.workspace())
                        .unwrap_or_else(|| self.game.doc.dom.root_ref());
                    let step = self.extras.grid_spacing;
                    for i in 1..self.extras.grid_cols {
                        let pasted = self.game.doc.paste(&bytes, parent, "Duplicate Row");
                        let dx = i as f32 * step;
                        let mut edits = Vec::new();
                        for root in &pasted {
                            for r in self.game.doc.subtree(*root) {
                                if let Some(Variant::CFrame(mut cf)) = self.game.doc.prop(r, "CFrame") {
                                    cf.position.x += dx;
                                    edits.push((r, ustr("CFrame"), Variant::CFrame(cf)));
                                }
                            }
                        }
                        if !edits.is_empty() {
                            self.game.doc.set_props("Offset Row", edits, None);
                        }
                    }
                    self.extras.status = Some(format!("Duplicated row of {} copies", self.extras.grid_cols));
                }
            }
        });

        ui.horizontal_wrapped(|ui| {
            if ui.button("Align Y (Floor)").clicked() && self.game.play.is_none() && self.game.doc.selection.len() >= 2 {
                let mut min_y = f32::INFINITY;
                let parts: Vec<Ref> = self
                    .game
                    .doc
                    .selection
                    .iter()
                    .copied()
                    .filter(|r| self.game.doc.class_of(*r).is_some_and(|c| doc::is_a(c.as_str(), "BasePart")))
                    .collect();
                for &r in &parts {
                    if let (Some(Variant::CFrame(cf)), Some(Variant::Vector3(sz))) = (self.game.doc.prop(r, "CFrame"), self.game.doc.prop(r, "Size")) {
                        min_y = min_y.min(cf.position.y - sz.y * 0.5);
                    }
                }
                if min_y.is_finite() {
                    let mut edits = Vec::new();
                    for r in parts {
                        if let (Some(Variant::CFrame(mut cf)), Some(Variant::Vector3(sz))) = (self.game.doc.prop(r, "CFrame"), self.game.doc.prop(r, "Size")) {
                            cf.position.y = min_y + sz.y * 0.5;
                            edits.push((r, ustr("CFrame"), Variant::CFrame(cf)));
                        }
                    }
                    self.game.doc.set_props("Align Floor Y", edits, None);
                    self.extras.status = Some("Aligned selected parts to lowest floor Y".into());
                }
            }
            if ui.button("Drop to Y=0").clicked() && self.game.play.is_none() {
                let mut edits = Vec::new();
                for &r in &self.game.doc.selection {
                    if let (Some(Variant::CFrame(mut cf)), Some(Variant::Vector3(sz))) = (self.game.doc.prop(r, "CFrame"), self.game.doc.prop(r, "Size")) {
                        cf.position.y = sz.y * 0.5;
                        edits.push((r, ustr("CFrame"), Variant::CFrame(cf)));
                    }
                }
                if !edits.is_empty() {
                    self.game.doc.set_props("Drop to Ground", edits, None);
                    self.extras.status = Some("Dropped selection to Y=0".into());
                }
            }
            if ui.button("Toggle Perf HUD").clicked() {
                self.extras.show_perf_hud = !self.extras.show_perf_hud;
            }
        });
    }

    fn spawn_quick_template(&mut self, tpl: &str) {
        let Some(ws) = self.game.doc.workspace() else { return };
        let sss = self.game.doc.find_child(self.game.doc.dom.root_ref(), "ServerScriptService").unwrap_or(ws);
        let sgui = self.game.doc.find_child(self.game.doc.dom.root_ref(), "StarterGui").unwrap_or(ws);
        match tpl {
            "obby" => {
                apply_lighting_preset_local(&mut self.game.doc, "Daylight");
                let mut folder = InstanceBuilder::new("Folder").with_name("ObbyCourse");
                folder.add_child(
                    InstanceBuilder::new("SpawnLocation")
                        .with_name("ObbySpawn")
                        .with_property("Anchored", true)
                        .with_property("Size", Vector3::new(14.0, 2.0, 14.0))
                        .with_property("CFrame", cf_at([0.0, 1.0, 0.0]))
                        .with_property("Color", Color3uint8::new(66, 179, 98)),
                );
                let palette = [[235, 87, 87], [242, 153, 74], [242, 201, 76], [39, 174, 96], [47, 128, 237], [155, 81, 224]];
                for i in 1..=12 {
                    let x = ((i as f32) * 0.85).sin() * 8.0;
                    let y = 2.0 + (i as f32) * 1.4;
                    let z = -(i as f32) * 13.5;
                    folder.add_child(quick_part(&format!("Stage_{i}"), [x, y, z], [8.0, 1.5, 8.0], palette[i % palette.len()], 256));
                }
                self.game.doc.insert(ws, folder);
                self.game.doc.insert(sss, InstanceBuilder::new("Script").with_name("Leaderstats").with_property("Source", Variant::String(SCRIPT_LIBRARY[0].source.into())));
                self.extras.status = Some("Spawned Obby starter course!".into());
            }
            "fps_arena" => {
                apply_lighting_preset_local(&mut self.game.doc, "Sci-Fi");
                let mut arena = InstanceBuilder::new("Folder").with_name("FPSArena");
                arena.add_child(quick_part("Floor", [0.0, -1.0, 0.0], [140.0, 2.0, 140.0], [52, 60, 70], 816));
                for (i, &(x, z)) in [(-24.0, -20.0), (24.0, -20.0), (-24.0, 20.0), (24.0, 20.0), (0.0, 0.0)].iter().enumerate() {
                    arena.add_child(quick_part(&format!("Cover_{i}"), [x, 4.0, z], [12.0, 8.0, 6.0], [80, 92, 108], 1088));
                    arena.add_child(quick_part(&format!("Trim_{i}"), [x, 8.3, z], [12.2, 0.4, 6.2], [0, 220, 255], 288));
                }
                self.game.doc.insert(ws, arena);
                self.game.doc.insert(sss, InstanceBuilder::new("Script").with_name("GunSystemServer").with_property("Source", Variant::String(SCRIPT_LIBRARY[8].source.into())));
                self.game.doc.insert(sgui, InstanceBuilder::new("LocalScript").with_name("SprintStamina").with_property("Source", Variant::String(SCRIPT_LIBRARY[1].source.into())));
                self.extras.status = Some("Spawned FPS Arena starter!".into());
            }
            "horror_chase" => {
                apply_lighting_preset_local(&mut self.game.doc, "Horror");
                let mut map = InstanceBuilder::new("Folder").with_name("HorrorFacility");
                map.add_child(quick_part("Floor", [0.0, -1.0, 0.0], [110.0, 2.0, 110.0], [30, 32, 34], 816));
                map.add_child(quick_part("Ceiling", [0.0, 14.0, 0.0], [110.0, 2.0, 110.0], [20, 22, 24], 816));
                map.add_child(quick_part("HallA", [-16.0, 6.5, -10.0], [2.0, 15.0, 64.0], [42, 44, 46], 816));
                map.add_child(quick_part("HallB", [16.0, 6.5, 10.0], [2.0, 15.0, 64.0], [42, 44, 46], 816));
                let mut lamp = quick_part("Lamp", [0.0, 12.7, 0.0], [4.0, 0.4, 2.0], [255, 200, 140], 288);
                lamp.add_child(InstanceBuilder::new("PointLight").with_name("Light").with_property("Brightness", 2.0f32).with_property("Range", 38.0f32));
                map.add_child(lamp);
                self.game.doc.insert(ws, map);
                let mut npc = rig::build_default("Stalker", RigType::R15, &BodyColors::default(), &Scales::default(), InstanceBuilder::new("Humanoid"));
                npc.add_child(InstanceBuilder::new("Script").with_name("PathfindingChaseAI").with_property("Source", Variant::String(SCRIPT_LIBRARY[3].source.into())));
                self.game.doc.insert(ws, npc);
                self.extras.status = Some("Spawned Horror Chase starter with Pathfinding Stalker!".into());
            }
            "combat_arena" => {
                apply_lighting_preset_local(&mut self.game.doc, "Cyberpunk");
                let mut map = InstanceBuilder::new("Folder").with_name("CombatArena");
                map.add_child(quick_part("ArenaFloor", [0.0, -1.0, 0.0], [100.0, 2.0, 100.0], [28, 24, 42], 1280));
                map.add_child(quick_part("NeonRing", [0.0, 0.05, 0.0], [30.0, 0.2, 30.0], [255, 45, 175], 288));
                self.game.doc.insert(ws, map);
                self.game.doc.insert(sgui, InstanceBuilder::new("LocalScript").with_name("MovementAbilities").with_property("Source", Variant::String(SCRIPT_LIBRARY[2].source.into())));
                self.extras.status = Some("Spawned Cyber Combat Arena with Double Jump + Dash!".into());
            }
            _ => {
                apply_lighting_preset_local(&mut self.game.doc, "Sunset");
                let mut map = InstanceBuilder::new("Folder").with_name("TycoonPlot");
                map.add_child(quick_part("PlotFloor", [0.0, 0.0, 0.0], [64.0, 1.0, 64.0], [185, 190, 196], 816));
                map.add_child(quick_part("Conveyor", [0.0, 1.0, -8.0], [6.0, 1.0, 30.0], [45, 48, 54], 1088));
                map.add_child(quick_part("Dropper", [0.0, 5.5, -22.0], [5.0, 4.0, 5.0], [235, 168, 45], 1088));
                map.add_child(quick_part("Collector", [0.0, 1.8, 8.0], [8.0, 2.6, 4.0], [52, 211, 153], 288));
                self.game.doc.insert(ws, map);
                self.game.doc.insert(sss, InstanceBuilder::new("Script").with_name("Leaderstats").with_property("Source", Variant::String(SCRIPT_LIBRARY[0].source.into())));
                self.game.doc.insert(sgui, InstanceBuilder::new("LocalScript").with_name("QuickShopGui").with_property("Source", Variant::String(SCRIPT_LIBRARY[9].source.into())));
                self.extras.status = Some("Spawned Tycoon Starter with Leaderstats + Shop GUI!".into());
            }
        }
    }
}
