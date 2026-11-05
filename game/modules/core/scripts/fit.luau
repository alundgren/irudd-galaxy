-- The ship's fit: what its parts add up to, and whether a part may go on or come off.
-- Plain rules with no world access, so tests can call them directly; `fit.hold(ctx)`
-- is the one world read, the hold's capacity, which screens and rules all share.
--
-- A part is a table { id, name, socket, price, power, mass, hold, mine, speed, owned,
-- fitted } (the `part` component plus its entity ID). Power caps what can run at
-- once, so a part that would draw more than the hull has is refused. Mass past what the
-- hull carries freely costs top speed and turn rate for each unit over; it never
-- refuses. Controls sit in the control rack and are never part of the fit.
local fit = {
    POWER = 3, -- the hull's power: what its parts may draw at once
    FREE_MASS = 3, -- mass the hull carries with no cost
    SLOW = 0.1, -- top speed and turn rate lost for each unit of mass over
    FLOOR = 0.5, -- never slower than this share
    HOLD = 6, -- the bare hull's hold
    MINE = 1, -- ore a second the bare hull's beam cuts
    SPEED = 20, -- the bare hull's top speed, world units a second, in the locked flight
    OLD_SPEED = 12, -- the bare hull's top speed in a save from before the locked flight
    TURN = 180, -- the bare hull's turn rate, degrees a second
    -- The sockets, in the order the fitting room shows them.
    SOCKETS = {
        { id = "drill", name = "Drill mount" },
        { id = "hold", name = "Hold" },
        { id = "engine", name = "Engine" },
    },
}

function fit.socket_name(id)
    for _, s in fit.SOCKETS do
        if s.id == id then return s.name end
    end
    return id
end

-- What the hull and its fitted parts add up to. `without` leaves one fitted part out,
-- and `with` adds one that isn't fitted, to ask what a change would make. `hull` is the
-- bare hull's top speed (fit.hull_speed); fit.SPEED when left out.
function fit.totals(parts, without, with, hull)
    local t = { power = 0, mass = 0, hold = fit.HOLD, mine = fit.MINE, speed = hull or fit.SPEED }
    for _, p in parts do
        local on = (p.fitted and p.id ~= without) or p.id == with
        if on then
            t.power += p.power
            t.mass += p.mass
            t.hold += p.hold
            t.mine += p.mine
            t.speed += p.speed
        end
    end
    t.over = math.max(0, t.mass - fit.FREE_MASS)
    t.factor = math.max(fit.FLOOR, 1 - fit.SLOW * t.over)
    t.top = t.speed * t.factor
    t.turn = fit.TURN * t.factor
    return t
end

-- Why `p` can't be dropped on `target` (a socket ID, or "locker" to take it off), or ""
-- when it can. Every rule answers here, so the room shows each refusal while the part
-- is still being dragged, before it is let go. `load` is the cargo aboard.
function fit.verdict(p, target, parts, credits, load)
    if target == "locker" then
        if not p.fitted then return "it isn't fitted" end
        -- A control in the rack (socket "rack") is never taken away once bought.
        if p.socket == "rack" then return "controls never come off" end
        local after = fit.totals(parts, p.id, nil)
        if load > after.hold then return "you carry " .. load .. "; the hold would take " .. after.hold end
        return ""
    end
    if p.socket ~= target then return "it goes in the " .. string.lower(fit.socket_name(p.socket)) end
    if p.fitted then return "it's fitted here" end
    -- Power first: without it the part couldn't go on even once paid for.
    local free = fit.POWER - fit.totals(parts).power
    if p.power > free then return "it needs " .. p.power .. " power; " .. free .. " is free" end
    if not p.owned and credits < p.price then return "it costs " .. p.price .. " cr; you have " .. credits end
    return ""
end

