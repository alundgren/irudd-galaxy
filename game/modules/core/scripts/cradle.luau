-- The fitting cradle: parts on the ship's sockets, the parts tray, and the budgets.
-- A part is dragged from the tray onto its socket to fit it (buying it first if it
-- isn't yours yet), and from its socket back onto the tray to take it off. Every rule
-- is a verdict on the drop target (fit.verdict), so a refusal and its reason show the
-- moment a part is picked up, before it is let go. The rules themselves are fit.luau.
local rules = require("./rules")
local fit = require("./fit")
local log = require("./log")

local cradle = {}

-- Every part in the world, in tray order, as fit.luau tables.
function cradle.parts(ctx)
    local list = {}
    for _, id in ctx.query("part") do
        local p = ctx.get(id, "part")
        p.id = id
        table.insert(list, p)
    end
    table.sort(list, function(a, b) return a.order < b.order end)
    return list
end

-- Sets the ship to what its fit adds up to: the hold, the beam, top speed and turn
-- rate, and its look (fitted pods show on the hull). `parts` is after the caller's
-- writes.
function cradle.apply(ctx, parts)
    local t = fit.totals(parts, nil, nil, fit.hull_speed(ctx))
    ctx.patch("ship", "fit", { hold = t.hold, mine = t.mine, power = t.power, mass = t.mass })
    ctx.patch("ship", "engine.flight", { speed = t.top, turn = t.turn })
    local look = "core.ship"
    for _, p in parts do
        if p.fitted and p.model ~= "" then look = p.model end
    end
    ctx.patch("ship", "engine.model", { model = look })
end

-- The open room, if it is a fitting cradle: its ID and facility.
local function at_cradle(ctx)
    local pilot = ctx.get("ship", "pilot")
    if pilot.room == "" then return nil end
    local facility = ctx.get(pilot.room, "facility")
    if not facility or not facility.fits then return nil end
    return pilot.room, facility, pilot
end

local function find(parts, id)
    for _, p in parts do
        if p.id == id then return p end
    end
    return nil
end

-- The fitter's next line: flavour about what just changed, never the receipt again.
local function say(ctx, facility, text)
    ctx.patch(facility.keeper .. "_now", "engine.talk_line", { text = text })
end

-- A part dropped on a socket: bought if it isn't yours yet, then fitted.
function cradle.fit(event, ctx)
    local room, facility, pilot = at_cradle(ctx)
    if not room then return end
    local parts = cradle.parts(ctx)
    local p = find(parts, event.payload.value)
    local socket = event.payload.socket
    if not p or not socket then return end
    local load = rules.load(ctx.get("ship", "hold"))
    if fit.verdict(p, socket, parts, pilot.credits, load) ~= "" then return end
    local cost = p.owned and 0 or p.price
    p.owned, p.fitted = true, true
    ctx.patch(p.id, "part", { owned = true, fitted = true })
    cradle.apply(ctx, parts)
    local name = string.lower(p.name)
    local where = ctx.get(facility.station, "port").name
    ctx.patch("ship", "pilot", { credits = pilot.credits - cost, sale = -cost,
        deal = "Fitted the " .. name .. (cost > 0 and "." or ", from the locker.") })
    say(ctx, facility, p.said)
    log.add(ctx, "Fitted the " .. name .. " at " .. where .. (cost > 0 and " for " .. cost .. " cr" or "") .. ": " .. p.effect .. ".")
    ctx.record("fitted", { part = p.id, credits = cost })
end

-- A fitted part dropped on the tray: taken off, into the locker.
function cradle.unfit(event, ctx)
    local room, facility, pilot = at_cradle(ctx)
    if not room then return end
    local parts = cradle.parts(ctx)
    local p = find(parts, event.payload.value)
    if not p then return end
    local load = rules.load(ctx.get("ship", "hold"))
    if fit.verdict(p, "locker", parts, pilot.credits, load) ~= "" then return end
    p.fitted = false
    ctx.patch(p.id, "part", { fitted = false })
    cradle.apply(ctx, parts)
    local name = string.lower(p.name)
    ctx.patch("ship", "pilot", { sale = 0, deal = "Took off the " .. name .. ", into your locker." })
    say(ctx, facility, "The locker keeps it safe till you want it back on.")
    log.add(ctx, "Took off the " .. name .. " at " .. ctx.get(facility.station, "port").name .. ", into your locker: fit it again here for nothing.")
    ctx.record("fitted", { part = p.id, credits = 0, off = true })
end

-- Widths: three socket columns across the room panel's 488px inside.
local COLUMN, INSIDE = 157, 141
-- Card heights: a fitted part shows its name and effect; one in the tray adds its
-- price (or that it's yours) and what it draws and weighs.
local FITTED, LOOSE = 60, 96
-- Every socket is as tall as the one with the longest refusal, so the row lines up.
local SOCKET = 160

local function short(id)
    return string.match(id, "[^.]+$")
end

-- A part's card. Its hover says the effect in words.
local function card(ui, p)
    local text = p.name .. "\n" .. fit.effect(p)
    if not p.fitted then
        text = p.name .. "\n" .. (p.owned and "In your locker" or p.price .. " cr") .. "\n" .. fit.effect(p) .. "\n" .. fit.cost(p)
    end
    return ui.drag_source { id = "part_" .. short(p.id), value = p.id, width = INSIDE, height = p.fitted and FITTED or LOOSE, size = 16, pad = 8,
        text = text, hover = p.name .. ": " .. p.effect .. "." }
end

-- Holds a card's place where a socket or the tray has none, so every column keeps its
-- size whatever is in it.
local function space(ui, id, height, text)
    return ui.text { id = id, width = INSIDE, height = height, size = 16, text = text }
end

-- A receipt's kind, as a word in a box (direction doc 1.5 and 4: "Confirmed").
function cradle.confirmed(ui)
    return ui.panel { id = "confirmed", width = 96, pad = 2, children = { ui.text { text = "CONFIRMED", size = 16 } } }
end

-- The whole fitting room: its title with the keeper (or the last receipt) and Back,
-- the budgets, the sockets, the controls and the parts tray.
function cradle.rows(ctx, ui, pilot, title, keeper)
    local parts = cradle.parts(ctx)
    local load = rules.load(ctx.get("ship", "hold"))
    local t = fit.totals(parts, nil, nil, fit.hull_speed(ctx))
    -- Under the title, the keeper; after a fit, the receipt instead, its CONFIRMED tag
    -- beside the title so the heading keeps its height.
    local named = { ui.text { id = "title", width = 196, text = title, size = 28 } }
    local under = ui.text { id = "keeper", width = 296, size = 16, text = keeper or "" }
    if pilot.deal ~= "" then
        named = { ui.row { id = "titled", gap = 4, children = { named[1], cradle.confirmed(ui) } } }
        local row = { ui.text { id = "deal", width = pilot.sale < 0 and 220 or 296, text = pilot.deal, size = 16 } }
        if pilot.sale < 0 then table.insert(row, ui.delta { id = "spent", width = 68, value = pilot.sale, size = 16 }) end
        under = ui.row { id = "receipt", gap = 8, children = row }
    end
    table.insert(named, under)
    local children = {
        ui.row { id = "heading", gap = 12, children = {
            ui.column { id = "named", width = 296, gap = 2, children = named },
            ui.button { id = "close", width = 180, text = "Back  [B]", on = "close" },
        } },
        ui.text { id = "budget", width = 488, size = 16,
            text = fit.power_text(t) .. "\n" .. fit.mass_text(t) .. "\n" .. fit.ship_text(t) },
    }
    -- The sockets: each shows its part, or that it is empty; dropping on it fits.
    local sockets = {}
    for _, s in fit.SOCKETS do
        local accepts, inside = {}, nil
        for _, p in parts do
            accepts[p.id] = fit.verdict(p, s.id, parts, pilot.credits, load)
            if p.fitted and p.socket == s.id then inside = card(ui, p) end
        end
        table.insert(sockets, ui.drop_target { id = "socket_" .. s.id, width = COLUMN, height = SOCKET, size = 16, pad = 8,
            text = s.name, on = "fit", payload = { socket = s.id }, accepts = accepts,
            children = { inside or space(ui, "empty_" .. s.id, FITTED, "Empty") } })
    end
    table.insert(children, ui.row { id = "sockets", gap = 8, children = sockets })
    -- The control rack: the hand controls, always fitted, then controls to buy (parts in
    -- the "rack" socket). A control costs no power or mass and, once bought, never comes
    -- off (direction doc 5); one that can't be bought yet rests with its reason.
    local rack = { ui.text { id = "rack", width = 80, size = 16, text = "Controls",
        hover = "Always fitted: the hand cutter and hand berthing. Controls cost no power or mass and never come off." } }
    for _, p in parts do
        if p.socket == "rack" then
            local about = p.name .. ": " .. p.effect .. "."
            if p.fitted then
                table.insert(rack, ui.text { id = "rack_" .. short(p.id), width = 196, size = 16, text = p.name .. ", fitted", hover = about })
            else
                local why = fit.verdict(p, "rack", parts, pilot.credits, load)
                table.insert(rack, ui.button { id = "rack_" .. short(p.id), width = 196, height = 44, size = 16, pad = 4,
                    text = p.name .. " · " .. p.price .. " cr", hover = about, resting = why ~= "" and why or nil,
                    on = "fit", payload = { socket = "rack", value = p.id } })
            end
        end
    end
    table.insert(children, ui.row { id = "controls", gap = 8, children = rack })
    -- The tray: parts on the rack to buy and parts in your locker, each under its own
    -- socket. Dropping a fitted part here takes it off.
    local accepts, tray = {}, {}
    for _, s in fit.SOCKETS do
        local here = nil
        for _, p in parts do
            if p.socket == s.id and not p.fitted then here = card(ui, p) end
        end
        table.insert(tray, here or space(ui, "off_" .. s.id, LOOSE, "On the ship"))
    end
    for _, p in parts do
        accepts[p.id] = fit.verdict(p, "locker", parts, pilot.credits, load)
    end
    table.insert(children, ui.drop_target { id = "tray", width = 488, size = 16, pad = 8, on = "unfit", accepts = accepts,
        text = "Parts tray: drop a part here to take it off",
        children = { ui.row { id = "tray_parts", gap = 24, children = tray } } })
    return children
end

return cradle
