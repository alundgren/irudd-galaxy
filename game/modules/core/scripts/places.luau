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

return places
