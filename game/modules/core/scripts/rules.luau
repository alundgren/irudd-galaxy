-- Plain rules with no world access, so tests can call them directly.
local rules = {
    -- The bare hull's hold; fitted parts can add to it (fit.luau, fit.hold).
    CAPACITY = 6,
    MINE_DISTANCE = 7,
    -- A docking counts as a visit only after the ship has flown this far from the
    -- berth it last left, mined, or docked somewhere else.
    AWAY = 20,
    -- The goods a hold carries, in the order screens list them.
    WARES = { "ore", "plate", "rations" },
}

function rules.distance2(a, b)
    return (a.x - b.x) ^ 2 + (a.y - b.y) ^ 2
end

-- Units in a hold ({ ore = n, plate = n, rations = n }).
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
