-- The ship's cutting beam and the berths' rings, read from the world: which rock the
-- beam would cut now, what the pilot is near, and turning the beam on and off. The
-- plain rules (reach, aim, how much a beam has cut) are in rules.luau.
--
-- By hand, the beam cuts the rock the nose points at, within the cutter's reach, while
-- Space is held (or after a click on that rock, until the ship flies). Two parts take
-- over some of that work once fitted (start.toml): the reach rig in the drill mount
-- aims the beam itself, cuts from far off and flies the ship out to a rock; the docking
-- jets in the engine socket fly the ship into a berth from anywhere.
local rules = require("./rules")
local teach = require("./teach")

local cutter = {
    NOSE = 1.7, -- where the beam leaves the hull: the nose, ahead of the centre
    HEIGHT = 0.25,
    RIG = "rig", -- the reach rig's part
    JETS = "jets", -- the docking jets' part
}

-- Whether the part `id` is fitted.
function cutter.has(ctx, id)
    local p = ctx.get(id, "part")
    return p ~= nil and p.fitted
end

-- The ware a rock gives: ore, ice or glassite.
function cutter.ware(ore)
    return ore and ore.ware ~= "" and ore.ware or "ore"
end

-- Whether the beam is held steady, as the reach rig and a script's orders hold it (`auto`
-- is the pilot's): the stone under it never runs warm or glazes (rules.HEAT).
function cutter.steady(ctx, auto)
    return auto or cutter.has(ctx, cutter.RIG)
end

-- Ore a second the beam cuts with the parts fitted now, on `rock` if given: a rich seam
-- cuts faster (its `rich`), and by hand the stone's heat speeds or slows the cut
-- (rules.heat_rate) unless the beam is `steady`.
function cutter.rate(ctx, rock, steady)
    local f = ctx.get("ship", "fit")
    local ore = rock and ctx.get(rock, "ore")
    local heat = (ore and not steady) and rules.heat_rate(cutter.ware(ore), ore.heat) or 1
    return (f and f.mine or 1) * (ore and ore.rich or 1) * heat
end

-- The cutting work `ticks` of beam on `rock` does, in whole work units (beam ticks times
-- ore a second, rounded), so a rich seam's fractional rate keeps the work an integer.
function cutter.work(ctx, rock, ticks, steady)
    return math.floor(ticks * cutter.rate(ctx, rock, steady) + 0.5)
end

-- How a rock of `ware` looks at `phase`: its own look, warmer as it heats; a glazed one
-- shimmers (an emitter over it), so which rock needs the beam moved off it reads at a
-- glance.
local LOOKS = { ore = "core.rock", ice = "core.ice", glassite = "core.glassite" }
function cutter.show_heat(ctx, rock, ware, phase)
    local base = LOOKS[ware] or LOOKS.ore
    ctx.patch(rock, "engine.model", { look = phase == "cold" and base or base .. "_" .. phase })
    if phase == "cold" then
        if ctx.has(rock, "engine.emitter") then ctx.remove(rock, "engine.emitter") end
    else
        -- Over the rock's top, not inside it: a big seam stands taller than a small rock.
        local model = ctx.get(rock, "engine.model")
        local height = 1.2 * ((model and model.scale) or 1)
        ctx.put(rock, "engine.emitter", { recipe = "core." .. phase .. "_" .. ware, height = height })
    end
end

-- Sets `rock`'s heat, and its look when that changes the phase. Returns the new phase.
function cutter.set_heat(ctx, rock, ore, heat)
    local ware = cutter.ware(ore)
    local was, now = rules.phase(ware, ore.heat), rules.phase(ware, heat)
    ctx.patch(rock, "ore", { heat = heat })
    if was ~= now then cutter.show_heat(ctx, rock, ware, now) end
    return now
end

-- Banks the beam time since the pilot's `cut_from` on its target: the work it did, at the
-- rate the stone's heat gave it then, and, by hand, the heat it put into the stone.
-- Returns the work not yet come loose, and the stone's phase now.
function cutter.bank(ctx, pilot)
    local rock = pilot.target
    local ticks = math.max(0, ctx.tick - pilot.cut_from)
    local steady = cutter.steady(ctx, pilot.auto)
    local work = pilot.cut_work + cutter.work(ctx, rock, ticks, steady)
    local ore = ctx.get(rock, "ore")
    if not ore then return work, "cold" end
    if steady or ticks == 0 then return work, rules.phase(cutter.ware(ore), ore.heat) end
    return work, cutter.set_heat(ctx, rock, ore, rules.warmed(cutter.ware(ore), ore.heat, ticks / rules.TICKS))
