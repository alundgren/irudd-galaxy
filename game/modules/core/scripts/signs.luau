-- The keepers' name signs (their engine.life name, with what they are doing). Each
-- keeper works in their own room, so only the people in the room you are in show. The
-- open log hides them all, so a sign never sits under a panel, and so does a chosen
-- part's preview for the cradle's keeper, whose walk to the ship passes under it (the
-- panel names them meanwhile). While someone speaks
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

-- `room`, `log_open`, `speaking`, `gliding` and `chosen` (the chosen part) are the
-- values after the calling handler's own writes; `gliding` and `chosen` default to the
-- pilot's. While the view moves no sign is worn. A keeper's sign shows in their own room.
--
-- Only the keeper of the room you are in walks where you can see it (presentation
-- "states"); everyone else's day moves on at once, out of sight, and leaving a room
-- mid-walk finishes the walk where it was going. Every caller that moves the player
-- calls this. (Keeping the others "instant" is also what lets check try every
-- arrangement of the day in reasonable time.)
function signs.show(ctx, station, room, log_open, speaking, gliding, chosen)
    local pilot = ctx.get("ship", "pilot")
    if gliding == nil then gliding = pilot.gliding end
    if chosen == nil then chosen = pilot.chosen_part end
    for _, id in ctx.query("facility") do
        local facility = ctx.get(id, "facility")
        local life = facility.keeper ~= "" and ctx.get(facility.keeper, "engine.life")
        if life then
            local here = facility.station == station and room == id
            local heard = (speaking == nil and not log_open) or facility.keeper == speaking
            local previewing = facility.fits and (chosen or "") ~= ""
            local name = here and not gliding and heard and not previewing and ctx.get(facility.keeper, "keeper").name or ""
            local presentation = here and "states" or "instant"
            if life.name ~= name or life.presentation ~= presentation then
                ctx.patch(facility.keeper, "engine.life", { name = name, presentation = presentation })
            end
        end
    end
end

return signs
