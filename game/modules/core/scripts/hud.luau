-- The ship's own screens: status, the log and, in flight, the things you can click: each
-- rock and each station. Each kind of thing keeps one look and one place: status top
-- right, the log bottom left, what just happened at the bottom with the key that fits
-- where the ship is, and each station's name under it while it is in view. A room panel or the open log takes the
-- whole focus, so the rest steps aside.
local rules = require("./rules")
local layout = require("./layout")
local teach = require("./teach")
local signs = require("./signs")
local trade = require("./trade")
local fit = require("./fit")
local cutter = require("./cutter")

-- One hint at a time, for what a press does right now: Space in a berth's ring, the
-- beam with a rock in its sights, the turn to bring the nose onto one in reach, then the
-- flight keys; each until it has been used a few times (teach.luau).
local function hint(ctx)
    local taught = ctx.get("ship", "taught")
    local pilot = ctx.get("ship", "pilot")
    local full = rules.load(ctx.get("ship", "hold")) >= fit.hold(ctx)
    local ring = string.match(pilot.near, "^ring:(.+)$")
    if ring and ctx.get(ring, "port") and teach.new(taught, "dock") then
        return "Space · dock at " .. ctx.get(ring, "port").name
    end
    if not full and not pilot.mining and teach.new(taught, "mine") then
        if pilot.near == "aim" then return "Hold Space · cut the rock ahead" end
        if pilot.near == "reach" then return "A or D · point your nose at the rock" end
    end
    if full and teach.new(taught, "dock") then
        if cutter.has(ctx, cutter.JETS) then return "E · the docking jets fly you in" end
        return "Fly slowly into a berth's glowing ring to dock"
    end
    if teach.new(taught, "thrust") then return "W · fly ahead" end
    if teach.new(taught, "turn") then return "A or D · turn" end
    if teach.new(taught, "boost") then return "Shift · boost" end
    if not full and teach.new(taught, "mine") then return "Fly up to a rock and face it to cut" end
    if ctx.get("ship", "engine.steer").mode ~= "" and teach.new(taught, "cancel") then return "0 · cancel the order" end
    return nil
end

local function where(ctx)
    local pilot = ctx.get("ship", "pilot")
    local here = ctx.get("ship", "engine.location").place
    local docked = here ~= "" and ctx.get(here, "port") ~= nil
    return pilot, here == "core.reach", docked
end

local function status(ctx, ui, pilot)
    local hold = ctx.get("ship", "hold")
    -- The status chip: credits first and largest, then the hold and what is in it.
    local children = {
        ui.text { id = "credits", text = pilot.credits .. " cr", size = 22 },
        ui.text { id = "hold", text = "Hold " .. rules.load(hold) .. " / " .. fit.hold(ctx), size = 18 },
    }
    -- What the beam has cut this time, counting up as each unit lands.
    if pilot.gained > 0 then
        children[2] = ui.row { id = "hold_row", gap = 8, children = {
            children[2], ui.delta { id = "gained", width = 48, value = pilot.gained, size = 18 } } }
    end
    if rules.load(hold) > 0 then
        table.insert(children, ui.text { id = "cargo", text = rules.list(hold), size = 16 })
    end
    return ui.panel { id = "status", align = "top-right", x = layout.M, y = layout.M, width = 200, gap = 0, children = children }
end

local function log_panel(ctx, ui)
    local entries = ctx.query("entry")
    table.sort(entries, function(a, b) return ctx.get(a, "entry").n > ctx.get(b, "entry").n end)
    -- Each entry carries its kind as a word in a tag: TALK for what someone said, SHIP
    -- for what happened to the ship.
    local rows = {}
    for _, id in entries do
        local entry = ctx.get(id, "entry")
        table.insert(rows, ui.row { gap = 10, children = {
            ui.panel { width = 64, pad = 4, children = { ui.text { text = entry.talk ~= "" and "TALK" or "SHIP", size = 16 } } },
            ui.text { width = layout.PANEL - 176, text = entry.text, size = 18 },
        } })
    end
    if #rows == 0 then rows = { ui.text { text = "Nothing logged yet.", size = 18 } } end
    return layout.frame(ui, "log", {
        ui.panel { id = "log", align = "top-left", x = layout.M, y = layout.M, width = layout.PANEL, gap = 12, children = {
            ui.row { id = "log_heading", gap = 12, children = {
                ui.column { width = 276, gap = 2, children = {
                    ui.text { id = "log_title", text = "Ship log", size = 28 },
                    ui.text { id = "log_order", text = "Newest first", size = 16 },
                } },
                ui.button { id = "log_close", width = 200, text = "Close  [L]", on = "log" },
            } },
            ui.scroll { id = "entries", height = 400, gap = 8, children = rows },
        } },
    })