end

-- The beam's reach with the parts fitted now.
function cutter.reach(ctx)
    return cutter.has(ctx, cutter.RIG) and rules.RIG_REACH or rules.REACH
end

-- A rock's centre and drawn radius now.
function cutter.rock(ctx, id)
    local model = ctx.get(id, "engine.model")
    return ctx.get(id, "engine.position"), rules.rock_radius(model and model.scale or 1, model and model.morph or 0)
end

-- The rocks with ore in the ship's place.
function cutter.rocks(ctx)
    local here = ctx.get("ship", "engine.location").place
    local found = {}
    for _, id in ctx.query("ore") do
        local where = ctx.get(id, "engine.location")
        if ctx.get(id, "ore").left > 0 and where and where.place == here then table.insert(found, id) end
    end
    return found
end

-- Whether the ship can cut `rock` now: in reach, and with the nose on it unless the beam
-- aims itself (the rig, or `auto`: a script's order).
function cutter.can_cut(ctx, rock, auto)
    local at = ctx.get("ship", "engine.position")
    local centre, radius = cutter.rock(ctx, rock)
    return rules.cuttable(at, centre, radius, cutter.reach(ctx), auto or cutter.has(ctx, cutter.RIG))
end

-- The rock the beam would cut now (the one the nose is most squarely on, then the
-- nearest), or nil; and, when none, the nearest rock with ore and its gap. For an
-- `order` (the rig's or a script's pick) a seam's spall is passed over: it is where a
-- hand rests the beam, and a click on it still cuts it.
function cutter.sighted(ctx, order)
    local at = ctx.get("ship", "engine.position")
    local rig = cutter.has(ctx, cutter.RIG)
    local reach = cutter.reach(ctx)
    local best, best_off, best_gap = nil, math.huge, math.huge
    local near, near_gap = nil, math.huge
    for _, id in cutter.rocks(ctx) do
        if order and ctx.get(id, "ore").spall then continue end
        local centre, radius = cutter.rock(ctx, id)
        local gap = rules.gap(at, centre, radius)
        if gap < near_gap then near, near_gap = id, gap end
        if rules.cuttable(at, centre, radius, reach, rig) then
            local off = rig and 0 or rules.off_aim(at, centre, radius)
            if off < best_off or (off == best_off and gap < best_gap) then best, best_off, best_gap = id, off, gap end
        end
    end
    return best, near, near_gap
end

-- The station whose berth ring the ship is in, whatever its speed, or nil.
function cutter.ring(ctx)
    local at = ctx.get("ship", "engine.position")
    for _, id in ctx.query("port") do
        local berth = ctx.get(ctx.get(id, "port").berth, "engine.position")
        if berth and rules.in_ring(at, berth, 0) then return id end
    end
    return nil
end

-- The ship's speed, world units a second.
function cutter.speed(ctx)
    local m = ctx.get("ship", "engine.motion")
    if not m then return 0 end
    return math.sqrt(m.vx * m.vx + m.vy * m.vy)
end

-- What the pilot is near, for the hint strip: "ring:<station>" in a berth's ring,
-- "aim" with a rock in the beam's sights, "reach" with one in reach but the nose off
-- it, or "".
-- A ring only counts once the ship has `cleared` the berth it last left.
function cutter.situation(ctx, cleared)
    local ring = cutter.ring(ctx)
    if ring and cleared then return "ring:" .. ring end
    local sighted, near, gap = cutter.sighted(ctx)
    if sighted then return "aim" end
    if near and gap <= cutter.reach(ctx) then return "reach" end
    return ""
end

-- The beam's look for `rock`: the rig's, or the hand cutter's for how far the cut is from
-- the nose, so its chips fly just the length of the beam and fade as they reach the ship.
-- On glazed stone (`phase`) the hand beam skates: no chips, sparks thrown wide.
function cutter.look(ctx, rock, phase)
    if cutter.has(ctx, cutter.RIG) then return "core.reach" end
    if phase == "glazed" then return "core.cutter_glazed" end
    local at = ctx.get("ship", "engine.position")
    local centre, radius = cutter.rock(ctx, rock)
    local gap = rules.gap(at, centre, radius) - cutter.NOSE
    if gap < 1.5 then return "core.cutter_near" end
    if gap < 3.5 then return "core.cutter" end
    return "core.cutter_far"
end

-- Turns the beam on `rock`. `latched` keeps it on without Space held (a click, the
-- rig or a script's order); `auto` aims it without the nose (the rig or a script's
-- order). Cut progress carries on if it is the same rock.
function cutter.on(ctx, rock, latched, auto)
    local pilot = ctx.get("ship", "pilot")
    -- Moved from one rock to another: the one left keeps the heat it got. Off one that has
    -- run hot (it may have cooled a little while the nose came round), that is the move
    -- the hint teaches.
    if pilot.target ~= rock and pilot.target ~= "" then
        local left
        if pilot.mining and pilot.cut_from > 0 then
            left = select(2, cutter.bank(ctx, pilot))
        else
            local o = ctx.get(pilot.target, "ore")
            left = o and rules.phase(cutter.ware(o), o.heat)
        end
        if left and left ~= "cold" and not cutter.steady(ctx, auto) then teach.used(ctx, "swap") end
    end
    local ore = ctx.get(rock, "ore")
    local phase = (ore and not cutter.steady(ctx, auto)) and rules.phase(cutter.ware(ore), ore.heat) or "cold"
    ctx.put("ship", "engine.beam", { target = rock, look = cutter.look(ctx, rock, phase), x = cutter.NOSE, height = cutter.HEIGHT })
    local fields = { mining = true, target = rock, latched = latched, auto = auto or false, cut_from = ctx.tick,
        approach = false, away = true }
    if pilot.target ~= rock then
        fields.cut_work = 0
    end
    -- A beam put on by hand is no order: the rig's "rock you picked" is not this one.
    if not auto then fields.chosen = false end
    if not pilot.mining then
        fields.gained = 0
        -- What stops it, so the line under a fresh beam never still says why it couldn't.
        if cutter.has(ctx, cutter.RIG) then
            fields.message = "The rig cuts on its own. Fly to stop it."
        elseif auto then
            fields.message = "Cutting. Fly to stop."
        elseif latched then
            fields.message = "Cutting. Fly, or click the rock again, to stop."
        else
            fields.message = "Cutting. Let go of Space to stop."
        end
    end
    ctx.patch("ship", "pilot", fields)
    if not pilot.mining then ctx.record("cut_start", { rock = rock }) end
end

-- Sefa's word on what to watch has been heard out: the first glaze, or the first beam
-- going off. The per-unit count takes the message line back.
function cutter.told(ctx)
    if ctx.get("ship", "taught").heat == 1 then ctx.patch("ship", "taught", { heat = 2 }) end
end

-- Turns the beam off, keeping how far it got into the rock. `text` says why, if the
-- player should hear it.
function cutter.off(ctx, text)
    local pilot = ctx.get("ship", "pilot")
    ctx.put("ship", "engine.beam", {})
    local fields = { mining = false, latched = false, auto = false, cut_from = 0, approach = false }
    if pilot.cut_from > 0 then fields.cut_work = (cutter.bank(ctx, pilot)) end
    if text then fields.message = text end
    ctx.patch("ship", "pilot", fields)
    if pilot.mining then ctx.record("cut_stop", { rock = pilot.target }) end
    cutter.told(ctx)
end

-- Where the beam lands on `rock`, in the ship's own frame (x ahead, y to the left), and
-- the heading in degrees from there back to the nose: so a burst of stone can leave the
-- cut and fly to the ship however both are turned.
function cutter.landing(ctx, rock)
    local at = ctx.get("ship", "engine.position")
    local centre, radius = cutter.rock(ctx, rock)
    local dx, dy = centre.x - at.x, centre.y - at.y
    local c, s = math.cos(at.angle or 0), math.sin(at.angle or 0)
    local lx, ly = dx * c + dy * s, -dx * s + dy * c
    local d = math.sqrt(lx * lx + ly * ly)
    if d > radius then
        lx, ly = lx * (d - radius) / d, ly * (d - radius) / d
    end
    return lx, ly, math.deg(math.atan2(-ly, cutter.NOSE - lx))
end

-- The names of the stations that buy something in `hold`.
function cutter.buyers(ctx, hold)
    local names = {}
    for _, station in ctx.query("port") do
        local buys = false
        for _, id in ctx.query("market") do
            local line = ctx.get(id, "market")
            local facility = ctx.get(line.room, "facility")
            if facility and facility.station == station and line.role == "buys" and (hold[line.ware] or 0) > 0 then buys = true end
        end
        if buys then table.insert(names, ctx.get(station, "port").name) end
    end
    return names
end

return cutter
