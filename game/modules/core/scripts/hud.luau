-- The ship's own screens: status, the log and, in flight, the orders. Each kind of
-- thing keeps one look and one place: status top right, the log bottom left, what just
-- happened at the bottom, orders on the left. A room panel or the open log takes the
-- whole focus, so the rest steps aside.
local rules = require("./rules")
local layout = require("./layout")
local teach = require("./teach")
local signs = require("./signs")
local trade = require("./trade")
local fit = require("./fit")

-- The flight keys, each shown until it has been used a few times.
local HINTS = {
    { "thrust", "W/S thrust" }, { "turn", "A/D turn" }, { "boost", "Shift boost" },
    { "mine", "Space mine" }, { "dock", "E dock" }, { "cancel", "0 cancel order" },
}

-- The flight keys still to learn, three to a line, so they fit beside the radio.
local function hints(ctx)
    local taught, lines, parts = ctx.get("ship", "taught"), {}, {}
    for _, hint in HINTS do
        if teach.new(taught, hint[1]) then table.insert(parts, hint[2]) end
        if #parts == 3 then table.insert(lines, table.concat(parts, " · ")); parts = {} end
    end
    if #parts > 0 then table.insert(lines, table.concat(parts, " · ")) end
    return lines
end

local function where(ctx)
    local pilot = ctx.get("ship", "pilot")
    local here = ctx.get("ship", "engine.location").place
    local docked = here ~= "" and ctx.get(here, "port") ~= nil
    return pilot, here == "core.reach", docked
end

local function status(ctx, ui, pilot)
    local hold = ctx.get("ship", "hold")
    local children = { ui.text { id = "hold", text = "Hold " .. rules.load(hold) .. " / " .. fit.hold(ctx), size = 20 } }
    if rules.load(hold) > 0 then
        table.insert(children, ui.text { id = "cargo", text = rules.list(hold), size = 18 })
    end
    table.insert(children, ui.text { id = "credits", text = pilot.credits .. " cr", size = 20 })
    return ui.panel { id = "status", align = "top-right", x = layout.M, y = layout.M, width = 240, gap = 4, children = children }
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
        ui.panel { id = "log", align = "top-left", x = layout.M, y = layout.M, width = layout.PANEL, gap = 12, children = {
            ui.text { id = "log_title", text = "Ship log", size = 28 },
            ui.text { id = "log_order", text = "Newest first", size = 16 },
            ui.scroll { id = "entries", height = 330, children = rows },
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
            hover = trade.posted(ctx, id), on = "dock", payload = { station = id } })
    end
    return ui.column { id = "orders", align = "top-left", x = layout.M, y = layout.M, width = 300, gap = 12, children = children }
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
            ui.button { id = "log_open", align = "bottom-left", x = layout.M, y = layout.M, width = 200, text = "Ship log  [L]", on = "log" },
        }
        if space then
            table.insert(children, orders(ctx, ui))
            -- The latest message sits just above the flight keys, or on the bottom edge
            -- once every key's hint has retired.
            local bottom = { ui.text { id = "message", width = layout.MESSAGE, text = pilot.message, size = 20 } }
            for i, line in hints(ctx) do
                table.insert(bottom, ui.text { id = i == 1 and "hints" or "hints" .. i, width = layout.MESSAGE, size = 16, text = line })
            end
            table.insert(children, ui.column { id = "bottom", align = "bottom", y = layout.M, width = layout.MESSAGE, gap = 6, children = bottom })
        end
        return layout.frame(ui, "hud", children)
    end,
}
