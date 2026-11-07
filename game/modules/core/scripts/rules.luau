-- Plain rules with no world access, so tests can call them directly.
local rules = {
    -- The bare hull's hold; fitted parts can add to it (fit.luau, fit.hold).
    CAPACITY = 24,
    -- The hand cutter reaches this far from the ship's centre to the rock's surface,
    -- world units: a ship parked against a rock is about 1.7 off it, so this is a
    -- couple of ship lengths of stand-off. The reach rig cuts from much further.
    REACH = 7,
    RIG_REACH = 16,
    -- Where the reach rig holds the ship while it cuts: this far off the rock's outline.
    RIG_STANDOFF = 9,
    -- A rock's surface from its centre, world units, at scale 1 and uncut (the rock
    -- model's footprint); fully cut it shrinks to about 80% of that.
    ROCK_RADIUS = 2.7,
    -- The nose must point within this many degrees of the rock's edge to cut it.
    AIM_SLACK = 12,
    -- A click's turn onto a rock settles the nose within this many degrees of its centre.
    SETTLE = 4,
    -- A berth's ring: a ship whose centre comes within this of the berth's, slower than
    -- RING_SPEED, is taken in by the station's clamps.
    RING = 3.5,
    RING_SPEED = 8,
    -- A rock cut down to pebbles comes back with this share of its stone on a counted
    -- visit to a station: fresh stone drifting into the field while the ship is away.
    REGROW = 0.5,
    TICKS = 30,
    -- A docking counts as a visit only after the ship has flown this far from the
    -- berth it last left, mined, or docked somewhere else.
    AWAY = 20,
    -- The goods a hold carries, in the order screens list them. Panes are only ever
    -- carried for someone (scripts/project.luau): no market trades them.
    WARES = { "ore", "ice", "glassite", "plate", "rations", "panes" },
}

-- How the stone under the hand cutter's beam takes its heat (scripts/cutter.luau). Each
-- second of beam on a rock warms it by one; each second off the beam it cools by `cool`.
-- Cold stone (below `warm`) cuts at `cold` times the ship's rate; warm stone, up to
-- `glaze`, comes away fast at `hot` times; past that it glazes over and the beam skates,
-- at `glazed` times, until it has cooled back. Nothing is lost: a glazed rock still cuts,
-- and cools by itself. Ore runs warm a few seconds and holds it, so a small rock is
-- nearly cut through before it glazes and a seam wants the beam moved to its spall;
-- ice warms at once, slumps soon and cools fast, so the close-packed floes want the beam
-- moved often; glassite crazes after a few units, so the crystals want working in turn,
-- coming back for what is left. The reach rig and a script's orders hold the beam
-- steady: always the ship's own rate, never warm, never glazed.
rules.HEAT = {
    ore = { warm = 1.5, glaze = 4.5, cool = 1.5, cold = 0.6, hot = 1.7, glazed = 0.45 },
    ice = { warm = 0.5, glaze = 3, cool = 3, cold = 0.6, hot = 1.7, glazed = 0.45 },
    glassite = { warm = 1, glaze = 3, cool = 2, cold = 0.5, hot = 1.5, glazed = 0.4 },
}

