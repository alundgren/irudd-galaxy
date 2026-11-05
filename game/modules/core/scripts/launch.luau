-- Leaving a station: from its berth back out into the reach, with the controls clear.
-- U, the Launch button and any movement key (W, A, S or D) all leave this way; a
-- movement key then flies on at once (main.luau), so there is no separate undock step.
local log = require("./log")
local signs = require("./signs")
local places = require("./places")

-- The station the ship is docked at, on its deck or in one of its rooms, or nil.
local function docked(ctx)
    return places.docked(ctx)
end

-- The heading opposite `angle`, in -pi..pi.
local function outward(angle)
    local a = angle + math.pi
    if a > math.pi then a -= 2 * math.pi end
    return a
end

local function launch(ctx, station)
    local port = ctx.get(station, "port")
    ctx.put("ship", "engine.helm", {})
    ctx.put("ship", "engine.steer", {})
    ctx.put("ship", "held", {})
    -- The view glides out (below) and the traders' labels wait until it lands: from a
    -- room it starts low among them (scripts/traffic.luau).
    ctx.patch("ship", "pilot", { room = "", mining = false, target = "", log_open = false, gliding = true })
    signs.show(ctx, station, "", false, nil, true) -- worn again once the next visit settles
    -- Out glides back to the ship's view, as docking glided in, from the deck or from
    -- a room (whose glides are off otherwise, so going in and out of them cuts:
    -- scripts/station.luau).
    local here = ctx.get("ship", "engine.location").place
    ctx.patch(here, "engine.room", { glide = 1.0 })
    ctx.move("ship", "core.reach", port.departure)
    -- Moored, the ship faces the station; it leaves facing away, so W flies straight
    -- out instead of into the station's hull.
    local berth = ctx.get(port.berth, "engine.position")
    ctx.patch("ship", "engine.position", { angle = outward(berth.angle) })
    ctx.put("ship", "engine.body", { mass = 1 })
    ctx.patch("ship", "engine.model", { scale = 1 })
    log.leave(ctx, "Launched from " .. port.name .. ".")
end

return { docked = docked, launch = launch }
