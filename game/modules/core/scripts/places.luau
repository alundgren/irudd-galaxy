-- Where the ship is among the places. A station is a place with a port; each of its
-- buildings has a room, a child place of the station (start.toml), entered with the cut.
local places = {}

-- The station `place` belongs to: the place itself when it is a station, or the
-- station it is a room of; nil out in the reach.
function places.station(ctx, place)
    if place == nil or place == "" then return nil end
    if ctx.get(place, "port") then return place end
    local p = ctx.get(place, "engine.place")
    local parent = p and p.parent or ""
    if parent ~= "" and ctx.get(parent, "port") then return parent end
    return nil
end

-- The station the ship is docked at, on its deck or in one of its rooms, or nil.
function places.docked(ctx)
    return places.station(ctx, ctx.get("ship", "engine.location").place)
end

-- The station a being stands in, on its deck or in one of its rooms, or nil.
function places.of(ctx, id)
    local at = ctx.get(id, "engine.location")
    return at and places.station(ctx, at.place) or nil
end

-- The facility whose room `place` is, or nil.
function places.facility(ctx, place)
    for _, id in ctx.query("facility") do
        if ctx.get(id, "facility").place == place then return id end
    end
    return nil
end

-- A keeper arriving mid-day starts at the first act of their day, whatever the shared
-- clock reads: the clock picks an act by its value (each act lasts one unit), so the
-- day's acts are turned round until the one it picks is the first. `ahead` is how far
-- the caller has just moved the clock on (its reads still see the old value). Returns
-- the act to start from.
function places.first_act(ctx, first, ahead)
    local chain, act = {}, first
    while act ~= "" and #chain < 16 do
        table.insert(chain, act)
        act = ctx.get(act, "engine.act").next or ""
    end
    local clock = ctx.get("day", "engine.clock")
    local shift = clock and ((clock.value or 0) + ahead) % #chain or 0
    if shift == 0 then return first end
    local turned = {}
    for i = 1, #chain do turned[i] = chain[(i - 1 - shift) % #chain + 1] end
    for i, id in turned do ctx.patch(id, "engine.act", { next = turned[i + 1] or "" }) end
    return turned[1]
end

return places
