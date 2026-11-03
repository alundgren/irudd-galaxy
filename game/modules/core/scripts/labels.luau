-- Where traders' name labels show, as plain rules tests can call. A label shows only
-- where it reads cleanly: out in open space near the player's ship, clear of the
-- stations and of another trader's label. Scripts can't see the zoom, so "near" is
-- what stays clear of the orders, the status, the log button and the message at the
-- default zoom (a view of 64 units) and wider, in a 1280x720 or a 1024x576 window.
local rules = require("./rules")

local labels = {
    NEAR = 12, -- within this distance of the player's ship, so it sits mid-screen
    CLEAR = 17, -- this far from a station's centre: a second's flight past its hull
    APART = 9, -- this far from a trader whose label already shows
}

-- Which traders' labels show. `ship` is the player's ship's position, or nil when it is
-- not out in the reach; `traders` is { id, at, moored } in ID order (the first of two
-- close traders keeps its label); `outsides` the stations' positions.
function labels.shown(ship, traders, outsides)
    local shown, placed = {}, {}
    for _, t in traders do
        local ok = ship ~= nil and not t.moored
            and rules.distance2(t.at, ship) <= labels.NEAR ^ 2
        if ok then
            for _, s in outsides do
                if rules.distance2(t.at, s) < labels.CLEAR ^ 2 then ok = false end
            end
        end
        if ok then
            for _, other in placed do
                if rules.distance2(t.at, other) < labels.APART ^ 2 then ok = false end
            end
        end
        shown[t.id] = ok
        if ok then table.insert(placed, t.at) end
    end
    return shown
end

-- Which side of its trader a label sits on: below a trader south of the player's
-- ship, above one north of it, so the label is never drawn over the player's ship.
function labels.below(ship, at)
    return at.y < ship.y
end

return labels
