-- harness (command-bar identity) -> a real server Script so the probe shares the game's module instances.
-- Fires one Infernal-Blades-sized voxel hit (90x90x90, voxel 6) at the player's feet, then logs
-- how many pooled voxel parts are loose / anchored and the client's frame cost markers.
local s = Instance.new("Script")
s.Name = "HarnessVoxel"
s.Source = [==[
local Players = game:GetService("Players")
local p = Players:GetPlayers()[1]
if not p then warn("[probe] no player") return end
local c = p.Character or p.CharacterAdded:Wait()
if not c.PrimaryPart then c:GetPropertyChangedSignal("PrimaryPart"):Wait() end
local hum = c:FindFirstChildWhichIsA("Humanoid")
local Utils = require(game.ServerScriptService.Modules.Utility.Utils)
local function count()
  local pool = workspace:FindFirstChild("Caches") and workspace.Caches:FindFirstChild("Parts")
  local loose, anch, total = 0, 0, 0
  if pool then
    for _, d in pool:GetDescendants() do
      if d:IsA("BasePart") then
        total += 1
        if d.Anchored then anch += 1 else loose += 1 end
      end
    end
  end
  local inws = 0
  for _, d in workspace:GetDescendants() do if d:IsA("BasePart") then inws += 1 end end
  return loose, anch, total, inws
end
local l0, a0, t0, w0 = count()
print(string.format("[probe] before: pool loose=%d anchored=%d total=%d workspace parts=%d", l0, a0, t0, w0))
local CS = game:GetService("CollectionService")
local tagged = CS:GetTagged("VoxelOrigin")
local vf = workspace.Map:FindFirstChild("Destroyables") and workspace.Map.Destroyables:FindFirstChild("Voxel")
local nvf = 0
if vf then for _, d in vf:GetDescendants() do if d:IsA("BasePart") then nvf += 1 end end end
local best, bestd = nil, math.huge
for _, part in tagged do
  local d = (part.Position - c.PrimaryPart.Position).Magnitude
  if d < bestd then best, bestd = part, d end
end
local homes, sizes = 0, 0
for _, part in tagged do
  if typeof(part:GetAttribute("VoxelHome")) == "CFrame" then homes += 1 end
  if typeof(part:GetAttribute("VoxelSize")) == "Vector3" then sizes += 1 end
end
local okv, vox = pcall(require, game.ServerScriptService.Framework.Core.Voxel)
local keys = {}
if okv and type(vox) == "table" then for k in vox do keys[#keys + 1] = tostring(k) end end
print(string.format("[probe] origins with VoxelHome=%d VoxelSize=%d; require(Core.Voxel) ok=%s type=%s keys=%s; VoxelReliable=%s VoxelRequest=%s", homes, sizes, tostring(okv), typeof(vox), table.concat(keys, ","), tostring(game.ReplicatedStorage:FindFirstChild("VoxelReliable")), tostring(game.ReplicatedStorage:FindFirstChild("VoxelRequest"))))
local t0 = os.clock()
local okc, errc = pcall(function()
  local p = Instance.new("Part") p:SetAttribute("H", CFrame.new(1, 2, 3)) p:SetAttribute("S", Vector3.new(1, 2, 3))
  return typeof(p:GetAttribute("H")) .. "/" .. typeof(p:GetAttribute("S"))
end)
print("[probe] CFrame/Vector3 attributes:", okc, errc)
print(string.format("[probe] VoxelOrigin tagged=%d, Map.Destroyables.Voxel parts=%d, nearest origin %s at %.1f studs (%s)", #tagged, nvf, best and best:GetFullName() or "nil", bestd, best and tostring(best.Position) or ""))
-- stand on the nearest voxel origin so the hit has something to break
local pos = c.PrimaryPart.Position
if best then
  pos = best.Position + Vector3.new(0, best.Size.Y * 0.5 + 3, 0)
  c:PivotTo(CFrame.new(pos))
  task.wait(0.5)
  pos = c.PrimaryPart.Position
end
local t = os.clock()
local ok, err = pcall(Utils.VoxelHitbox, CFrame.new(pos), vector.create(90, 90, 90), hum, 6, vector.create(0, 1, 0))
print(string.format("[probe] VoxelHitbox(90^3) at %s ok=%s err=%s took %.1f ms (server side)", tostring(pos), tostring(ok), tostring(err), (os.clock() - t) * 1000))
for i = 1, 16 do
  task.wait(0.5)
  local l, a, tt, w = count()
  print(string.format("[probe] +%.1fs pool loose=%d anchored=%d total=%d workspace parts=%d", i * 0.5, l, a, tt, w))
end
]==]
s.Parent = game.ServerScriptService