-- What fitting `p` (or taking it off, when it is fitted) would change, before it
-- happens, one change each: what it costs, then each number that moves, from what it
-- is to what it would be. Each is { text, good }: good is true when that is better,
-- false when worse, nil for the cost and the power left free. `hull` as in fit.totals.
function fit.change_list(p, parts, hull)
    local before = fit.totals(parts, nil, nil, hull)
    local after = p.fitted and fit.totals(parts, p.id, nil, hull) or fit.totals(parts, nil, p.id, hull)
    local list = {}
    if not p.fitted then table.insert(list, { text = p.owned and "from your locker, free" or p.price .. " cr" }) end
    local function moved(name, a, b, unit)
        if fit.num(a) ~= fit.num(b) then
            table.insert(list, { text = name .. " " .. fit.num(a) .. " to " .. fit.num(b) .. (unit or ""), good = b > a })
        end
    end
    moved("hold", before.hold, after.hold)
    moved("mines", before.mine, after.mine, " a second")
    moved("top speed", before.top, after.top)
    moved("turns", before.turn, after.turn, "°")
    local free_before, free_after = fit.POWER - before.power, fit.POWER - after.power
    if free_before ~= free_after then
        table.insert(list, { text = "power free " .. free_before .. " to " .. free_after })
    end
    return list
end

-- The same changes in one line: "90 cr · mines 1 to 2 a second, better · power free 3 to 1".
function fit.changes(p, parts, hull)
    local words = {}
    for _, c in fit.change_list(p, parts, hull) do
        table.insert(words, c.text .. (c.good == nil and "" or c.good and ", better" or ", worse"))
    end
    return table.concat(words, " · ")
end

-- The short form a dragged part shows under "Drop here": its cost and what it gives.
function fit.note(p)
    return (p.owned and "free" or p.price .. " cr") .. " · " .. fit.effect(p)
end

-- A number with at most one decimal: 13.5, 12.
function fit.num(x)
    local r = math.floor(x * 10 + 0.5) / 10
    if r == math.floor(r) then return tostring(math.floor(r)) end
    return string.format("%.1f", r)
end

-- "Power 2 of 3 in use, 1 free"
function fit.power_text(t)
    return "Power " .. t.power .. " of " .. fit.POWER .. " in use, " .. (fit.POWER - t.power) .. " free"
end

-- "Mass 4: the hull carries 3 freely. 1 over: 10% slower, wider turns"
function fit.mass_text(t)
    local s = "Mass " .. t.mass .. ": the hull carries " .. fit.FREE_MASS .. " freely"
    if t.over > 0 then
        local pct = math.floor((1 - t.factor) * 100 + 0.5)
        return s .. ". " .. t.over .. " over: " .. pct .. "% slower, wider turns"
    end
    return s .. ", " .. (fit.FREE_MASS - t.mass) .. " more before it slows you"
end

-- "Hold 9 · mines 1 a second · top speed 18 · turns 162° a second"
function fit.ship_text(t)
    return "Hold " .. t.hold .. " · mines " .. t.mine .. " a second · top speed " .. fit.num(t.top)
        .. " · turns " .. fit.num(t.turn) .. "° a second"
end

-- What a part does, in numbers: "mines 2 a second", "hold +3", "top speed +5".
function fit.effect(p)
    local parts = {}
    if p.mine > 0 then table.insert(parts, "mines " .. (fit.MINE + p.mine) .. " a second") end
    if p.hold > 0 then table.insert(parts, "hold +" .. p.hold) end
    if p.speed > 0 then table.insert(parts, "top speed +" .. fit.num(p.speed)) end
    return table.concat(parts, " · ")
end

-- What a part draws and weighs: "power 2 · mass 1".
function fit.cost(p)
    return "power " .. p.power .. " · mass " .. p.mass
end

-- The hold's capacity with the parts fitted now.
function fit.hold(ctx)
    local f = ctx.get("ship", "fit")
    return f and f.hold or fit.HOLD
end

-- The bare hull's top speed this ship was built with: fit.SPEED in a new game. A save
-- from before the locked flight keeps flying the old hull, so its `fit` loads with the
-- field's default, fit.OLD_SPEED, and the cradle goes on from the speed it really has.
function fit.hull_speed(ctx)
    local f = ctx.get("ship", "fit")
    return f and f.hull_speed or fit.SPEED
end

-- Ore a second the beam cuts with the parts fitted now.
function fit.mine(ctx)
    local f = ctx.get("ship", "fit")
    return f and f.mine or fit.MINE
end

return fit
