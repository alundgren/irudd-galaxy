-- Docked: the station up close. Its buildings are the menu; a room opens as a panel
-- beside its building, and the station key along the bottom reaches every room.
local rules = require("./rules")
local log = require("./log")
local layout = require("./layout") -- room panels are layout.PANEL wide: at most about 45% of the view

local function pressed(event)
    return event.payload.down ~= false
end

-- The station the ship is docked at, or nil: a place with a port is a station's dock.
local function docked(ctx)
    local pilot = ctx.get("ship", "pilot")
    local here = ctx.get("ship", "engine.location").place
    if here ~= "" and ctx.get(here, "port") then return here, pilot end
    return nil, pilot
end

-- The station's buildings, in key order.
local function facilities(ctx, station)
    local list = {}
    for _, id in ctx.query("facility") do
        if ctx.get(id, "facility").station == station then table.insert(list, id) end
    end
    table.sort(list, function(a, b) return ctx.get(a, "facility").order < ctx.get(b, "facility").order end)
    return list
end

local function short(id)
    return string.match(id, "_(%w+)$") or id
end

local function open(event, ctx)
    if not pressed(event) then return end
    local station, pilot = docked(ctx)
    if not station then return end
    local id = event.payload.facility or ctx.get(station, "port").exchange
    local facility = ctx.get(id, "facility")
    if not facility or facility.station ~= station or pilot.room == id then return end
    ctx.patch("ship", "pilot", { room = id, sale = 0, log_open = false })
end

local function close(event, ctx)
    if not pressed(event) then return end
    local station, pilot = docked(ctx)
    if station and pilot.room ~= "" then ctx.patch("ship", "pilot", { room = "", sale = 0 }) end
end

local function launch(event, ctx)
    if not pressed(event) then return end
    local station = docked(ctx)
    if not station then return end
    local port = ctx.get(station, "port")
    ctx.put("ship", "engine.helm", {})
    ctx.put("ship", "engine.steer", {})
    ctx.put("ship", "held", {})
    ctx.patch("ship", "pilot", { room = "", mining = false, target = "", log_open = false })
    ctx.move("ship", "core.reach", port.departure)
    ctx.put("ship", "engine.body", { mass = 1 })
    ctx.patch("ship", "engine.model", { scale = 1 })
    log.add(ctx, "Launched from " .. port.name .. ". Space finds the nearest ore.")
end

local function sell(event, ctx)
    if not pressed(event) then return end
    local station, pilot = docked(ctx)
    if not station then return end
    local port = ctx.get(station, "port")
    if pilot.room ~= port.exchange or pilot.cargo == 0 then return end
    local earned = rules.sale(pilot.cargo, port.price)
    ctx.patch("ship", "pilot", { cargo = 0, credits = pilot.credits + earned, sale = earned })
    ctx.patch(station, "port", { stock = port.stock + pilot.cargo })
    ctx.patch(station .. "_stock", "engine.model", { scale = 0.4 + (port.stock + pilot.cargo) * 0.04 })
    log.add(ctx, "Sold " .. pilot.cargo .. " ore to " .. port.name .. " for " .. earned .. " cr.")
    ctx.record("sold", { units = pilot.cargo, credits = earned, station = station })
end

-- The skiffs ferry crates across their deck, back and forth, without end.
local function skiffs(ctx)
    for _, id in ctx.query("skiff") do
        local walk = ctx.get(id, "engine.walk")
        if not walk or not walk.active then
            local route = ctx.get(id, "skiff")
            local at = ctx.get(id, "engine.location").spot
            ctx.walk(id, at == route.a and route.b or route.a, 1.5)
        end
    end
end

