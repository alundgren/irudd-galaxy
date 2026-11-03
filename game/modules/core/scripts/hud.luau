-- The ship's own screens: status, the log and, in flight, the orders. Each kind of
-- thing keeps one look and one place: status top right, the log bottom left, what just
-- happened at the bottom, orders on the left. A room panel or the open log takes the
-- whole focus, so the rest steps aside.
local rules = require("./rules")
local layout = require("./layout")
local teach = require("./teach")
local signs = require("./signs")

-- The flight keys, each shown until it has been used a few times.
local HINTS = {
    { "thrust", "W/S thrust" }, { "turn", "A/D turn" }, { "boost", "Shift boost" },
    { "mine", "Space mine" }, { "dock", "E dock" }, { "cancel", "0 cancel order" },
}

local function hints(ctx)
    local taught, parts = ctx.get("ship", "taught"), {}
    for _, hint in HINTS do
        if teach.new(taught, hint[1]) then table.insert(parts, hint[2]) end
    end
    return table.concat(parts, " · ")
end

local function where(ctx)
    local pilot = ctx.get("ship", "pilot")
    local here = ctx.get("ship", "engine.location").place
    local docked = here ~= "" and ctx.get(here, "port") ~= nil
    return pilot, here == "core.reach", docked
end

local function status(ui, pilot)
    return ui.panel { id = "status", x = layout.W - 220, y = 0, width = 220, gap = 4, children = {
        ui.text { id = "hold", text = "Hold " .. pilot.cargo .. " / " .. rules.CAPACITY .. " ore", size = 20 },
        ui.text { id = "credits", text = pilot.credits .. " cr", size = 20 },
    } }
end

local function log_panel(ctx, ui)
    local entries = ctx.query("entry")
    table.sort(entries, function(a, b) return ctx.get(a, "entry").n > ctx.get(b, "entry").n end)
    local rows = {}
    for _, id in entries do
        table.insert(rows, ui.text { text = ctx.get(id, "entry").text, size = 18 })
    end
    if #rows == 0 then rows = { ui.text { text = "Nothing logged yet.", size = 18 } } end
    return layout.frame(ui, "log", {
        ui.panel { id = "log", x = 0, y = 0, width = layout.PANEL, gap = 12, children = {
            ui.text { id = "log_title", text = "Ship log", size = 28 },
            ui.text { id = "log_order", text = "Newest first", size = 16 },
            ui.scroll { id = "entries", height = 440, children = rows },
            ui.button { id = "log_close", text = "Close the log  [L]", on = "log" },
        } },
    })
end

local function orders(ctx, ui)
    local children = {
        ui.button { id = "mine", text = "Mine nearest ore  [Space]", on = "mine" },
    }
    for _, id in ctx.query("port") do
        local port = ctx.get(id, "port")
        table.insert(children, ui.button { id = string.match(id, "%.(%w+)$"), text = "Dock at " .. port.name,
            hover = port.name .. " pays " .. port.price .. " cr per ore", on = "dock", payload = { station = id } })
    end
    return ui.column { id = "orders", x = 0, y = 0, width = 300, gap = 12, children = children }
end

return {
    on = {
        log = function(event, ctx)
            if event.payload.down == false then return end
            local pilot = ctx.get("ship", "pilot")
            if pilot.room ~= "" then return end
            ctx.patch("ship", "pilot", { log_open = not pilot.log_open })
            local here = ctx.get("ship", "engine.location").place
            if here ~= "" and ctx.get(here, "port") then signs.show(ctx, here, "", not pilot.log_open, signs.speaking(ctx)) end
        end,
    },
    screen = function(ctx, ui)
        local pilot, space, docked = where(ctx)
        if not space and not docked then return nil end
        if pilot.log_open then return log_panel(ctx, ui) end
        if docked and pilot.room ~= "" then return nil end
        local children = {
            status(ui, pilot),
            ui.button { id = "log_open", x = 0, y = layout.H - layout.BUTTON, width = 200, text = "Ship log  [L]", on = "log" },
        }
        if space then
            table.insert(children, orders(ctx, ui))
            table.insert(children, ui.text { id = "message", x = 336, y = layout.H - 76, width = 560, text = pilot.message, size = 20 })
            local keys = hints(ctx)
            if keys ~= "" then
                table.insert(children, ui.text { id = "hints", x = 336, y = layout.H - 26, width = 560, size = 16, text = keys })
            end
        end
        return layout.frame(ui, "hud", children)
    end,
}
