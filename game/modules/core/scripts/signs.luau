-- The keepers' name signs (their engine.life name, with what they are doing). Each
-- keeper works in their own room, so only the people in the room you are in show. The
-- open log hides them all, so a sign never sits under a panel. While someone speaks
-- only they keep theirs, so the bubble never sits on another keeper's sign. A bubble
-- ends when you go into or out of a room, and never shows with the log (hush), so it
-- never rides a cut or sits on a panel.
local signs = {}

-- The keeper whose bubble is showing, if any.
function signs.speaking(ctx)
    for _, id in ctx.query("engine.talk") do
        local talk = ctx.get(id, "engine.talk")
        if talk.active then return talk.speaker end
    end
    return nil
end

-- Ends whatever is being said, as leaving a place does. The talk stays unseen, so an
-- unanswered offer is still on offer, and a click on the keeper begins it again. A
-- handler's reads come before its own writes: callers pass nil as `speaking` after.
function signs.hush(ctx)
    for _, id in ctx.query("engine.talk") do
        if ctx.get(id, "engine.talk").active then ctx.patch(id, "engine.talk", { active = false }) end
    end
end

-- `room`, `log_open`, `speaking` and `gliding` are the values after the calling
-- handler's own writes; `gliding` defaults to the pilot's. While the view moves no
-- sign is worn. A keeper's sign shows in their own room.
function signs.show(ctx, station, room, log_open, speaking, gliding)
    if gliding == nil then gliding = ctx.get("ship", "pilot").gliding end
    for _, id in ctx.query("facility") do
        local facility = ctx.get(id, "facility")
        if facility.station == station and facility.keeper ~= "" and ctx.get(facility.keeper, "engine.life") then
            local heard = (speaking == nil and not log_open) or facility.keeper == speaking
            local shown = not gliding and heard and room == id
            local name = shown and ctx.get(facility.keeper, "keeper").name or ""
            ctx.patch(facility.keeper, "engine.life", { name = name })
        end
    end
end

return signs
