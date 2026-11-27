-- server: the cost of property writes from Lua, on 4000 fresh anchored parts (os.clock around each batch).
local folder = Instance.new("Folder"); folder.Name = "PropSetProbe"; folder.Parent = workspace
local parts = {}
local t0 = os.clock()
for i = 1, 4000 do
  local p = Instance.new("Part"); p.Anchored = true; p.Size = Vector3.new(2, 2, 2)
  p.CFrame = CFrame.new((i % 64) * 3, -50 + math.floor(i / 64) * 3, 300)
  p.Parent = folder; parts[i] = p
end
print(string.format("[propset] made 4000 parts in %.1f ms", (os.clock() - t0) * 1000))
task.wait(1)
local function bench(label, f)
  local t = os.clock()
  for i = 1, #parts do f(parts[i], i) end
  local dt = (os.clock() - t) * 1000
  print(string.format("[propset] %-28s %7.1f ms  (%.1f us/write)", label, dt, dt * 1000 / #parts))
  task.wait(0.5)
end
bench("CollisionGroup=Default", function(p) p.CollisionGroup = "Default" end)
bench("CollisionGroup=Nothing", function(p) p.CollisionGroup = "Nothing" end)
bench("CanCollide=false", function(p) p.CanCollide = false end)
bench("Transparency=0.5", function(p) p.Transparency = 0.5 end)
bench("Color=random", function(p, i) p.Color = Color3.fromRGB(i % 255, 100, 100) end)
bench("CFrame=+0.01", function(p) p.CFrame = p.CFrame + Vector3.new(0, 0.01, 0) end)
bench("Name=x", function(p, i) p.Name = "P" .. i end)
bench("SetAttribute(k,i)", function(p, i) p:SetAttribute("k", i) end)
bench("Anchored=true (no change)", function(p) p.Anchored = true end)
bench("CanQuery=false", function(p) p.CanQuery = false end)
bench("read .CFrame", function(p) local _ = p.CFrame end)
print("[propset] done")
