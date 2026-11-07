-- Finds out in the reach (scripts/rumour.luau): flying close to what a rumour leads to
-- wakes it, once, with its line in the log, and pointing at it says what it is.
local rumour = require("./rumour")
local log = require("./log")

local function cadence(ctx)
    if ctx.get("ship", "engine.location").place ~= "core.reach" then return end
    local seen = rumour.arrive(ctx, ctx.get("ship", "engine.position"))
    if seen then log.add(ctx, seen) end
end

local function screen(ctx, ui)
    if ctx.get("ship", "engine.location").place ~= "core.reach" then return nil end
    if ctx.get("ship", "pilot").gliding then return nil end
    local thing, text = rumour.hover(ctx)
    if not thing then return nil end
    return ui.column { id = "finds", children = {
        ui.hover { id = "hover_" .. string.sub(thing, 6), anchor = thing .. "#label", text = text },
    } }
end

return { cadence = cadence, screen = screen }
