-- Where traders' name labels show, as plain rules tests can call. A label is offered
-- for every trader under way out in the reach, at any distance, except near a station,
-- where it would sit on the station's hull. The screen marks each label `yield`, so the
-- engine leaves it undrawn while its trader is off screen, while it would leave the
-- window and while it would overlap the HUD or another trader's label: that holds at
-- every zoom and window size, which a distance rule here couldn't see.
local rules = require("./rules")

local labels = {
    CLEAR = 17, -- this far from a station's centre: a second's flight past its hull
}

-- Which traders' labels are offered. `ship` is the player's ship's position, or nil when
-- it is not out in the reach; `traders` is { id, at, moored }; `outsides` the stations'
-- positions.
function labels.shown(ship, traders, outsides)
    local shown = {}
    for _, t in traders do
        local ok = ship ~= nil and not t.moored
        if ok then
            for _, s in outsides do
                if rules.distance2(t.at, s) < labels.CLEAR ^ 2 then ok = false end
            end
        end
        shown[t.id] = ok
    end
    return shown
end

-- Which side of its trader a label sits on: below a trader south of the player's
-- ship, above one north of it, so the label is never drawn over the player's ship.
function labels.below(ship, at)
    return at.y < ship.y
end

return labels
