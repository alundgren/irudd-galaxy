local rules = require("./rules")
local log = require("./log")
local teach = require("./teach")
local trade = require("./trade")
local signs = require("./signs")
local fit = require("./fit")
local undock = require("./launch")
local cutter = require("./cutter")

-- Which hint each held key teaches.
local TEACHES = { ahead = "thrust", back = "thrust", left = "turn", right = "turn", boost = "boost" }

-- The ship's hold, where cut stone lands: a little behind the canopy.
local HOLD_X = -0.4

local function space(ctx)
    return ctx.get("ship", "engine.location").place == "core.reach"
end

local function pressed(event)
    return event.payload.down ~= false
end

-- An order a script gives (a scenario's route, another module's pilot) flies itself,
-- as the fitted parts do; the player's keys and clicks fly by hand.
local function scripted(event)
    return event.source == "script"
end

local function message(ctx, text)
    ctx.patch("ship", "pilot", { message = text })
end

local function stop(ctx)
    ctx.put("ship", "engine.helm", {})
    ctx.put("ship", "engine.steer", {})
    ctx.put("ship", "held", {})
    ctx.put("ship", "engine.beam", {})
    ctx.patch("ship", "pilot", { mining = false, target = "", latched = false, auto = false, cut_from = 0, cut_work = 0,
        approach = false })
end

-- The keys that fly the ship off the dock: the movement keys, not boost alone.
local MOVES = { ahead = true, back = true, left = true, right = true }

-- The follow camera's feel: it widens a little at speed and looks a quarter second
-- ahead (start.toml has the same). A beat worth feeling kicks it by `share` of the view.
local CAMERA = { speed_zoom = 0.12, lead = 0.25 }
local function kick(ctx, share)
    ctx.put("ship", "engine.camera_feel", { speed_zoom = CAMERA.speed_zoom, lead = CAMERA.lead, shake = share,
        shake_seconds = 0.35, shake_tick = ctx.tick })
end

-- Starts the beam on the rock in the nose's sights, or says why there is none.
local function cut_ahead(ctx, latched)
    local sighted, near, gap = cutter.sighted(ctx)
    if sighted then
        cutter.on(ctx, sighted, latched)
        return true
    end
    if not near then
        local hold = ctx.get("ship", "hold")
        message(ctx, rules.worked_out(rules.load(hold), rules.list(hold), cutter.buyers(ctx, hold)))
    elseif gap <= cutter.reach(ctx) then
        message(ctx, "Point your nose at the rock to cut it: A or D turns.")
    else
        message(ctx, "No rock in reach. The cutter reaches " .. cutter.reach(ctx) .. " units: fly closer.")
    end
    return false
end

