local rules = require("./rules")
local log = require("./log")
local teach = require("./teach")
local trade = require("./trade")
local signs = require("./signs")
local fit = require("./fit")
local undock = require("./launch")

-- Which hint each held key teaches.
local TEACHES = { ahead = "thrust", back = "thrust", left = "turn", right = "turn", boost = "boost" }

local function space(ctx)
    return ctx.get("ship", "engine.location").place == "core.reach"
end

local function pressed(event)
    return event.payload.down ~= false
end

local function message(ctx, text)
    ctx.patch("ship", "pilot", { message = text })
end

local function stop(ctx)
    ctx.put("ship", "engine.helm", {})
    ctx.put("ship", "engine.steer", {})
    ctx.put("ship", "held", {})
    ctx.patch("ship", "pilot", { mining = false, target = "" })
end

-- The keys that fly the ship off the dock: the movement keys, not boost alone.
local MOVES = { ahead = true, back = true, left = true, right = true }

local function flight(event, ctx)
    local key = string.sub(event.kind, 6)
    local held = ctx.get("ship", "held")
    if not space(ctx) then
        -- Docked, a movement key simply flies the ship off: there is no undock step.
        local station = undock.docked(ctx)
        if not station or not MOVES[key] or event.payload.down ~= true then return end
        undock.launch(ctx, station)
        held = { ahead = false, back = false, left = false, right = false, boost = false }
    end
    held[key] = event.payload.down == true
    ctx.patch("ship", "held", { [key] = held[key] })
    local function axis(a, b)
        return (held[a] and 1 or 0) - (held[b] and 1 or 0)
    end
    ctx.patch("ship", "engine.helm", {
        thrust = axis("ahead", "back"), turn = axis("left", "right"), boost = held.boost,
    })
    if pressed(event) then
        teach.used(ctx, TEACHES[key])
        local ordered = ctx.get("ship", "engine.steer").mode ~= ""
        ctx.put("ship", "engine.steer", {})
        ctx.patch("ship", "pilot", { mining = false, target = "" })
        if ordered then message(ctx, "Order dropped: flying by hand.") end
    end
end

local function nearest(ctx, component)
    local at = ctx.get("ship", "engine.position")
    local best, found = math.huge, nil
    for _, id in ctx.query(component) do
        local usable = component ~= "ore" or ctx.get(id, "ore").left > 0
        if usable then
            local positionId = component == "port" and ctx.get(id, "port").berth or id
            local d = rules.distance2(at, ctx.get(positionId, "engine.position"))
            if d < best then best, found = d, id end
        end
    end
    return found
end

local function mine(event, ctx)
    if not pressed(event) or not space(ctx) then return end
    teach.used(ctx, "mine")
    if rules.load(ctx.get("ship", "hold")) >= fit.hold(ctx) then message(ctx, "Hold full. Dock to sell or trade."); return end
    local target = nearest(ctx, "ore")
    if not target then message(ctx, "The field is worked out. Your ore still has a buyer."); return end
    stop(ctx)
    -- The ship parks on the rock's drawn surface, with no gap.
    ctx.put("ship", "engine.steer", { mode = "hold", target = target, range = 0 })
    ctx.patch("ship", "pilot", { mining = true, target = target, message = "Heading for the nearest ore." })
end

local function dock(event, ctx)
    if not pressed(event) or not space(ctx) then return end
    teach.used(ctx, "dock")
    local station = event.payload.station
    if station ~= "core.lantern" and station ~= "core.kiln" then station = nearest(ctx, "port") end
    local port = ctx.get(station, "port")
    stop(ctx)
    ctx.put("ship", "engine.steer", { mode = "dock", target = port.berth })
    ctx.patch("ship", "pilot", { station = station, message = "Approaching " .. port.name .. "." })
end

return {
    on = {
        ahead = flight, back = flight, left = flight, right = flight, boost = flight,
        mine = mine, dock = dock,
        cancel = function(event, ctx)
            if pressed(event) and space(ctx) then
                teach.used(ctx, "cancel")
                stop(ctx)
                message(ctx, "Order cancelled. Coasting to a stop.")
            end
        end,
        ["engine.arrived"] = function(event, ctx)
            if event.payload.entity ~= "core.ship" then return end -- traders: traffic.luau
            local pilot = ctx.get("ship", "pilot")
            if pilot.station == "" or not space(ctx) then return end
            local port = ctx.get(pilot.station, "port")
            if event.payload.mode ~= "dock" or event.payload.target ~= port.berth then return end
            stop(ctx)
            ctx.remove("ship", "engine.body")
            ctx.patch("ship", "pilot", { room = "", sale = 0, deal = "", last_dock = pilot.station, away = false, gliding = true })
            ctx.patch("ship", "engine.model", { scale = 1.3 }) -- moored, seen up close
            ctx.move("ship", pilot.station)
            signs.show(ctx, pilot.station, "", pilot.log_open, nil, true) -- up once the glide in lands
            -- A visit only counts once the ship has really been away: then the
            -- station's market eases back towards its usual stock.
            local counted = rules.counts(pilot.last_dock, pilot.away, pilot.station)
            if counted then
                ctx.patch(pilot.station, "port", { visits = port.visits + 1 })
                for _, id in trade.station_lines(ctx, pilot.station) do
                    local line = ctx.get(id, "market")
                    local stock = rules.settle(line)
                    ctx.patch(id, "market", { stock = stock })
                    if line.ware == "ore" then
                        ctx.patch(pilot.station .. "_stock", "engine.model", { scale = trade.pile(stock) })
                    end
                end
            end
            log.add(ctx, "Docked at " .. port.name .. ".")
            ctx.record("docked", { station = pilot.station, counted = counted })
        end,
    },
    cadence = function(ctx)
        local pilot = ctx.get("ship", "pilot")
        if not space(ctx) then return end
        local at = ctx.get("ship", "engine.position")
        -- Flown well clear of the last berth: the next docking there is a new visit.
        if pilot.last_dock ~= "" and not pilot.away then
            local berth = ctx.get(ctx.get(pilot.last_dock, "port").berth, "engine.position")
            if rules.distance2(at, berth) > rules.AWAY ^ 2 then ctx.patch("ship", "pilot", { away = true }) end
        end
        if not pilot.mining or pilot.target == "" then return end
        local ore = ctx.get(pilot.target, "ore")
        if rules.distance2(at, ctx.get(pilot.target, "engine.position")) > rules.MINE_DISTANCE ^ 2 then return end
        local hold = ctx.get("ship", "hold")
        local capacity = fit.hold(ctx)
        local amount = rules.transfer(rules.load(hold), ore.left, capacity, fit.mine(ctx))
        if amount == 0 then
            ctx.patch("ship", "pilot", { mining = false })
            -- The keys for what comes next are the hint strip's to teach (hud.luau).
            if rules.load(hold) == capacity then
                log.add(ctx, "Hold full: " .. rules.list(hold) .. " aboard.")
            else
                log.add(ctx, "Rock worked out.")
            end
            return
        end
        -- Each rock yields its own ware: ore, ice or glassite.
        local ware = ore.ware ~= "" and ore.ware or "ore"
        ctx.patch("ship", "hold", { [ware] = (hold[ware] or 0) + amount })
        ctx.patch("ship", "pilot", { message = "+" .. amount .. " " .. ware .. " aboard.", away = true })
        ctx.patch(pilot.target, "ore", { left = ore.left - amount })
        ctx.patch(pilot.target, "engine.model", { morph = (ore.total - ore.left + amount) / ore.total })
        ctx.record("mined", { rock = pilot.target, units = amount })
    end,
}
