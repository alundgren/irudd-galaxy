-- Docked: the station up close. Its buildings are the menu; a room opens as a panel
-- beside its building, and the station key along the bottom reaches every room.
local rules = require("./rules")
local log = require("./log")
local teach = require("./teach")
local voices = require("./voices")
local signs = require("./signs")
local trade = require("./trade")
local fit = require("./fit")
local cradle = require("./cradle")
local undock = require("./launch")
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

-- A room still being built trades nothing yet.
local function trading(ctx, room)
    local site = ctx.get(room, "build")
    return not (site and not site.done)
end

local function open(event, ctx)
    if not pressed(event) then return end
    local station, pilot = docked(ctx)
    if not station then return end
    local id = event.payload.facility or ctx.get(station, "port").exchange
    local facility = ctx.get(id, "facility")
    if not facility or facility.station ~= station or pilot.room == id then return end
    signs.hush(ctx) -- the bubble would ride the glide in over the room's panel
    ctx.patch("ship", "pilot", { room = id, sale = 0, deal = "", log_open = false, gliding = true })
    -- The view glides to frame the building, beside its room's panel.
    ctx.patch(station, "engine.room", { camera = facility.camera })
    signs.show(ctx, station, id, false, nil, true)
end

-- Back (B, Esc, right-click or the Back button): from a room to the whole station,
-- or shut the open log.
local function close(event, ctx)
    if not pressed(event) then return end
    local station, pilot = docked(ctx)
    if station and pilot.room ~= "" then
        signs.hush(ctx) -- the bubble would ride the glide out over the HUD
        ctx.patch("ship", "pilot", { room = "", sale = 0, deal = "", gliding = true })
        ctx.patch(station, "engine.room", { camera = "" })
        signs.show(ctx, station, "", false, nil, true)
    elseif pilot.log_open then
        ctx.patch("ship", "pilot", { log_open = false })
        if station then signs.show(ctx, station, "", false, nil) end
    end
end

local function launch(event, ctx)
    if not pressed(event) then return end
    local station = docked(ctx)
    if station then undock.launch(ctx, station) end
end

-- The site becomes its room: the finished building, its keeper arriving to stand by
-- it, and the goods they trade.
local function build_up(ctx, site, build)
    local station = build.station
    ctx.patch(site, "engine.model", { model = build.model })
    ctx.patch(site, "facility", { name = build.name, keeper = build.keeper, offer = build.offer })
    local keeper = build.keeper
    local act = keeper .. "_act"
    local plan = ctx.get(act, "engine.act")
    ctx.put(keeper, "engine.being", {})
    ctx.put(keeper, "engine.location", { place = station, spot = plan.spot })
    ctx.put(keeper, "engine.position", { angle = plan.angle })
    ctx.put(keeper, "engine.model", { model = "core.keeper", scale = 1.5 })
    -- The sale that builds it happens in another room, where only that room's keeper
    -- wears a sign; the new keeper's sign goes up when the player is back on the
    -- station (signs.show).
    ctx.put(keeper, "engine.life", { clock = "core.day", first = act, name = "" })
    -- Their thanks for the building goes on offer once the introduction is heard; the
    -- sale that built it left room in the hold.
    for _, id in ctx.query("gift") do
        local gift = ctx.get(id, "gift")
        if ctx.get(id, "engine.talk").speaker == keeper and not gift.answered then
            ctx.patch(id, "engine.talk", { enabled = true })
            ctx.patch(gift.instead, "engine.talk", { enabled = false })
        end
    end
end

