local rules = require("./rules")
local log = require("./log")
local teach = require("./teach")
local trade = require("./trade")
local signs = require("./signs")
local fit = require("./fit")
local undock = require("./launch")
local cutter = require("./cutter")
local project = require("./project")
local salvage = require("./salvage")
local notice = require("./notice")

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

-- A message, and its parts (notice.luau): what the pilot sees, not logged.
local function message(ctx, text, parts)
    ctx.patch("ship", "pilot", notice.with({}, text, parts))
end

-- A rock in reach is off the nose: what the pilot hears, and the keys that turn onto it.
local OFF_NOSE = "The rock is in reach, but off your nose."
local TURN = { press = "A or D · turn onto the rock" }

local function stop(ctx)
    -- A beam cut short here lets go like any other, so its hum stops with it.
    local pilot = ctx.get("ship", "pilot")
    if pilot.mining then ctx.record("cut_stop", { rock = pilot.target }) end
    ctx.put("ship", "engine.helm", {})
    ctx.put("ship", "engine.steer", {})
    ctx.put("ship", "held", {})
    ctx.put("ship", "engine.beam", {})
    ctx.patch("ship", "pilot", { mining = false, target = "", latched = false, auto = false, cut_from = 0, cut_work = 0,
        approach = false, facing = "", press = "" })
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

-- What Sefa at Cinder Kiln says on the radio the first time the hand beam comes on,
-- before the stone has warmed: what to watch while cutting.
local WATCH = "Sefa on Cinder Kiln's radio: \u{201C}Watch the stone under your beam. Warm, it comes away fast. Once it glazes, move the beam to another rock and let it cool.\u{201D}"

-- The hand beam comes on: `cutter.on`, and the first time Sefa says what to watch.
local function hand_on(ctx, rock, latched)
    cutter.on(ctx, rock, latched)
    if ctx.get("ship", "taught").heat == 0 then
        ctx.patch("ship", "taught", { heat = 1 })
        log.add(ctx, WATCH)
    end
end

-- Starts the beam on the rock in the nose's sights, or says why there is none.
local function cut_ahead(ctx, latched)
    local sighted, near, gap = cutter.sighted(ctx)
    if sighted then
        hand_on(ctx, sighted, latched)
        return true
    end
    if not near then
        local hold = ctx.get("ship", "hold")
        message(ctx, rules.worked_out(rules.load(hold), project.carried(ctx, hold), cutter.buyers(ctx, hold)))
    elseif gap <= cutter.reach(ctx) then
        message(ctx, OFF_NOSE, TURN)
    else
        message(ctx, "No rock in reach. The cutter reaches " .. cutter.reach(ctx) .. " units: fly closer.")
    end
    return false
end

