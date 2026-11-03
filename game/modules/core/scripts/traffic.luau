-- Traffic out in the reach: traders flying their own routes between the stations. Each
-- flies a leg's waypoints without stopping, docks onto a mooring off a station's far
-- side, waits there a while, then flies the other leg back. The engine flies them and
-- they are solid like everything else; this script only gives the orders.
--
-- Their name labels show only where they read cleanly: out in open space near the
-- player's ship, clear of the stations and of each other's labels (labels.luau). The
-- screen decides that every tick, as things move. Pointing at a trader always names it.
local labels = require("./labels")

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
    if not trader then return end
    if event.payload.mode == "follow" then
        -- The leg's waypoints are flown: dock onto its mooring, facing the station.
        ctx.put(id, "engine.steer", { mode = "dock", target = trader[trader.leg .. "_moor"] })
    elseif event.payload.mode == "dock" then
        ctx.patch(id, "trader", { moored = true, since = ctx.tick })
    end
end

local function cadence(ctx)
    for _, id in ctx.query("trader") do
        local trader = ctx.get(id, "trader")
        if trader.moored and ctx.tick - trader.since >= DWELL then depart(ctx, id, trader) end
    end
end

local function screen(ctx, ui)
    if ctx.get("ship", "engine.location").place ~= "core.reach" then return nil end
    local ids, list = ctx.query("trader"), {}
    for _, id in ids do
        table.insert(list, { id = id, at = ctx.get(id, "engine.position"), moored = ctx.get(id, "trader").moored })
    end
    local ship = ctx.get("ship", "engine.position")
    local shown = labels.shown(ship, list, stations(ctx))
    local children = {}
    for _, id in ids do
        local trader = ctx.get(id, "trader")
        -- Pointing at a trader names it, label or not.
        table.insert(children, ui.hover { id = "hover_" .. string.sub(id, 6), anchor = id .. "#label",
            text = trader.name .. ": " .. trader.about })
        if shown[id] then
            -- Above the trader by default; below it when the player's ship is above.
            local at = ctx.get(id, "engine.position")
            table.insert(children, ui.text { id = "label_" .. string.sub(id, 6), anchor = id .. "#label",
                y = labels.below(ship, at) and 40 or 0, width = #trader.name * 8 + 14, text = trader.name, size = 16 })
        end
    end
    return ui.column { id = "traffic", children = children }
end

return { on = { ["engine.arrived"] = arrived }, cadence = cadence, screen = screen }