-- Sells the hold's `ware` (or everything this room buys) at the room's posted prices.
local function sell(event, ctx)
    if not pressed(event) then return end
    local station, pilot = docked(ctx)
    if not station or pilot.room == "" or not trading(ctx, pilot.room) then return end
    local port = ctx.get(station, "port")
    local hold = ctx.get("ship", "hold")
    local site, build = trade.building(ctx, station)
    local earned, sold, count, into, grew = 0, {}, 0, 0, false
    for _, id in trade.lines(ctx, pilot.room) do
        local line = ctx.get(id, "market")
        local units = hold[line.ware]
        if line.role == "buys" and units > 0 and (event.payload.ware == nil or event.payload.ware == line.ware) then
            -- Goods a station is building with go into the build, at a steady price,
            -- until it is done; the rest go onto its stock.
            local building = 0
            if build and build.ware == line.ware then
                building = math.min(units, build.need - build.given)
                into = building
                earned += building * rules.price(line)
                grew = build.given + building >= build.need
                ctx.patch(site, "build", { given = build.given + building, done = grew })
            end
            local prices, stock = rules.walk(line, units - building)
            earned += rules.total(prices)
            ctx.patch(id, "market", { stock = stock })
            if line.ware == "ore" then
                ctx.patch(station .. "_stock", "engine.model", { scale = trade.pile(stock) })
            end
            table.insert(sold, units .. " " .. line.ware)
            count += units
            hold[line.ware] = 0
        end
    end
    if #sold == 0 then return end
    local what = table.concat(sold, " and ")
    local text = "Sold " .. what .. " to " .. port.name .. " for " .. earned .. " cr."
    if grew then
        build_up(ctx, site, build)
        text ..= " The " .. string.lower(build.name) .. " is built: " .. ctx.get(build.keeper, "keeper").name .. " " .. build.offer .. " there."
        ctx.record("grown", { station = station, site = site })
    elseif into > 0 then
        text ..= " " .. into .. " " .. build.ware .. " went into the " .. string.lower(build.name) .. ": "
            .. (build.given + into) .. " of " .. build.need .. "."
    end
    ctx.put("ship", "hold", hold)
    local deal = "Sold " .. what .. "." .. (grew and " The " .. string.lower(build.name) .. " is built!" or "")
    ctx.patch("ship", "pilot", { credits = pilot.credits + earned, sale = earned, deal = deal })
    voices.refresh(ctx, station, hold, earned, build and build.need - build.given - into or 0)
    log.add(ctx, text)
    ctx.record("sold", { units = count, credits = earned, station = station })
end

-- Buys `units` of `ware` (0 for as many as fit and are affordable) from the room.
local function buy(event, ctx)
    if not pressed(event) then return end
    local station, pilot = docked(ctx)
    if not station or pilot.room == "" or not trading(ctx, pilot.room) then return end
    local hold = ctx.get("ship", "hold")
    for _, id in trade.lines(ctx, pilot.room) do
        local line = ctx.get(id, "market")
        if line.role == "sells" and line.ware == event.payload.ware then
            local units = rules.affordable(line, fit.hold(ctx) - rules.load(hold), pilot.credits)
            local asked = event.payload.units or 0
            if asked > 0 then units = math.min(units, asked) end
            if units == 0 then return end -- resting: the hold is full, or credits are short
            local prices, stock = rules.walk(line, units)
            local spent = rules.total(prices)
            hold[line.ware] += units
            ctx.patch(id, "market", { stock = stock })
            ctx.put("ship", "hold", hold)
            local what = units .. " " .. line.ware
            ctx.patch("ship", "pilot", { credits = pilot.credits - spent, sale = -spent, deal = "Bought " .. what .. "." })
            local port = ctx.get(station, "port")
            local _, build = trade.building(ctx, station)
            voices.refresh(ctx, station, hold, 0, build and build.need - build.given or 0)
            log.add(ctx, "Bought " .. what .. " at " .. port.name .. " for " .. spent .. " cr.")
            ctx.record("bought", { ware = line.ware, units = units, credits = spent, station = station })
            return
        end
    end
end

-- The view has settled at a station: arrived, or framed a building. Signs go back up.
local function settled(event, ctx)
    local place = event.payload.place
    if event.payload.entity ~= "core.ship" or not ctx.get(place, "port") then return end
    local pilot = ctx.get("ship", "pilot")
    if not pilot.gliding then return end
    ctx.patch("ship", "pilot", { gliding = false })
    signs.show(ctx, place, pilot.room, pilot.log_open, signs.speaking(ctx), false)
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