-- The heat rules for a rock of `ware` (ore's for any other).
function rules.heat_of(ware)
    return rules.HEAT[ware] or rules.HEAT.ore
end

-- "cold", "warm" or "glazed": the stone of `ware` at `heat`.
function rules.phase(ware, heat)
    local h = rules.heat_of(ware)
    if (heat or 0) >= h.glaze then return "glazed" end
    if (heat or 0) >= h.warm then return "warm" end
    return "cold"
end

-- How fast the hand cutter cuts stone of `ware` at `heat`, times the ship's rate.
function rules.heat_rate(ware, heat)
    local h = rules.heat_of(ware)
    local phase = rules.phase(ware, heat)
    if phase == "glazed" then return h.glazed end
    if phase == "warm" then return h.hot end
    return h.cold
end

-- A rock's heat after `seconds` more of beam: it stops rising a little past glazing, so a
-- rock left glazed cools back within a few seconds however long the beam sat on it.
function rules.warmed(ware, heat, seconds)
    return math.min((heat or 0) + seconds, rules.heat_of(ware).glaze + 2)
end

-- A rock's heat after `seconds` off the beam.
function rules.cooled(ware, heat, seconds)
    return math.max(0, (heat or 0) - rules.heat_of(ware).cool * seconds)
end

-- What the pilot reads about the stone under the beam, after the hold's count.
rules.FEEL = {
    cold = "cold stone, it warms as you cut",
    warm = "warm: cutting fast",
    glazed = "glazed: put the beam on another rock",
}

function rules.distance2(a, b)
    return (a.x - b.x) ^ 2 + (a.y - b.y) ^ 2
end

-- Units in a hold ({ ore = n, ice = n, ... }).
function rules.load(hold)
    local n = 0
    for _, ware in rules.WARES do
        n += hold[ware] or 0
    end
    return n
end

-- "4 ore, 2 plate", or "nothing" for an empty hold.
function rules.list(hold)
    local parts = {}
    for _, ware in rules.WARES do
        if (hold[ware] or 0) > 0 then table.insert(parts, hold[ware] .. " " .. ware) end
    end
    return #parts > 0 and table.concat(parts, ", ") or "nothing"
end

-- Ore mined this second: `rate` units (one by default), as far as the hold of
-- `capacity` (the bare hull's by default) has room and the rock has ore.
function rules.transfer(load, left, capacity, rate)
    return math.max(0, math.min(rate or 1, (capacity or rules.CAPACITY) - load, left))
end

-- The model a rock wears with `mined` of its `total` units gone: `stages` lists its
-- models, whole first, separated by spaces, each taking an equal share of the rock's
-- life, and the last one stays to the end. Once all are mined, `empty` (a model, if given)
-- takes over. Nil when neither applies: the rock keeps its model.
function rules.stage(stages, mined, total, empty)
    if empty and empty ~= "" and total > 0 and mined >= total then return empty end
    local list = string.split(stages, " ")
    for i = #list, 1, -1 do
        if list[i] == "" then table.remove(list, i) end
    end
    if #list == 0 or total <= 0 then return nil end
    return list[math.clamp(math.floor(mined / total * #list) + 1, 1, #list)]
end

-- How a rock with `left` of its units still in it looks: its morph, and its stage's model
-- when it has stages or an `empty` model (rules.stage), so one regrown from empty gets
-- its stage back.
function rules.worn(ore, left)
    local mined = ore.total - left
    local look = { morph = mined / ore.total }
    local stage = rules.stage(ore.stages or "", mined, ore.total, ore.empty)
    if stage then look.model = stage end
    return look
end

-- The price of the next unit on a market line: the base price, raised while the
-- station's stock is short of its target and lowered while it is over, by half the
-- shortfall, within 70% to 130% of base. Rounded to whole credits, halves up.
function rules.price(line)
    local stock = math.clamp(line.stock, 0.4 * line.target, 1.6 * line.target)
    local exact = line.base * (1 + 0.5 * (1 - stock / line.target))
    return math.max(1, math.floor(exact + 0.5))
end

-- The unit prices of trading `units` on a line, in order, and the stock after: each
-- unit sold to a buying line adds one to its stock, each unit bought from a selling
-- line takes one. A trade is priced unit by unit, so splitting it never pays more.
function rules.walk(line, units)
    local prices, stock = {}, line.stock
    local step = line.role == "buys" and 1 or -1
    for _ = 1, units do
        table.insert(prices, rules.price({ base = line.base, target = line.target, stock = stock }))
        stock += step
    end
    return prices, stock
end

function rules.total(prices)
    local sum = 0
    for _, p in prices do
        sum += p
    end
    return sum
end

-- What selling `units` to a buying line pays.
function rules.sale(line, units)
    return rules.total((rules.walk(line, units)))
end

-- How many units the player can buy from a selling line: limited by its stock, the
-- room in the hold and the credits on hand.
function rules.affordable(line, room, credits)
    local units = math.min(room, line.stock)
    local prices = rules.walk(line, units)
    local n, spent = 0, 0
    for _, p in prices do
        if spent + p > credits then break end
        spent += p
        n += 1
    end
    return n
end

-- A rock's surface from its centre: its model scale times the uncut radius, shrinking
-- as it is cut (`morph` 0 uncut to 1 worked out).
function rules.rock_radius(scale, morph)
    return rules.ROCK_RADIUS * (scale or 1) * (1 - 0.2 * (morph or 0))
end

-- How far the ship at `at` is from the surface of a rock at `rock` with `radius`.
function rules.gap(at, rock, radius)
    return math.sqrt(rules.distance2(at, rock)) - radius
end

-- How many degrees the nose of a ship at `at` (its `angle` in radians) is off the
-- nearest edge of a rock at `rock` with `radius`: 0 when the nose points anywhere at
-- the rock. No trigonometry beyond what the sandbox makes portable.
function rules.off_aim(at, rock, radius)
    local dx, dy = rock.x - at.x, rock.y - at.y
    local d = math.sqrt(dx * dx + dy * dy)
    if d <= radius then return 0 end
    local half = math.deg(math.asin(math.min(1, radius / d)))
    return math.max(0, rules.off_centre(at, rock) - half)
end

-- How many degrees the nose of a ship at `at` is off the centre of a rock at `rock`.
function rules.off_centre(at, rock)
    local dx, dy = rock.x - at.x, rock.y - at.y
    local d = math.sqrt(dx * dx + dy * dy)
    if d <= 0 then return 0 end
    local cos = (dx * math.cos(at.angle or 0) + dy * math.sin(at.angle or 0)) / d
    return math.deg(math.acos(math.clamp(cos, -1, 1)))
end

-- Whether a ship at `at` can cut a rock at `rock`: within `reach` of its surface and,
-- unless the beam aims itself (`any_aim`), with the nose on it.
function rules.cuttable(at, rock, radius, reach, any_aim)
    if rules.gap(at, rock, radius) > reach then return false end
    return any_aim or rules.off_aim(at, rock, radius) <= rules.AIM_SLACK
end

-- What `work` (beam ticks times ore a second) breaks loose: whole units, and the work
-- left over, which may be a little below zero. A unit comes loose half-way through its
-- second, so the first lands within a second of starting and none is counted twice.
function rules.cut(work)
    local units = math.max(0, math.floor(work / rules.TICKS + 0.5))
    return units, work - units * rules.TICKS
end

-- Whether a ship at `at` moving at `speed` is in a berth's ring and slow enough to be
-- taken in.
function rules.in_ring(at, berth, speed)
    return rules.distance2(at, berth) <= rules.RING ^ 2 and (speed or 0) <= rules.RING_SPEED
end

-- What a rock holds after a counted visit: one cut down to pebbles comes back with half
-- its stone; one still holding ore is left as it is.
function rules.regrow(left, total)
    if left > 0 then return left end
    return math.floor(total * rules.REGROW)
end

-- What to tell a pilot who asks to mine when no rock has ore: what they carry, and
-- the real next step. `buyers` names the stations that buy what is aboard ("" if none
-- does); `cargo` is rules.list of the hold.
function rules.worked_out(load, cargo, buyers)
    local field = "Every rock is cut down to pebbles; fresh stone drifts in while you're docked."
    if load == 0 then return field .. " Your hold is empty: dock at either station, then come back." end
    if #buyers == 0 then return field .. " You carry " .. cargo .. "." end
    local verb = #buyers == 1 and " buys" or " buy"
    return field .. " You carry " .. cargo .. ": " .. table.concat(buyers, " and ") .. verb .. " it."
end

-- Where a point `dx`, `dy` off lies, in one of eight words (north is +y). Slopes, not
-- trigonometry, so every machine gives the same word.
function rules.compass(dx, dy)
    local ax, ay = math.abs(dx), math.abs(dy)
    local slope = 0.41421356 -- tan 22.5 degrees
    local ns = dy >= 0 and "north" or "south"
    local ew = dx >= 0 and "east" or "west"
    if ay <= slope * ax then return ew end
    if ax <= slope * ay then return ns end
    return ns .. "-" .. ew
end

-- A station inside this box round the ship (half-width, half-height) is well on screen at
-- the starting view (64 units tall, so 32 up and down and at least 42 across in a 4:3 or
-- wider window) and wears its own name; outside it the bottom strip names it with the way
-- it lies. The box is kept inside the view, so a station is never both off screen and
-- unnamed; near the box's edge it may be on screen and named twice, which does no harm.
rules.ON_SCREEN = { x = 40, y = 24 }

-- "Lantern Dock south-west · Cinder Kiln south-east": each station in `stations`
-- ({ name, x, y }, in order) not well on screen from `at`, and the way it lies; "" when
-- every station is on screen.
function rules.bearings(at, stations)
    local parts = {}
    for _, s in stations do
        local dx, dy = s.x - at.x, s.y - at.y
        if math.abs(dx) > rules.ON_SCREEN.x or math.abs(dy) > rules.ON_SCREEN.y then
            table.insert(parts, s.name .. " " .. rules.compass(dx, dy))
        end
    end
    return table.concat(parts, " · ")
end

-- A counted visit eases a line back towards its target by its rate.
function rules.settle(line)
    if line.stock < line.target then return math.min(line.target, line.stock + line.rate) end
    return math.max(line.target, line.stock - line.rate)
end

-- Whether docking at `station` counts as a visit: unless the ship last docked there
-- and hasn't been away since, it does.
function rules.counts(last, away, station)
    return last ~= station or away
end

return rules
