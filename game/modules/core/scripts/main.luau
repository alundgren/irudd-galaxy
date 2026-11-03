local rules = require("./rules")
local log = require("./log")
local teach = require("./teach")

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

local function flight(event, ctx)
    if not space(ctx) then return end
    local held = ctx.get("ship", "held")
    local key = string.sub(event.kind, 6)
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
    local pilot = ctx.get("ship", "pilot")
    if pilot.cargo >= rules.CAPACITY then message(ctx, "Hold full. Dock and sell your six ore."); return end
    local target = nearest(ctx, "ore")
    if not target then message(ctx, "The field is worked out. Your ore still has a buyer."); return end
    stop(ctx)
    ctx.put("ship", "engine.steer", { mode = "hold", target = target, range = 1.5 })
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
            local pilot = ctx.get("ship", "pilot")
            if pilot.station == "" or not space(ctx) then return end
            local port = ctx.get(pilot.station, "port")
            if event.payload.mode ~= "dock" or event.payload.target ~= port.berth then return end
            stop(ctx)
            ctx.remove("ship", "engine.body")
            ctx.patch("ship", "pilot", { room = "", sale = 0 })
            ctx.patch("ship", "engine.model", { scale = 1.3 }) -- moored, seen up close
            ctx.move("ship", pilot.station)
            log.add(ctx, "Docked at " .. port.name .. ".")
            ctx.record("docked", { station = pilot.station })
        end,
    },
    cadence = function(ctx)
        local pilot = ctx.get("ship", "pilot")
        if not space(ctx) or not pilot.mining or pilot.target == "" then return end
        local ore = ctx.get(pilot.target, "ore")
        if rules.distance2(ctx.get("ship", "engine.position"), ctx.get(pilot.target, "engine.position")) > rules.MINE_DISTANCE ^ 2 then return end
        local amount = rules.transfer(pilot.cargo, ore.left)
        if amount == 0 then
            ctx.patch("ship", "pilot", { mining = false })
            local taught = ctx.get("ship", "taught")
            if pilot.cargo == rules.CAPACITY then
                log.add(ctx, "Hold full: " .. pilot.cargo .. " ore aboard." .. (teach.new(taught, "dock") and " E docks at the nearest station." or ""))
            else
                log.add(ctx, "Rock worked out." .. (teach.new(taught, "mine") and " Space finds the next one." or ""))
            end
            return
        end
        ctx.patch("ship", "pilot", { cargo = pilot.cargo + amount, message = "+1 ore aboard." })
        ctx.patch(pilot.target, "ore", { left = ore.left - amount })
        ctx.patch(pilot.target, "engine.model", { morph = (ore.total - ore.left + amount) / ore.total })
        ctx.record("mined", { rock = pilot.target, units = amount })
    end,
}
