-- rustudio engine library: ContextActionService's touch buttons when
-- Roblox's CoreScripts are not running — what their ContextActionTouch
-- CoreScript (StarterScript runs it on a touch screen) draws for each
-- action bound with createTouchButton, built the way it builds them
-- (Roblox-Client-Tracker scripts/CoreScripts/CoreScripts/ContextActionTouch.lua):
--
--   PlayerGui.ContextActionGui (ResetOnSpawn false)
--     ContextButtonFrame          Frame, 30 % × 50 % of the screen from (70 %, 50 %)
--       ContextActionButton       ImageButton 45 × 45 (35 × 35 on a screen
--                                 320 tall or less), Active, at one of seven
--                                 places in the frame; the round grey
--                                 button image, pressed while a finger is on it
--         ActionIcon              ImageLabel 65 % of the button, SetImage's image
--         ActionTitle             TextLabel over the button, SetTitle's text
--                                 (SourceSansBold 18, white, stroked, wrapped)
--
-- A finger landing on a button runs the action's handler with
-- UserInputState.Begin, its moves with Change, its lift with End, and a
-- lift after it dragged off the button with Cancel (Roblox's AWAY state),
-- through ContextActionService:CallFunction (RobloxScriptSecurity;
-- library.luau), the handler getting the finger's own InputObject.
-- BoundActionAdded / Removed / Changed add, take away and restyle the
-- buttons (SetImage / SetTitle / SetPosition; the description has no
-- place in Roblox's either), GetActionButtonEvent is answered with
-- FireActionButtonFoundSignal, so GetButton gets the button — and games
-- that restyle it (its ActionIcon / ActionTitle, its Position) find what
-- they find in Roblox. The frame shows only while the player's preferred
-- input is touch and GuiService.TouchControlsEnabled, as Roblox's.
--
-- Started by gui_input.luau (start_touch_buttons) in Play on a touch
-- screen once the LocalPlayer's PlayerGui is there, with the CoreScripts
-- off; with them on, theirs does all this.
local playerGui = ...

local CAS = game:GetService("ContextActionService")
local UIS = game:GetService("UserInputService")
local GuiService = game:GetService("GuiService")

local UP_IMAGE = "https://www.roblox.com/asset/?id=97166444"
local DOWN_IMAGE = "https://www.roblox.com/asset/?id=97166756"
-- Where the buttons go in the frame, first free place first.
local PLACES = {
    UDim2.new(0, 123, 0, 70),
    UDim2.new(0, 30, 0, 60),
    UDim2.new(0, 180, 0, 160),
    UDim2.new(0, 85, 0, -25),
    UDim2.new(0, 185, 0, -25),
    UDim2.new(0, 185, 0, 260),
    UDim2.new(0, 216, 0, 65),
}

local entries = {} -- action name → { info = table, button = ImageButton?, place = n? }
local places = {} -- place → action name
local gui, frame = nil, nil

local function ensure_gui()
    if gui then return end
    gui = Instance.new("ScreenGui")
    gui.Name = "ContextActionGui"
    gui.ResetOnSpawn = false
    frame = Instance.new("Frame")
    frame.Name = "ContextButtonFrame"
    frame.BackgroundTransparency = 1
    frame.Size = UDim2.new(0.3, 0, 0.5, 0)
    frame.Position = UDim2.new(0.7, 0, 0.5, 0)
    frame.Parent = gui
    local function update_visible()
        local ok_pref, pref = pcall(function() return UIS.PreferredInput end)
        local ok_on, on = pcall(function() return GuiService.TouchControlsEnabled end)
        frame.Visible = (not ok_pref or pref == Enum.PreferredInput.Touch) and (not ok_on or on ~= false)
    end
    pcall(function() GuiService:GetPropertyChangedSignal("TouchControlsEnabled"):Connect(update_visible) end)
    pcall(function() UIS:GetPropertyChangedSignal("PreferredInput"):Connect(update_visible) end)
    update_visible()
end

local function is_touch(io)
    return io.UserInputType == Enum.UserInputType.Touch
end

local function call(name, state, io)
    if not entries[name] then return end
    local ok, err = pcall(CAS.CallFunction, CAS, name, state, io)
    if not ok then
        warn("ContextActionService touch button " .. name .. ": " .. tostring(err))
    end
end

local function small_screen()
    local ok, res = pcall(function() return GuiService:GetScreenResolution() end)
    return ok and typeof(res) == "Vector2" and res.Y <= 320
end

local function new_button(name, info)
    local b = Instance.new("ImageButton")
    b.Name = "ContextActionButton"
    b.BackgroundTransparency = 1
    local px = if small_screen() then 35 else 45
    b.Size = UDim2.new(0, px, 0, px)
    b.Active = true
    b.Image = UP_IMAGE

    -- The finger on the button, if any. OFF → ON on its landing (Begin),
    -- ON → OFF on its lift from the button (End); a lift anywhere else —
    -- it dragged off first — is Roblox's AWAY → OFF (Cancel), heard from
    -- UserInputService alone, since the button no longer has the finger.
    local finger = nil
    local function release(io, state)
        finger = nil
        b.Image = UP_IMAGE
        call(name, state, io)
    end
    b.InputBegan:Connect(function(io)
        if not is_touch(io) or io.UserInputState ~= Enum.UserInputState.Begin then return end
        if finger ~= nil and finger ~= io then return end
        finger = io
        b.Image = DOWN_IMAGE
        call(name, Enum.UserInputState.Begin, io)
    end)
    b.InputChanged:Connect(function(io)
        if not is_touch(io) or finger ~= io then return end
        b.Image = DOWN_IMAGE
        call(name, Enum.UserInputState.Change, io)
    end)
    b.InputEnded:Connect(function(io)
        if not is_touch(io) or finger ~= io then return end
        if io.UserInputState == Enum.UserInputState.End then
            release(io, Enum.UserInputState.End)
        elseif io.UserInputState == Enum.UserInputState.Cancel then
            release(io, Enum.UserInputState.Cancel)
        end
    end)
    UIS.InputEnded:Connect(function(io)
        if not is_touch(io) or finger ~= io then return end
        release(io, Enum.UserInputState.Cancel)
    end)

    local icon = Instance.new("ImageLabel")
    icon.Name = "ActionIcon"
    icon.Position = UDim2.new(0.175, 0, 0.175, 0)
    icon.Size = UDim2.new(0.65, 0, 0.65, 0)
    icon.BackgroundTransparency = 1
    if type(info.image) == "string" then icon.Image = info.image end
    icon.Parent = b

    local title = Instance.new("TextLabel")
    title.Name = "ActionTitle"
    title.Size = UDim2.new(1, 0, 1, 0)
    title.BackgroundTransparency = 1
    title.Font = Enum.Font.SourceSansBold
    title.TextColor3 = Color3.new(1, 1, 1)
    title.TextStrokeTransparency = 0
    title.TextSize = 18
    title.TextWrapped = true
    title.Text = if type(info.title) == "string" then info.title else ""
    title.Parent = b
    return b
end

local function remove_action(name)
    local e = entries[name]
    if not e then return end
    entries[name] = nil
    if e.place then places[e.place] = nil end
    if e.button then e.button:Destroy() end
end

local function add_action(name, create, info)
    if entries[name] then remove_action(name) end
    local e = { info = info }
    entries[name] = e
    if not create then return end
    local place = nil
    for i = 1, #PLACES do
        if places[i] == nil then place = i break end
    end
    if not place then
        warn("Maximum of " .. #PLACES .. " allowed touch buttons, new touch button not created for " .. name .. ".")
        return
    end
    ensure_gui()
    local b = new_button(name, info)
    e.button, e.place = b, place
    places[place] = name
    b.Position = if typeof(info.position) == "UDim2" then info.position else PLACES[place]
    b.Parent = frame
    if gui.Parent == nil then gui.Parent = playerGui end
end

CAS.BoundActionChanged:Connect(function(name, field, changes)
    local e = entries[name]
    if not e or type(changes) ~= "table" then return end
    local value = changes[field]
    if value == nil and (field == "image" or field == "title" or field == "description") then
        value = "" -- cleared
    end
    e.info[field] = value
    local b = e.button
    if not b then return end
    if field == "image" then
        b.ActionIcon.Image = if type(value) == "string" then value else ""
    elseif field == "title" then
        b.ActionTitle.Text = if type(value) == "string" then value else ""
    elseif field == "position" then
        if typeof(value) == "UDim2" then b.Position = value end
    end
end)
CAS.BoundActionAdded:Connect(function(name, create, info)
    add_action(name, create == true, if type(info) == "table" then info else {})
end)
CAS.BoundActionRemoved:Connect(function(name)
    remove_action(name)
end)
CAS.GetActionButtonEvent:Connect(function(name)
    local e = entries[name]
    if e then CAS:FireActionButtonFoundSignal(name, e.button) end
end)

-- The actions bound before this ran (and the GetButton calls waiting on
-- them).
for name, info in CAS:GetAllBoundActionInfo() do
    add_action(name, info.createTouchButton == true, info)
    local e = entries[name]
    if e and e.button then CAS:FireActionButtonFoundSignal(name, e.button) end
end
