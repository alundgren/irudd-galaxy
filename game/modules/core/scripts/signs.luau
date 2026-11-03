-- The keepers' name signs (their engine.life name). On the whole station every keeper
-- wears one; inside a room only that room's keeper does, and the open log hides them
-- all, so a sign never sits under a panel. While someone speaks only they keep theirs,
-- so the bubble never sits on another keeper's sign.
local signs = {}

-- The keeper whose bubble is showing, if any.
function signs.speaking(ctx)
    for _, id in ctx.query("engine.talk") do
        local talk = ctx.get(id, "engine.talk")
        if talk.active then return talk.speaker end
    end
    return nil
end

-- `room`, `log_open`, `speaking` and `gliding` are the values after the calling
-- handler's own writes; `gliding` defaults to the pilot's. While the view glides no
-- sign is worn.
function signs.show(ctx, station, room, log_open, speaking, gliding)
    if gliding == nil then gliding = ctx.get("ship", "pilot").gliding end
    for _, id in ctx.query("facility") do
        local facility = ctx.get(id, "facility")
        if facility.station == station and facility.keeper ~= "" then
            local heard = (speaking == nil and not log_open) or facility.keeper == speaking
            local shown = not gliding and heard and (room == "" or room == id)
            local name = shown and ctx.get(facility.keeper, "keeper").name or ""
            ctx.patch(facility.keeper, "engine.life", { name = name })
        end
    end
end

return signs
