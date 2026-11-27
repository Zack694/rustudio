-- client: fps + instance count every second
local RunService = game:GetService("RunService")
local frames, last, t0 = 0, os.clock(), os.clock()
RunService.RenderStepped:Connect(function() frames += 1 end)
task.spawn(function()
  for i = 1, 40 do
    task.wait(1)
    local fps = frames / (os.clock() - last); frames = 0; last = os.clock()
    local bp = workspace:FindFirstChild("BurstProbe")
    print(string.format("[watch %.1f] fps=%.0f burst_children=%d", os.clock() - t0, fps, bp and #bp:GetChildren() or -1))
  end
end)