end

-- Each station wears its name under it while it is in view, as the traders do
-- (traffic.luau): `yield`, so it is left undrawn rather than sit on the HUD.
local function station_names(ctx, ui, children)
    for _, id in ctx.query("port") do
        local name = ctx.get(id, "port").name
        table.insert(children, ui.text { id = "name_" .. string.match(id, "%.(%w+)$"), anchor = id .. "_exterior#label",
            width = #name * 9 + 16, text = name, size = 16, yield = true })
    end
end

-- Everything in flight is also a click on the thing itself (direction doc 9, "Mouse
-- first"): a rock lifts and outlines under the pointer and a click cuts it, a station
-- docks. What a click does is said on hover; what is in reach depends on the fit.
local function targets(ctx, ui, children)
    local rig = cutter.has(ctx, cutter.RIG)
    local pilot = ctx.get("ship", "pilot")
    for _, id in ctx.query("ore") do
        if ctx.get(id, "engine.anchor") then
            local ore = ctx.get(id, "ore")
            local text
            if ore.left <= 0 then
                text = "Cut down to pebbles"
            elseif pilot.mining and pilot.target == id then
                text = ore.left .. " of " .. ore.total .. " ore left · click to stop the beam"
            elseif rig then
                text = ore.left .. " of " .. ore.total .. " ore · click: the rig flies out and cuts it"
            else
                text = ore.left .. " of " .. ore.total .. " ore · click to cut it, nose on it and in reach"
            end
            table.insert(children, ui.target { id = "rock_" .. string.match(id, "%.(%w+)$"), anchor = id .. "#rock",
                text = text, on = "mine", payload = { rock = id } })
        end
    end
    local jets = cutter.has(ctx, cutter.JETS)
    for _, id in ctx.query("port") do
        local how = jets and "click: the docking jets fly you in" or "fly slowly into its berth's ring to dock"
        table.insert(children, ui.target { id = "dock_" .. string.match(id, "%.(%w+)$"), anchor = id .. "_exterior#label",
            text = trade.posted(ctx, id) .. " · " .. how, on = "dock", payload = { station = id } })
    end
end

return {
    on = {
        log = function(event, ctx)
            if event.payload.down == false then return end
            local pilot = ctx.get("ship", "pilot")
            if pilot.room ~= "" then return end
            -- The open log takes the whole focus: a bubble would sit on it.
            signs.hush(ctx)
            ctx.patch("ship", "pilot", { log_open = not pilot.log_open })
            local here = ctx.get("ship", "engine.location").place
            if here ~= "" and ctx.get(here, "port") then signs.show(ctx, here, "", not pilot.log_open, nil) end
        end,
    },
    screen = function(ctx, ui)
        local pilot, space, docked = where(ctx)
        if not space and not docked then return nil end
        if pilot.log_open then return log_panel(ctx, ui) end
        if docked and pilot.room ~= "" then return nil end
        local children = {
            status(ctx, ui, pilot),
            ui.button { id = "log_open", align = "bottom-left", x = layout.M, y = layout.M, width = 104, height = 64, text = "Log · L", hover = "The ship log: what happened and what was said", on = "log" },
        }
        if space then
            station_names(ctx, ui, children)
            targets(ctx, ui, children)
            -- One strip at the bottom: the latest message, then the one key still to learn.
            local bottom = { ui.text { id = "message", width = layout.MESSAGE - 32, text = pilot.message, size = 20 } }
            local key = hint(ctx)
            if key then table.insert(bottom, ui.text { id = "hints", width = layout.MESSAGE - 32, size = 16, text = key }) end
            table.insert(children, ui.panel { id = "bottom", align = "bottom", y = layout.M, width = layout.MESSAGE, gap = 6, children = bottom })
        end
        return layout.frame(ui, "hud", children)
    end,
}