local function room_panel(ctx, ui, station, pilot)
    local port = ctx.get(station, "port")
    local facility = ctx.get(pilot.room, "facility")
    local keeper = ctx.get(facility.keeper, "keeper")
    local at = ctx.get(pilot.room, "engine.position")
    local first = string.match(keeper.name, "^%S+")
    local children = {
        ui.text { id = "title", text = facility.name, size = 28 },
        ui.text { id = "keeper", text = keeper.name .. " keeps it. Click " .. first .. " to talk.", size = 18 },
    }
    if pilot.room == port.exchange then
        table.insert(children, ui.text { id = "price", text = "Pays " .. port.price .. " cr per ore", size = 22 })
        table.insert(children, ui.text { id = "ledger", size = 20,
            text = "In your hold: " .. pilot.cargo .. " ore\nYou have " .. pilot.credits .. " cr\nStation stock: " .. port.stock .. " ore" })
        if pilot.sale > 0 then
            table.insert(children, ui.row { id = "receipt", gap = 8, children = {
                ui.text { width = 210, text = "Sold. Credits paid:", size = 20 },
                ui.delta { width = 80, value = pilot.sale, size = 20 },
            } })
        end
        if pilot.cargo > 0 then
            table.insert(children, ui.button { id = "sell", on = "sell",
                text = "Sell " .. pilot.cargo .. " ore for " .. rules.sale(pilot.cargo, port.price) .. " cr  [Enter]" })
        else
            table.insert(children, ui.text { id = "nothing", text = "Nothing to sell. Ore waits in the rocks outside.", size = 18 })
        end
    else
        local lines = { "Posted ore prices:" }
        for _, id in ctx.query("port") do
            local other = ctx.get(id, "port")
            table.insert(lines, other.name .. " pays " .. other.price .. " cr per ore" .. (id == station and " (here)" or ""))
        end
        local left, total = 0, 0
        for _, id in ctx.query("ore") do
            local ore = ctx.get(id, "ore")
            left, total = left + ore.left, total + ore.total
        end
        table.insert(children, ui.text { id = "prices", text = table.concat(lines, "\n"), size = 20 })
        table.insert(children, ui.text { id = "field", text = "Ore left in the field: " .. left .. " of " .. total, size = 20 })
    end
    table.insert(children, ui.button { id = "close", text = "Back to the station  [B]", on = "close" })
    -- The panel opens on the side away from its building, so the building stays in view.
    local x = at.x < 0 and layout.W - layout.PANEL or 0
    return layout.frame(ui, "room", {
        ui.panel { id = "room", x = x, y = 0, width = layout.PANEL, gap = 14, children = children },
    })
end

local function overview(ctx, ui, station)
    local port = ctx.get(station, "port")
    local children = {
        ui.text { id = "station", x = 0, y = 0, width = 420, text = "Docked at " .. port.name, size = 30 },
    }
    local key = {}
    for _, id in facilities(ctx, station) do
        local facility = ctx.get(id, "facility")
        local keeper = ctx.get(facility.keeper, "keeper")
        local about = keeper.name .. " · " .. facility.offer
        table.insert(children, ui.button { id = "sign_" .. short(id), anchor = id .. "#sign", width = 190, text = facility.name,
            hover = about, on = "open", payload = { facility = id } })
        table.insert(children, ui.hover { id = "hover_" .. short(id), anchor = "being." .. id, text = about })
        table.insert(key, ui.button { id = "key_" .. short(id), text = facility.name, hover = about,
            on = "open", payload = { facility = id } })
    end
    table.insert(children, ui.row { id = "key", x = 336, y = layout.H - layout.BUTTON, width = 400, gap = 12, children = key })
    table.insert(children, ui.button { id = "launch", x = layout.W - 200, y = layout.H - layout.BUTTON, width = 200,
        text = "Launch  [U]", hover = "Fly off from the berth", on = "launch" })
    return layout.frame(ui, "dock", children)
end

return {
    on = { open = open, close = close, launch = launch, sell = sell },
    cadence = skiffs,
    screen = function(ctx, ui)
        local station, pilot = docked(ctx)
        if not station or pilot.log_open then return nil end
        if pilot.room ~= "" then return room_panel(ctx, ui, station, pilot) end
        return overview(ctx, ui, station)
    end,
}