-- One good in a trading room: its price and stock as plain text, then its verbs.
local function line_rows(ctx, ui, id, hold, capacity, credits, rows, sellable, build)
    local line = ctx.get(id, "market")
    local name = string.upper(string.sub(line.ware, 1, 1)) .. string.sub(line.ware, 2)
    local price = rules.price(line)
    local w = line.ware
    if line.role == "buys" then
        local about = name .. " · pays " .. price .. " cr"
        if build and build.ware == w then
            about ..= " · builds the " .. string.lower(build.name) .. " (" .. build.given .. " of " .. build.need .. ")"
        else
            about ..= " · stock " .. line.stock
        end
        table.insert(rows, ui.text { id = "about_" .. w, text = about, size = 20 })
        if hold[w] > 0 then
            local units = hold[w]
            local building = build and build.ware == w and math.min(units, build.need - build.given) or 0
            local value = building * price + rules.total((rules.walk(line, units - building)))
            local key = sellable == 1 and "  [Enter]" or ""
            table.insert(rows, ui.button { id = "sell_" .. w, on = "sell", payload = { ware = w },
                text = "Sell " .. units .. " " .. w .. " for " .. value .. " cr" .. key })
        end
    else
        table.insert(rows, ui.text { id = "about_" .. w, text = name .. " · asks " .. price .. " cr · stock " .. line.stock, size = 20 })
        local room = capacity - rules.load(hold)
        local most = rules.affordable(line, room, credits)
        -- Buying that can't happen now rests in place, with its reason beside it.
        local why = nil
        if room == 0 then
            why = "Hold full: sell first."
        elseif line.stock == 0 then
            why = "Sold out until the stock comes back."
        elseif most == 0 then
            why = "Not enough credits for one " .. w .. "."
        end
        if why then
            table.insert(rows, ui.row { id = "buy_" .. w, gap = 12, children = {
                ui.button { id = "buy1_" .. w, width = 200, on = "buy", payload = { ware = w, units = 1 },
                    text = "Buy 1 · " .. price .. " cr", resting = why },
                ui.text { id = "why_" .. w, width = 276, text = why, size = 18 },
            } })
        else
            local buttons = { ui.button { id = "buy1_" .. w, width = 200, on = "buy", payload = { ware = w, units = 1 },
                text = "Buy 1 · " .. price .. " cr" } }
            if most > 1 then
                table.insert(buttons, ui.button { id = "buymax_" .. w, width = 240, on = "buy", payload = { ware = w, units = 0 },
                    text = "Buy " .. most .. " · " .. rules.total((rules.walk(line, most))) .. " cr" })
            end
            table.insert(rows, ui.row { id = "buy_" .. w, gap = 12, children = buttons })
        end
    end
end

local function trade_rows(ctx, ui, station, pilot, children)
    local hold = ctx.get("ship", "hold")
    local _, build = trade.building(ctx, station)
    local lines = trade.lines(ctx, pilot.room)
    local sellable = 0
    for _, id in lines do
        local line = ctx.get(id, "market")
        if line.role == "buys" and hold[line.ware] > 0 then sellable += 1 end
    end
    local capacity = fit.hold(ctx)
    table.insert(children, ui.text { id = "ledger", size = 20, text = "Hold " .. rules.load(hold) .. " / " .. capacity
        .. ": " .. rules.list(hold) .. "\nYou have " .. pilot.credits .. " cr" })
    for _, id in lines do
        line_rows(ctx, ui, id, hold, capacity, pilot.credits, children, sellable, build)
    end
    if sellable > 1 then
        table.insert(children, ui.button { id = "sell", on = "sell", text = "Sell everything here  [Enter]" })
    end
    if pilot.deal ~= "" then
        table.insert(children, ui.row { id = "receipt", gap = 8, children = {
            ui.text { width = 330, text = pilot.deal .. (pilot.sale >= 0 and " Paid:" or " Spent:"), size = 20 },
            ui.delta { width = 80, value = pilot.sale, size = 20 },
        } })
    end
end

local function office_rows(ctx, ui, station, children)
    local lines = { "Posted prices:" }
    for _, id in ctx.query("port") do
        table.insert(lines, trade.posted(ctx, id) .. (id == station and " (here)" or ""))
    end
    table.insert(children, ui.text { id = "prices", width = 488, text = table.concat(lines, "\n"), size = 20 })
    for _, id in ctx.query("build") do
        local build = ctx.get(id, "build")
        local where = ctx.get(build.station, "port").name
        local text = build.done and where .. "'s " .. string.lower(build.name) .. " is open."
            or where .. " is building a " .. string.lower(build.name) .. ": " .. build.given .. " of " .. build.need .. " " .. build.ware .. " in."
        table.insert(children, ui.text { id = "build_" .. short(id), width = 488, text = text, size = 20 })
    end
    local left, total = 0, 0
    for _, id in ctx.query("ore") do
        local ore = ctx.get(id, "ore")
        left, total = left + ore.left, total + ore.total
    end
    table.insert(children, ui.text { id = "field", text = "Ore left in the field: " .. left .. " of " .. total, size = 20 })
end

