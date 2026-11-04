-- The ship's own screens: status, the log and, in flight, the orders. Each kind of
-- thing keeps one look and one place: status top right, the log bottom left, what just
-- happened at the bottom with the key to learn and the orders, and each station's name
-- under it while it is in view. A room panel or the open log takes the
-- whole focus, so the rest steps aside.
local rules = require("./rules")
local layout = require("./layout")
local teach = require("./teach")
local signs = require("./signs")
local trade = require("./trade")
local fit = require("./fit")

-- The flight keys, taught one at a time: one key and what it does, until it has been
-- used a few times (teach.luau). Mining has its own button, which names its key.
local function hint(ctx)
    local taught = ctx.get("ship", "taught")
    local full = rules.load(ctx.get("ship", "hold")) >= fit.hold(ctx)
    if full and teach.new(taught, "dock") then return "E · dock at the nearest station" end
    if teach.new(taught, "thrust") then return "W · fly ahead" end
    if teach.new(taught, "turn") then return "A or D · turn" end
    if teach.new(taught, "boost") then return "Shift · boost" end
    if teach.new(taught, "dock") then return "E · dock at the nearest station" end
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

-- Every flight order can be given by mouse (direction doc 9, "Mouse first"): mining,
-- named with its key, beside the one key still to learn; then docking at either station,
-- whose posted prices show on hover.
local function orders(ctx, ui, bottom)
    local inside = layout.MESSAGE - 32
    local first = { ui.button { id = "mine", width = 120, pad = 12, size = 16, text = "Space · Mine",
        hover = "Mine the nearest ore", on = "mine" } }
    local key = hint(ctx)
    if key then table.insert(first, ui.text { id = "hints", width = inside - 128, pad = 12, size = 16, text = key }) end
    table.insert(bottom, ui.row { id = "orders", gap = 8, children = first })
    local docks = { ui.text { id = "dock_at", width = 88, pad = 12, text = "Dock at", size = 16 } }
    for _, id in ctx.query("port") do
        local port = ctx.get(id, "port")
        table.insert(docks, ui.button { id = string.match(id, "%.(%w+)$"), width = #port.name * 9 + 28, pad = 12, size = 16,
            text = port.name, hover = trade.posted(ctx, id), on = "dock", payload = { station = id } })
    end
    table.insert(bottom, ui.row { id = "docks", gap = 8, children = docks })
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
            -- One strip at the bottom: the latest message, then the orders and the one
            -- key still to learn.
            local bottom = { ui.text { id = "message", width = layout.MESSAGE - 32, text = pilot.message, size = 20 } }
            orders(ctx, ui, bottom)
            table.insert(children, ui.panel { id = "bottom", align = "bottom", y = layout.M, width = layout.MESSAGE, gap = 6, children = bottom })
        end
        return layout.frame(ui, "hud", children)
    end,
}
