-- server: a spawn burst like a crater + VFX storm (physics debris, anchored pieces, short-lived VFX
-- parts with emitters), near the player; a [burst] line a second with the awake count.
-- Env knobs through attributes are not available to a chunk; edit the counts here.
local RS = game:GetService("RunService")
local Players = game:GetService("Players")
local DEBRIS, ANCHORED, VFX_PER_FRAME, VFX_LIFE, SECONDS = 400, 2000, 10, 0.5, 14

local function origin()
  local p = Players:GetPlayers()[1]
  local c = p and p.Character
  local hrp = c and c:FindFirstChild("HumanoidRootPart")
  return hrp and hrp.Position or Vector3.new(0, 50, 0)
end
local folder = Instance.new("Folder")
folder.Name = "BurstProbe"
folder.Parent = workspace
local o = origin()
print(string.format("[burst] origin %.0f %.0f %.0f at os.clock %.2f", o.X, o.Y, o.Z, os.clock()))

local t0 = os.clock()
local debris = {}
for i = 1, DEBRIS do
  local p = Instance.new("Part")
  p.Size = Vector3.new(1 + math.random() * 2, 1 + math.random() * 2, 1 + math.random() * 2)
  p.CFrame = CFrame.new(o + Vector3.new((math.random() - 0.5) * 30, 10 + math.random() * 30, (math.random() - 0.5) * 30))
    * CFrame.Angles(math.random() * 3, math.random() * 3, 0)
  p.Color = Color3.fromHSV(math.random(), 0.6, 0.8)
  p.Material = Enum.Material.Concrete
  p.Anchored = false
  p.CanCollide = true
  p.AssemblyLinearVelocity = Vector3.new((math.random() - 0.5) * 60, math.random() * 40, (math.random() - 0.5) * 60)
  p.Parent = folder
  debris[i] = p
end
local t1 = os.clock()
for i = 1, ANCHORED do
  local p = Instance.new("Part")
  p.Size = Vector3.new(2, 2, 2)
  p.Anchored = true
  p.Material = Enum.Material.Slate
  p.CFrame = CFrame.new(o + Vector3.new((i % 50) * 2 - 50, -3 + (math.floor(i / 50) % 4) * 2, math.floor(i / 200) * 2 + 25))
  p.Parent = folder
end
local t2 = os.clock()
print(string.format("[burst] spawned %d debris in %.1f ms, %d anchored in %.1f ms", DEBRIS, (t1 - t0) * 1000, ANCHORED, (t2 - t1) * 1000))

-- VFX churn: every Heartbeat VFX_PER_FRAME short-lived parts with an emitter and an attachment.
local n, vfx = 0, {}
local conn
conn = RS.Heartbeat:Connect(function()
  n += 1
  for _ = 1, VFX_PER_FRAME do
    local p = Instance.new("Part")
    p.Size = Vector3.new(0.5, 0.5, 0.5)
    p.Anchored = true
    p.CanCollide = false
    p.Transparency = 0.5
    p.CFrame = CFrame.new(o + Vector3.new((math.random() - 0.5) * 20, 5 + math.random() * 10, (math.random() - 0.5) * 20))
    local e = Instance.new("ParticleEmitter")
    e.Rate = 20
    e.Lifetime = NumberRange.new(0.5)
    e.Parent = p
    local a = Instance.new("Attachment")
    a.Parent = p
    p.Parent = folder
    table.insert(vfx, { p, os.clock() + VFX_LIFE })
  end
  while vfx[1] and vfx[1][2] < os.clock() do
    table.remove(vfx, 1)[1]:Destroy()
  end
end)

task.spawn(function()
  for s = 1, SECONDS do
    task.wait(1)
    local awake, gone = 0, 0
    for _, p in ipairs(debris) do
      if not p.Parent then
        gone += 1
      elseif p.AssemblyLinearVelocity.Magnitude > 0.5 then
        awake += 1
      end
    end
    print(string.format("[burst] t=%d awake=%d gone=%d vfx_live=%d vfx_made=%d heartbeats=%d", s, awake, gone, #vfx, n * VFX_PER_FRAME, n))
  end
  conn:Disconnect()
  for _, v in ipairs(vfx) do v[1]:Destroy() end
  print("[burst] done")
end)