local function site_rows(ui, build, children)
    table.insert(children, ui.text { id = "plan", width = 488, size = 20, text = "A " .. string.lower(build.name)
        .. " goes up here once the station has the " .. build.ware .. " for it." })
    table.insert(children, ui.text { id = "progress", size = 22, text = build.given .. " of " .. build.need .. " " .. build.ware .. " in" })
    table.insert(children, ui.text { id = "how", width = 488, size = 18, text = "Plate sold at the exchange builds it. When it opens, a cook will sell rations here." })
end

local function room_panel(ctx, ui, station, pilot)
    local facility = ctx.get(pilot.room, "facility")
    local at = ctx.get(pilot.room, "engine.position")
    local site = ctx.get(pilot.room, "build")
    local keeper_text = nil
    if facility.keeper ~= "" then
        local keeper = ctx.get(facility.keeper, "keeper")
        local first = string.match(keeper.name, "^%S+")
        keeper_text = keeper.name .. " keeps it."
            .. (teach.new(ctx.get("ship", "taught"), "talk") and " Click " .. first .. " to talk." or "")
    end
    local children
    if facility.fits then
        -- The fitting room has the most to show, so it packs its own rows.
        children = cradle.rows(ctx, ui, pilot, facility.name, keeper_text)
    else
        children = { ui.text { id = "title", text = facility.name, size = 28 } }
        if keeper_text then table.insert(children, ui.text { id = "keeper", size = 18, text = keeper_text }) end
        if site and not site.done then
            site_rows(ui, site, children)
        elseif #trade.lines(ctx, pilot.room) > 0 then
            trade_rows(ctx, ui, station, pilot, children)
        else
            office_rows(ctx, ui, station, children)
        end
        table.insert(children, ui.button { id = "close", text = "Back to the station  [B]", on = "close" })
    end
    -- The panel opens on the side away from its building, so the building stays in view.
    return layout.frame(ui, "room", {
        ui.panel { id = "room", align = at.x < 0 and "top-right" or "top-left", x = layout.M, y = layout.M,
            width = layout.PANEL, gap = facility.fits and 8 or 12, children = children },
    })
end

-- What a building's sign says on hover: its keeper and offer, or how its build stands.
local function about(ctx, id)
    local facility = ctx.get(id, "facility")
    local site = ctx.get(id, "build")
    if site and not site.done then
        return facility.name .. " · " .. site.given .. " of " .. site.need .. " " .. site.ware .. " in"
    end
    return ctx.get(facility.keeper, "keeper").name .. " · " .. facility.offer
end

local function overview(ctx, ui, station, pilot)
    local port = ctx.get(station, "port")
    local children = {
        ui.text { id = "station", align = "top-left", x = layout.M, y = layout.M, width = 420,
            text = "Docked at " .. port.name, size = 30 },
    }
    local key = {}
    local talking = signs.speaking(ctx) ~= nil
    for _, id in facilities(ctx, station) do
        local facility = ctx.get(id, "facility")
        local text = about(ctx, id)
        -- Signs rest while the view glides or someone speaks, so they never crowd the
        -- moving view or a bubble.
        if not pilot.gliding and not talking then
            table.insert(children, ui.button { id = "sign_" .. short(id), anchor = id .. "#sign", width = 170, text = facility.name,
                hover = text, on = "open", payload = { facility = id } })
        end
        -- The building itself: it lifts and outlines under the pointer, and a click opens it.
        table.insert(children, ui.target { id = "target_" .. short(id), anchor = "being." .. id, text = text,
            on = "open", payload = { facility = id } })
        table.insert(key, ui.button { id = "key_" .. short(id), width = 170, text = facility.name, hover = text,
            on = "open", payload = { facility = id } })
    end
    table.insert(children, ui.row { id = "key", align = "bottom", y = layout.M, width = #key * 182 - 12, gap = 12, children = key })
    table.insert(children, ui.button { id = "launch", align = "bottom-right", x = layout.M, y = layout.M, width = 200,
        text = "Launch  [U]", hover = "Fly off from the berth: U, or just fly with W, A, S or D", on = "launch" })
    return layout.frame(ui, "dock", children)
end

return {
    on = {
        open = open, close = close, launch = launch, sell = sell, buy = buy,
        fit = cradle.fit, unfit = cradle.unfit,
        ["engine.place_arrived"] = settled, ["engine.camera_settled"] = settled,
    },
    cadence = skiffs,
    screen = function(ctx, ui)
        local station, pilot = docked(ctx)
        if not station or pilot.log_open then return nil end
        if pilot.room ~= "" then return room_panel(ctx, ui, station, pilot) end
        return overview(ctx, ui, station, pilot)
    end,
}