-- The rig (or a script's order) flies out to `rock` and holds off it while it cuts. The
-- rig cuts at once from where it is if it can, and holds off the rock meanwhile, so a
-- ship still coasting from the last rock can't drift out of its reach; a script's order
-- parks the ship on the rock's surface (`standoff` 0) and cuts there.
local function approach(ctx, rock, standoff)
    ctx.put("ship", "engine.helm", {})
    if cutter.can_cut(ctx, rock, true) then
        ctx.put("ship", "engine.steer", { mode = "hold", target = rock, range = standoff })
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
        -- Flying by hand takes over from a click's turn onto a rock.
        if pilot.facing ~= "" then ctx.patch("ship", "pilot", { facing = "" }) end
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
    -- Space, or another click, takes over from a click's turn onto a rock: the nose stops
    -- where it is (unless A or D is held).
    if pilot.facing ~= "" then
        ctx.patch("ship", "pilot", { facing = "" })
        ctx.patch("ship", "engine.helm", { turn = (held.left and 1 or 0) - (held.right and 1 or 0) })
    end
    if key then
        -- Holding Space brakes, so the ship sits steady on the rock it cuts.
        ctx.patch("ship", "held", { mine = true })
        ctx.patch("ship", "engine.helm", { brake = true })
        -- In a berth's ring, Space docks: once the ship has cleared the one it left.
        local ring = cutter.ring(ctx)
        if ring and pilot.cleared then clamp(ctx, ring); return end
    end
    -- Space beside a barge adrift passes her a line, by hand: the rig cuts rock, it can't
    -- pass a line. With a barge in tow the beam stays off.
    local barge, gap = salvage.near(ctx)
    if key and barge and gap <= salvage.REACH then
        ctx.record("hitch", { barge = barge }) -- tow.luau passes it, next tick
        return
    end
    if salvage.towing(ctx) then message(ctx, "Not with a barge in tow.", { press = "0 · cast off the line" }); return end
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
            local sighted, near = cutter.sighted(ctx, true)
            target = sighted or near
        end
        if not target then cut_ahead(ctx, true); return end
        if pilot.mining and pilot.target == target and key then cutter.off(ctx, "Beam off."); return end
        approach(ctx, target, rig and rules.RIG_STANDOFF or 0)
        -- A rock picked by name is the player's choice of ware: the rig stops with it.
        ctx.patch("ship", "pilot", { chosen = rock ~= nil })
        return
    end
    if rock ~= nil then
        -- A click on a rock: the beam cuts it while the ship holds steady, as Space does.
        if cutter.can_cut(ctx, rock) then
            hand_on(ctx, rock, true)
        else
            local at = ctx.get("ship", "engine.position")
            local centre, radius = cutter.rock(ctx, rock)
            if rules.gap(at, centre, radius) > cutter.reach(ctx) then
                message(ctx, "Too far to cut. The cutter reaches " .. cutter.reach(ctx) .. " units: fly closer.")
            else
                -- In reach but off the nose: she turns onto it where she sits (the helm's
                -- own turn, no flying), and the beam latches on once it is aimed (`facing`).
                if pilot.mining then cutter.off(ctx, nil) end
                ctx.patch("ship", "pilot", { facing = rock, message = "Turning onto the rock." })
                ctx.patch("ship", "engine.helm", { turn = cutter.turn_toward(ctx, rock) })
                ctx.record("facing", { rock = rock, from = ctx.tick })
            end
        end
        return
    end
    cut_ahead(ctx, false)
end

-- A click's turn onto a rock in reach, one tick at a time (a recorded fact runs its
-- handler on the next tick): the nose comes round at the helm's turn rate and settles on
-- the rock's centre (rules.SETTLE, or the tick it swings past it), not at the edge of the
-- aim, and the beam latches on there. Anything that takes the helm (a key, an order, docking,
-- another click) clears `facing` and ends it.
local function facing(event, ctx)
    local rock = event.payload.rock
    local pilot = ctx.get("ship", "pilot")
    if pilot.facing ~= rock or rock == "" then return end
    local function done(text, parts)
        local fields = { facing = "" }
        if text then notice.with(fields, text, parts) end
        ctx.patch("ship", "pilot", fields)
        ctx.patch("ship", "engine.helm", { turn = 0 })
    end
    local ore = ctx.get(rock, "ore")
    if not space(ctx) or pilot.mining or ctx.get("ship", "engine.steer").mode ~= "" or not ore or ore.left <= 0 then
        done(nil)
        return
    end
    local at = ctx.get("ship", "engine.position")
    local centre, radius = cutter.rock(ctx, rock)
    if rules.gap(at, centre, radius) > cutter.reach(ctx) then
        done("Too far to cut. The cutter reaches " .. cutter.reach(ctx) .. " units: fly closer.")
        return
    end
    local turn = cutter.turn_toward(ctx, rock)
    local was = ctx.get("ship", "engine.helm").turn
    if cutter.can_cut(ctx, rock) and (rules.off_centre(at, centre) <= rules.SETTLE or (was ~= 0 and turn ~= was)) then
        done(nil)
        hand_on(ctx, rock, true)
        return
    end
    -- A whole turn takes two seconds on the bare hull; past three, something holds the nose.
    if ctx.tick - (event.payload.from or ctx.tick) > 3 * rules.TICKS then
        done(OFF_NOSE, TURN)
        return
    end
    ctx.patch("ship", "engine.helm", { turn = turn })
    ctx.record("facing", { rock = rock, from = event.payload.from })
end

local function dock(event, ctx)
    if not pressed(event) or not space(ctx) then return end
    teach.used(ctx, "dock")
    local station = event.payload.station
    if station ~= nil and not ctx.get(station, "port") then station = nil end
    local ring = cutter.ring(ctx)
    if ring and (station == nil or station == ring) then clamp(ctx, ring); return end
    if salvage.towing(ctx) and not scripted(event) then
        message(ctx, "The jets can't berth a tow. Fly her slowly into a berth's ring by hand.")
        return
    end
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
    -- How docking works is a lesson, with no message of its own.
    message(ctx, "", { lesson = "Fly into " .. name .. "'s berth ring, the glowing ring off its dock, and the clamps take you in." })
end

