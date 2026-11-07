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

-- An act of someone's day says something new; whoever is at that act now shows it on
-- their sign at once, not only from their next act on.
function places.restate(ctx, act, state)
    ctx.patch(act, "engine.act", { state = state })
    for _, who in ctx.query("keeper") do
        local life = ctx.get(who, "engine.life")
        if life and life.current == act then ctx.patch(who, "engine.being", { state = state }) end
    end
end

-- Sends `who` to `act` now, the rest of their day following on from it, so where you can
-- see them they get up and walk there (engine.life): what you just did shows in the room.
-- An act not yet in their day joins it, for good. Their chain is turned so the shared
-- clock picks `act`, as first_act does (reads see the chain before these writes).
function places.go(ctx, who, act)
    local life = ctx.get(who, "engine.life")
    if not life or not ctx.get(act, "engine.act") then return end
    local day, at = {}, life.first
    while at and at ~= "" and #day < 16 do
        table.insert(day, at)
        at = ctx.get(at, "engine.act").next or ""
    end
    local from = table.find(day, act)
    if not from then
        table.insert(day, 1, act)
        from = 1
    end
    local chain = {}
    for i = 1, #day do chain[i] = day[(from + i - 2) % #day + 1] end
    local clock = ctx.get("day", "engine.clock")
    local shift = clock and (clock.value or 0) % #chain or 0
    local turned = {}
    for i = 1, #chain do turned[i] = chain[(i - 1 - shift) % #chain + 1] end
    for i, id in turned do ctx.patch(id, "engine.act", { next = turned[i + 1] or "" }) end
    ctx.patch(who, "engine.life", { first = turned[1] })
end

-- A being resting at an act with a model of its own (`pose`: a seated figure at a table)
-- wears it while there, and its keeper figure again once its day moves on. Only for
-- beings whose changes show at once: one walking keeps the figure it walks in.
function places.dress(ctx)
    local posed = {}
    for _, id in ctx.query("pose") do posed[ctx.get(id, "pose").model] = true end
    for _, who in ctx.query("keeper") do
        local life = ctx.get(who, "engine.life")
        local model = life and life.presentation == "instant" and ctx.get(who, "engine.model")
        if model then
            local pose = (life.current or "") ~= "" and ctx.get(life.current, "pose")
            local figure = ctx.get(who, "keeper").figure or ""
            local want = pose and pose.model or (posed[model.model] and figure ~= "" and figure) or model.model
            if want ~= model.model then ctx.patch(who, "engine.model", { model = want }) end
        end
    end
end

return places
