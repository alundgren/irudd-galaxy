-- The keepers' name signs (their engine.life name). On the whole station every keeper
-- wears one; inside a room only that room's keeper does, and the open log hides them
-- all, so a sign never sits under a panel. The one who is speaking keeps theirs: their
-- bubble stays up, and speech always says who it is from.
local signs = {}

-- The keeper whose bubble is showing, if any.
function signs.speaking(ctx)
    for _, id in ctx.query("engine.talk") do
        local talk = ctx.get(id, "engine.talk")
        if talk.active then return talk.speaker end
    end
    return nil
end

-- `room`, `log_open` and `speaking` are the values after the calling handler's own
-- writes.
function signs.show(ctx, station, room, log_open, speaking)
    for _, id in ctx.query("facility") do
        local facility = ctx.get(id, "facility")
        if facility.station == station then
            local heard = not log_open or facility.keeper == speaking
            local shown = heard and (room == "" or room == id)
            local name = shown and ctx.get(facility.keeper, "keeper").name or ""
            ctx.patch(facility.keeper, "engine.life", { name = name })
        end
    end
end

return signs