-- The moment the stone under a hand beam glazes: what happened, how glazed stone comes
-- back, and the keys that move the beam (the hint strip's own, hud.luau), each apart.
local GLAZED = "The stone has glazed: the beam skates on it."
local GLAZE = { lesson = "Glazed stone cools once the beam is off it.",
    press = "Click another rock, or A or D · move the beam" }

local told = cutter.told

-- One second of the beam on its rock: what it has cut lands in the hold.
local function cut(ctx, pilot)
    local rock = pilot.target
    -- A save from before beam time was kept has no start: its order's beam starts now.
    if pilot.cut_from <= 0 then
        cutter.on(ctx, rock, true, true)
        return
    end
    local ore = ctx.get(rock, "ore")
    if not ore or ore.left <= 0 or not cutter.holds(ctx, rock, pilot.auto) then
        local rig = cutter.has(ctx, cutter.RIG)
        cutter.off(ctx, rig and "The rock is out of the rig's reach." or "The beam slipped off the rock.")
        return
    end
    -- The second's beam time is banked at the rate the stone's heat gave it, and by hand
    -- it warms the stone (rules.HEAT).
    local ware = cutter.ware(ore)
    local was = rules.phase(ware, ore.heat)
    local work, phase = cutter.bank(ctx, pilot)
    local steady = cutter.steady(ctx, pilot.auto)
    -- The ship may have drifted nearer or further, and the stone may have glazed: the beam
    -- shows it, and its chips fly the beam's new length.
    local look = cutter.look(ctx, rock, phase)
    if ctx.get("ship", "engine.beam").look ~= look then ctx.patch("ship", "engine.beam", { look = look }) end
    local hold = ctx.get("ship", "hold")
    local capacity = fit.hold(ctx)
    local due, rest = rules.cut(work)
    local amount = rules.transfer(rules.load(hold), ore.left, capacity, due)
    local turned = not steady and phase ~= was
    if amount <= 0 then
        local fields = { cut_work = work, cut_from = ctx.tick }
        if turned and phase == "glazed" then notice.with(fields, GLAZED, GLAZE) end
        ctx.patch("ship", "pilot", fields)
        if turned and phase == "glazed" then told(ctx) end
        return
    end
    local load = rules.load(hold) + amount
    local left = ore.left - amount
    ctx.patch("ship", "hold", { [ware] = (hold[ware] or 0) + amount })
    ctx.patch(rock, "ore", { left = left })
    -- The rock shrinks as it is mined; one with stages also changes model as it goes,
    -- so its crystals break off, and one left empty wears its `empty` model (rules.stage).
    ctx.patch(rock, "engine.model", rules.worn(ore, left))
    ctx.record("mined", { rock = rock, units = amount })
    local full = load >= capacity
    -- A full hold is the work itself: the fitter counts them (fit.verdict, part `holds`).
    -- The count says what the stone under a hand beam is doing; just after the first beam
    -- comes on, the line about it stays up until the stone first glazes (told).
    local line = "+" .. amount .. " " .. ware .. " · hold " .. load .. " of " .. capacity .. ((ore.rich or 1) > 1 and " · rich seam" or "")
        .. (steady and "" or " · " .. rules.FEEL[phase])
    local fields = { cut_work = rest, cut_from = ctx.tick, gained = pilot.gained + amount, away = true,
        holds = pilot.holds + (full and 1 or 0), message = line }
    if turned and phase == "glazed" then
        notice.with(fields, GLAZED, GLAZE)
        told(ctx)
    elseif not steady and ctx.get("ship", "taught").heat == 1 then
        fields.message = nil
    end
    ctx.patch("ship", "pilot", fields)
    if left > 0 then
        -- The unit itself: a fistful of stone breaks off the cut and flies to the ship.
        local lx, ly, back = cutter.landing(ctx, rock)
        local chunk = ({ ["core.cutter_near"] = "core.chunk_near", ["core.cutter"] = "core.chunk",
            ["core.cutter_far"] = "core.chunk_far", ["core.cutter_glazed"] = "core.chunk" })[look] or "core.reach_chunk"
        ctx.put(rock, "engine.burst", { recipe = chunk, tick = ctx.tick, target = "core.ship", x = lx, y = ly,
            height = cutter.HEIGHT, angle = back })
    else
        -- The rock's beat: it gives its last and breaks into pebbles.
        ctx.put(rock, "engine.burst", { recipe = "core.rubble", tick = ctx.tick })
        ctx.record("rock_done", { rock = rock })
    end
    -- The unit lands in the hold with an amber puff; the last one fills it with a bigger one.
    ctx.put("ship", "engine.burst", { recipe = full and "core.full" or "core.ore_in", tick = ctx.tick, x = HOLD_X, height = 0.5 })
    if full or left <= 0 then
        -- The hold's beat or the rock's: the beam lets go, the camera feels it settle.
        kick(ctx, full and 0.012 or 0.008)
        cutter.off(ctx, nil)
        ctx.patch("ship", "pilot", { cut_work = rest }) -- off's reads predate this cut
        local lines = {}
        local parts
        local others = {}
        if left <= 0 then
            -- The count is of rocks holding this rock's ware; the rig moves on to any.
            local alike = 0
            for _, id in cutter.rocks(ctx) do
                local o = ctx.get(id, "ore")
                if id ~= rock then
                    -- A seam's spall is somewhere for a hand to rest the beam: an order
                    -- moves on to a real rock, never to it by itself.
                    if not o.spall then table.insert(others, id) end
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
            table.insert(lines, "Hold full: " .. project.carried(ctx, now) .. " aboard.")
        end
        if left <= 0 and not full and pilot.chosen then
            table.insert(lines, "The rig stops with the rock you picked.")
            parts = { press = "Space · the nearest rock, or click another" }
        end
        -- Both beats on one unit log both, the hold's last so it is the message showing.
        log.add_all(ctx, lines, parts)
        if not full and pilot.auto and not pilot.chosen and #others > 0 then
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
        mine = mine, dock = dock, facing = facing,
        cancel = function(event, ctx)
            if pressed(event) and space(ctx) then
                teach.used(ctx, "cancel")
                stop(ctx)
                local off = salvage.cast_off(ctx)
                if off then log.add(ctx, off) end
                message(ctx, off or "Order cancelled. Coasting to a stop.")
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
            -- Docking glides in from the ship's view (going back out of a room turns the
            -- glide off: scripts/station.luau).
            ctx.patch(pilot.station, "engine.room", { glide = 1.0 })
            ctx.move("ship", pilot.station)
            signs.show(ctx, pilot.station, "", pilot.log_open, nil, true) -- up once the glide in lands
            -- A visit only counts once the ship has really been away: then every
            -- station's market eases back towards its usual stock (the trip took time
            -- everywhere, so a station left alone recovers while you sell at the other),
            -- and fresh stone drifts into the rocks cut down to pebbles.
            local counted = rules.counts(pilot.last_dock, pilot.away, pilot.station)
            if counted then
                ctx.patch(pilot.station, "port", { visits = port.visits + 1 })
                for _, id in ctx.query("market") do
                    local line = ctx.get(id, "market")
                    local stock = rules.settle(line)
                    ctx.patch(id, "market", { stock = stock })
                    if line.ware == "ore" then trade.piles(ctx, trade.station(ctx, line), stock, line.target) end
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
            -- A project's steps that wait on dockings move on, and a happening there counts it.
            local note = project.arrived(ctx, pilot.station, counted, port.visits + (counted and 1 or 0))
            -- A barge in tow is brought in, and work on one finishes on its station's visit.
            for _, more in { salvage.docked(ctx, pilot.station), salvage.arrived(ctx, pilot.station, counted, port.visits + (counted and 1 or 0)) } do
                note = note and note .. " " .. more or more
            end
            log.add(ctx, "Docked at " .. port.name .. "." .. (note and " " .. note or ""))
            ctx.record("docked", { station = pilot.station, counted = counted })
        end,
    },
    cadence = function(ctx)
        local pilot = ctx.get("ship", "pilot")
        -- Every rock off the beam cools a second's worth, docked or not.
        for _, id in ctx.query("ore") do
            local ore = ctx.get(id, "ore")
            if ore.heat > 0 and not (pilot.mining and pilot.target == id) then
                cutter.set_heat(ctx, id, ore, rules.cooled(cutter.ware(ore), ore.heat, 1))
            end
        end
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
        if near ~= "aim" and not string.match(near, "^ring:") then
            local barge, gap = salvage.near(ctx)
            if barge and gap <= salvage.REACH then near = "barge" end
        end
        if near ~= pilot.near then ctx.patch("ship", "pilot", { near = near }) end
        if pilot.mining and pilot.target ~= "" then
            cut(ctx, pilot)
        elseif pilot.approach and pilot.target ~= "" and cutter.can_cut(ctx, pilot.target, true) then
            cutter.on(ctx, pilot.target, true, true)
        end
    end,
}
