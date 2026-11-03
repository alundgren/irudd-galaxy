-- The keepers' name signs (their engine.life name). On the whole station every keeper
-- wears one; inside a room only that room's keeper does, and the open log hides them
-- all, so a sign never sits under a panel.
local signs = {}

-- `room` and `log_open` are the values after the calling handler's own writes.
function signs.show(ctx, station, room, log_open)
    for _, id in ctx.query("facility") do
        local facility = ctx.get(id, "facility")
        if facility.station == station then
            local shown = not log_open and (room == "" or room == id)
            local name = shown and ctx.get(facility.keeper, "keeper").name or ""
            ctx.patch(facility.keeper, "engine.life", { name = name })
        end
    end
end

return signs