-- The rig (or a script's order) flies out to `rock` and holds off it while it cuts. The
-- rig cuts at once from where it is if it can; a script's order parks the ship on the
-- rock's surface (`standoff` 0) and cuts there.
local function approach(ctx, rock, standoff)
    ctx.put("ship", "engine.helm", {})
    if cutter.can_cut(ctx, rock, true) then
        if standoff > 0 then
            ctx.put("ship", "engine.steer", {})
        else
            ctx.put("ship", "engine.steer", { mode = "hold", target = rock, range = 0 })
        end
        cutter.on(ctx, rock, true, true)
        return
    end
    ctx.put("ship", "engine.steer", { mode = "hold", target = rock, range = standoff })
    ctx.put("ship", "engine.beam", {})
    local pilot = ctx.get("ship", "pilot")
    ctx.patch("ship", "pilot", { mining = false, target = rock, latched = true, auto = true, approach = true, cut_from = 0,
        cut_work = pilot.target == rock and pilot.cut_work or 0,
        message = standoff > 0 and "The rig takes her out to the rock." or "Heading for the nearest ore." })
end

local function flight(event, ctx)
    local key = string.sub(event.kind, 6)
    local held = ctx.get("ship", "held")
    if not space(ctx) then
        -- Docked, a movement key simply flies the ship off: there is no undock step.
        local station = undock.docked(ctx)
        if not station or not MOVES[key] or event.payload.down ~= true then return end
        undock.launch(ctx, station)
        held = { ahead = false, back = false, left = false, right = false, boost = false, mine = false }
    end
    held[key] = event.payload.down == true
    ctx.patch("ship", "held", { [key] = held[key] })
    local function axis(a, b)
        return (held[a] and 1 or 0) - (held[b] and 1 or 0)
    end
    ctx.patch("ship", "engine.helm", {
        thrust = axis("ahead", "back"), turn = axis("left", "right"), boost = held.boost,
    })
    local pilot = ctx.get("ship", "pilot")
    if pressed(event) then
        teach.used(ctx, TEACHES[key])
        if not MOVES[key] then return end
        local ordered = ctx.get("ship", "engine.steer").mode ~= ""
        ctx.put("ship", "engine.steer", {})
        if ordered then
            ctx.put("ship", "engine.beam", {})
            ctx.patch("ship", "pilot", { mining = false, target = "", latched = false, cut_from = 0, approach = false,
                message = "Order dropped: flying by hand." })
        elseif pilot.mining then
            -- The beam is held on a rock by a steady ship: flying lifts it off, the
            -- rig's and a click's as well as the hand cutter's. Letting go with Space
            -- still held puts the hand cutter back.
            cutter.off(ctx, pilot.latched and "Beam off: flying by hand." or nil)
        end
    elseif MOVES[key] and held.mine and not pilot.mining and space(ctx)
        and not (held.ahead or held.back or held.left or held.right) then
        cut_ahead(ctx, false)
    end
end

local function dock_at(ctx, station)
    local port = ctx.get(station, "port")
    stop(ctx)
    ctx.put("ship", "engine.steer", { mode = "dock", target = port.berth })
    ctx.patch("ship", "pilot", { station = station })
end

-- The berth's clamps take the ship in: it is in the ring, so the last metre is theirs.
local function clamp(ctx, station)
    local port = ctx.get(station, "port")
    dock_at(ctx, station)
    ctx.put("ship", "engine.burst", { recipe = "core.puff", tick = ctx.tick, x = -1.2 })
    ctx.patch("ship", "pilot", { message = port.name .. "'s clamps have you." })
    ctx.record("clamped", { station = station })
end

local function mine(event, ctx)
    if not space(ctx) then return end
    local rock = event.payload.rock
    local held = ctx.get("ship", "held")
    local pilot = ctx.get("ship", "pilot")
    if event.payload.down == false then
        -- Space let go: the hand cutter stops; a latched beam keeps cutting.
        ctx.patch("ship", "held", { mine = false })
        ctx.patch("ship", "engine.helm", { brake = false })
        if pilot.mining and not pilot.latched then cutter.off(ctx, nil) end
        return
    end
    teach.used(ctx, "mine")
    local key = rock == nil and not scripted(event)
    if key then
        -- Holding Space brakes, so the ship sits steady on the rock it cuts.
        ctx.patch("ship", "held", { mine = true })
        ctx.patch("ship", "engine.helm", { brake = true })
        -- In a berth's ring, Space docks: once the ship has cleared the one it left.
        local ring = cutter.ring(ctx)
        if ring and pilot.cleared then clamp(ctx, ring); return end
    end
    if rock ~= nil and pilot.mining and pilot.target == rock then cutter.off(ctx, "Beam off."); return end
    if rules.load(ctx.get("ship", "hold")) >= fit.hold(ctx) then message(ctx, "Hold full. Dock to sell or trade."); return end
    if rock ~= nil and (not ctx.get(rock, "ore") or ctx.get(rock, "ore").left <= 0) then
        message(ctx, "That rock is cut down to pebbles.")
        return
    end
    local rig = cutter.has(ctx, cutter.RIG)
    if scripted(event) or rig then
        -- The rig, like a script's order, picks the rock and flies out to it itself.
        local target = rock
        if target == nil then
            local sighted, near = cutter.sighted(ctx)
            target = sighted or near
        end
        if not target then cut_ahead(ctx, true); return end
        if pilot.mining and pilot.target == target and key then cutter.off(ctx, "Beam off."); return end
        approach(ctx, target, rig and rules.RIG_STANDOFF or 0)
        return
    end
    if rock ~= nil then
        -- A click on a rock: the beam cuts it while the ship holds steady, as Space does.
        if cutter.can_cut(ctx, rock) then
            cutter.on(ctx, rock, true)
        else
            local at = ctx.get("ship", "engine.position")
            local centre, radius = cutter.rock(ctx, rock)
            if rules.gap(at, centre, radius) > cutter.reach(ctx) then
                message(ctx, "Too far to cut. The cutter reaches " .. cutter.reach(ctx) .. " units: fly closer.")
            else
                message(ctx, "Point your nose at the rock to cut it: A or D turns.")
            end
        end
        return
    end
    cut_ahead(ctx, false)
end

local function dock(event, ctx)
    if not pressed(event) or not space(ctx) then return end
    teach.used(ctx, "dock")
    local station = event.payload.station
    if station ~= nil and not ctx.get(station, "port") then station = nil end
    local ring = cutter.ring(ctx)
    if ring and (station == nil or station == ring) then clamp(ctx, ring); return end
    if scripted(event) or cutter.has(ctx, cutter.JETS) then
        if station == nil then
            local at = ctx.get("ship", "engine.position")
            local best = math.huge
            for _, id in ctx.query("port") do
                local d = rules.distance2(at, ctx.get(ctx.get(id, "port").berth, "engine.position"))
                if d < best then best, station = d, id end
            end
        end
        dock_at(ctx, station)
        local name = ctx.get(station, "port").name
        message(ctx, scripted(event) and "Approaching " .. name .. "." or "The docking jets take her into " .. name .. ".")
        return
    end
    local name = station and ctx.get(station, "port").name or "a station"
    message(ctx, "Fly into " .. name .. "'s berth ring, the glowing ring off its dock, and the clamps take you in.")
end

-- One second of the beam on its rock: what it has cut lands in the hold.
local function cut(ctx, pilot)
    local rock = pilot.target
    -- A save from before beam time was kept has no start: its order's beam starts now.
    if pilot.cut_from <= 0 then
        cutter.on(ctx, rock, true, true)
        return
    end
    local ore = ctx.get(rock, "ore")
    if not ore or ore.left <= 0 or not cutter.can_cut(ctx, rock, pilot.auto) then
        local rig = cutter.has(ctx, cutter.RIG)
        cutter.off(ctx, rig and "The rock is out of the rig's reach." or "The beam slipped off the rock.")
        return
    end
    -- The ship may have drifted nearer or further: the chips fly the beam's new length.
    local look = cutter.look(ctx, rock)
    if ctx.get("ship", "engine.beam").look ~= look then ctx.patch("ship", "engine.beam", { look = look }) end
    local hold = ctx.get("ship", "hold")
    local capacity = fit.hold(ctx)
    local due, rest = rules.cut(pilot.cut_work + (ctx.tick - pilot.cut_from) * cutter.rate(ctx))
    local amount = rules.transfer(rules.load(hold), ore.left, capacity, due)
    if amount <= 0 then return end
    local load = rules.load(hold) + amount
    local left = ore.left - amount
    -- Each rock yields its own ware: ore, ice or glassite.
    local ware = ore.ware ~= "" and ore.ware or "ore"
    ctx.patch("ship", "hold", { [ware] = (hold[ware] or 0) + amount })
    ctx.patch(rock, "ore", { left = left })
    -- The rock shrinks as it is mined; one with stages also changes model as it goes,
    -- so its crystals break off, and one left empty wears its `empty` model (rules.stage).
    ctx.patch(rock, "engine.model", rules.worn(ore, left))
    ctx.record("mined", { rock = rock, units = amount })
    ctx.patch("ship", "pilot", { cut_work = rest, cut_from = ctx.tick, gained = pilot.gained + amount, away = true,
        message = "+" .. amount .. " " .. ware .. " · hold " .. load .. " of " .. capacity })
    if left > 0 then
        -- The unit itself: a fistful of stone breaks off the cut and flies to the ship.
        local lx, ly, back = cutter.landing(ctx, rock)
        local chunk = ({ ["core.cutter_near"] = "core.chunk_near", ["core.cutter"] = "core.chunk",
            ["core.cutter_far"] = "core.chunk_far" })[look] or "core.reach_chunk"
        ctx.put(rock, "engine.burst", { recipe = chunk, tick = ctx.tick, target = "core.ship", x = lx, y = ly,
            height = cutter.HEIGHT, angle = back })
    else
        -- The rock's beat: it gives its last and breaks into pebbles.
        ctx.put(rock, "engine.burst", { recipe = "core.rubble", tick = ctx.tick })
        ctx.record("rock_done", { rock = rock })
    end
    local full = load >= capacity
    -- The unit lands in the hold with an amber puff; the last one fills it with a bigger one.
    ctx.put("ship", "engine.burst", { recipe = full and "core.full" or "core.ore_in", tick = ctx.tick, x = HOLD_X, height = 0.5 })
    if full or left <= 0 then
        -- The hold's beat or the rock's: the beam lets go, the camera feels it settle.
        kick(ctx, full and 0.012 or 0.008)
        cutter.off(ctx, nil)
        ctx.patch("ship", "pilot", { cut_work = rest }) -- off's reads predate this cut
        local lines = {}
        local others = {}
        if left <= 0 then
            -- The count is of rocks holding this rock's ware; the rig moves on to any.
            local alike = 0
            for _, id in cutter.rocks(ctx) do
                if id ~= rock then
                    table.insert(others, id)
                    local o = ctx.get(id, "ore")
                    if (o.ware ~= "" and o.ware or "ore") == ware then alike += 1 end
                end
            end
            table.insert(lines, "Rock cut down to pebbles. " .. (alike > 0 and alike .. " more " .. (alike == 1 and "rock holds" or "rocks hold") .. " " .. ware .. "."
                or "That was the field's last " .. ware .. " for now; fresh stone drifts in while you're docked."))
        end
        if full then
            local now = table.clone(hold)
            now[ware] = (now[ware] or 0) + amount
            -- The `hold_full` fact itself is sound.luau's, from the `mined` that filled it.
            table.insert(lines, "Hold full: " .. rules.list(now) .. " aboard.")
        end
        -- Both beats on one unit log both, the hold's last so it is the message showing.
        log.add_all(ctx, lines)
        if not full and pilot.auto and #others > 0 then
            -- The rig (or a script's order) moves on to the nearest rock by itself.
            local at = ctx.get("ship", "engine.position")
            table.sort(others, function(a, b)
                return rules.distance2(at, ctx.get(a, "engine.position")) < rules.distance2(at, ctx.get(b, "engine.position"))
            end)
            approach(ctx, others[1], cutter.has(ctx, cutter.RIG) and rules.RIG_STANDOFF or 0)
        end
    end
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
            if not space(ctx) then return end
            if event.payload.mode == "hold" and pilot.approach and pilot.target ~= "" then
                if cutter.can_cut(ctx, pilot.target, true) then cutter.on(ctx, pilot.target, true, true) end
                return
            end
            if pilot.station == "" then return end
            local port = ctx.get(pilot.station, "port")
            if event.payload.mode ~= "dock" or event.payload.target ~= port.berth then return end
            stop(ctx)
            ctx.remove("ship", "engine.body")
            ctx.put("ship", "engine.burst", { recipe = "core.puff", tick = ctx.tick })
            ctx.patch("ship", "pilot", { room = "", sale = 0, deal = "", last_dock = pilot.station, away = false, gliding = true,
                gained = 0, cleared = false })
            ctx.patch("ship", "engine.model", { scale = 1.3 }) -- moored, seen up close
            ctx.move("ship", pilot.station)
            signs.show(ctx, pilot.station, "", pilot.log_open, nil, true) -- up once the glide in lands
            -- A visit only counts once the ship has really been away: then the
            -- station's market eases back towards its usual stock, and fresh stone
            -- drifts into the rocks cut down to pebbles.
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
                for _, id in ctx.query("ore") do
                    local ore = ctx.get(id, "ore")
                    if ore.left <= 0 then
                        local left = rules.regrow(ore.left, ore.total)
                        ctx.patch(id, "ore", { left = left })
                        ctx.patch(id, "engine.model", rules.worn(ore, left))
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
        -- Every rock can be pointed at and clicked: it publishes an anchor for the HUD.
        for _, id in ctx.query("ore") do
            if not ctx.has(id, "engine.anchor") then ctx.put(id, "engine.anchor", { name = "rock" }) end
        end
        -- Flown well clear of the last berth: the next docking there is a new visit.
        if pilot.last_dock ~= "" and not pilot.away then
            local berth = ctx.get(ctx.get(pilot.last_dock, "port").berth, "engine.position")
            if rules.distance2(at, berth) > rules.AWAY ^ 2 then ctx.patch("ship", "pilot", { away = true }) end
        end
        -- Flying into a berth's ring, slowly, docks: the clamps take the ship in. A ship
        -- that has just left a berth must clear its ring first.
        local ring = cutter.ring(ctx)
        local steer = ctx.get("ship", "engine.steer").mode
        if ring == nil and not pilot.cleared then ctx.patch("ship", "pilot", { cleared = true }) end
        if ring and pilot.cleared and steer ~= "dock" and cutter.speed(ctx) <= rules.RING_SPEED then
            clamp(ctx, ring)
            return
        end
        local near = cutter.situation(ctx, pilot.cleared or ring == nil)
        if near ~= pilot.near then ctx.patch("ship", "pilot", { near = near }) end
        if pilot.mining and pilot.target ~= "" then
            cut(ctx, pilot)
        elseif pilot.approach and pilot.target ~= "" and cutter.can_cut(ctx, pilot.target, true) then
            cutter.on(ctx, pilot.target, true, true)
        end
    end,
}
