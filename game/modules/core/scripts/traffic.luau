-- Traffic out in the reach: traders flying their own routes between the stations. Each
-- flies a leg's waypoints without stopping, docks onto a mooring off a station's far
-- side, waits there a while, then flies the other leg back. The engine flies them and
-- they are solid like everything else; this script only gives the orders.
--
-- Their name labels show at any distance where they read cleanly: clear of the
-- stations (labels.luau), and, as `yield` labels, never off screen, over the HUD or over
-- each other's labels. The screen decides that every tick, as things move. Pointing at
-- a trader always names it.
local labels = require("./labels")
local kit = require("./kit")
local salvage = require("./salvage")
local log = require("./log")
local cutter = require("./cutter")

local DWELL = 240 -- ticks a trader waits at a mooring (8 seconds)

-- The stations' outsides, which labels keep clear of.
local function stations(ctx)
    local list = {}
    for _, id in ctx.query("port") do
        local at = ctx.get(id .. "_exterior", "engine.position")
        if at then table.insert(list, at) end
    end
    return list
end

-- Sets off along the leg after `leg`.
local function depart(ctx, id, trader)
    local leg = trader.leg == "out" and "home" or "out"
    ctx.patch(id, "trader", { leg = leg, moored = false })
    ctx.put(id, "engine.steer", { mode = "follow", target = trader[leg] })
end

local function arrived(event, ctx)
    local id = event.payload.entity
    local trader = id and ctx.get(id, "trader")
    if not trader or trader.stopped then return end -- taken off its route (salvage.luau)
    if event.payload.mode == "follow" then
        -- The leg's waypoints are flown: dock onto its mooring, facing the station.
        ctx.put(id, "engine.steer", { mode = "dock", target = trader[trader.leg .. "_moor"] })
    elseif event.payload.mode == "dock" then
        ctx.patch(id, "trader", { moored = true, since = ctx.tick })
    end
end

-- A skipper with something to say (`hail`) hails the ship once over the radio: a person
-- and a question to carry, early. Once the pilot has cut a first hold, the hail comes
-- when the ship flies within HAIL of them or they come in to the station they serve
-- (moored at the end of their out leg), whichever is first. It comes while she flies
-- (faster than FLYING): never while the beam cuts or she lies still at a rock, so it
-- doesn't cover the hold-full message, and never while a dock order flies her in, so the
-- docking's message doesn't cover it.
local HAIL = 150
local FLYING = 8

local function hail(ctx)
    if ctx.get("ship", "engine.location").place ~= "core.reach" then return end
    local pilot = ctx.get("ship", "pilot")
    if pilot.holds < 1 or pilot.mining or cutter.speed(ctx) <= FLYING then return end
    local steer = ctx.get("ship", "engine.steer")
    if steer and steer.mode == "dock" then return end
    local ship = ctx.get("ship", "engine.position")
    for _, id in ctx.query("trader") do
        local trader = ctx.get(id, "trader")
        if (trader.hail or "") ~= "" and not trader.hailed and not trader.stopped then
            local at = ctx.get(id, "engine.position")
            local dx, dy = at.x - ship.x, at.y - ship.y
            local served = trader.moored and trader.leg == "out"
            if served or dx * dx + dy * dy <= HAIL * HAIL then
                ctx.patch(id, "trader", { hailed = true })
                log.add(ctx, trader.hail)
                return
            end
        end
    end
end

local function cadence(ctx)
    for _, id in ctx.query("trader") do
        local trader = ctx.get(id, "trader")
        if trader.moored and not trader.stopped and ctx.tick - trader.since >= DWELL then depart(ctx, id, trader) end
    end
    hail(ctx)
end

local function screen(ctx, ui)
    if ctx.get("ship", "engine.location").place ~= "core.reach" then return nil end
    -- While the view still glides out after a launch, the traders it passes may be
    -- beside or behind it: their labels and hovers wait until it lands.
    if ctx.get("ship", "pilot").gliding then return nil end
    local ids, list = ctx.query("trader"), {}
    for _, id in ids do
        table.insert(list, { id = id, at = ctx.get(id, "engine.position"), moored = ctx.get(id, "trader").moored })
    end
    local ship = ctx.get("ship", "engine.position")
    local shown = labels.shown(ship, list, stations(ctx))
    local children = {}
    for _, id in ids do
        local trader = ctx.get(id, "trader")
        -- A barge a job has taken off her route says so: adrift, or in tow.
        local tag, about = salvage.tag(ctx, id)
        local name = trader.name .. (tag or "")
        -- Pointing at a trader names it, label or not. The job's barge is a click target
        -- while a click on her does something: it passes her a line (tow.luau) as Space
        -- does, adrift or lying off a berth, and casts it off in tow, as 0 does.
        local on, said = salvage.click(ctx, id)
        if on then
            table.insert(children, ui.target { id = "hover_" .. string.sub(id, 6), anchor = id .. "#label",
                text = trader.name .. ": " .. said, on = on, payload = { barge = id } })
        else
            table.insert(children, ui.hover { id = "hover_" .. string.sub(id, 6), anchor = id .. "#label",
                text = trader.name .. ": " .. (about or trader.about) })
        end
        if shown[id] then
            -- Above the trader by default; below it when the player's ship is above.
            local at = ctx.get(id, "engine.position")
            table.insert(children, kit.label(ui, { id = "label_" .. string.sub(id, 6), anchor = id .. "#label",
                y = labels.below(ship, at) and 40 or 0, width = #name * 8 + 14, text = name, size = kit.SIZE.small,
                yield = true }))
        end
    end
    return ui.column { id = "traffic", children = children }
end

return { on = { ["engine.arrived"] = arrived }, cadence = cadence, screen = screen }
