-- The keepers' talk around the engine's bubbles: each heard conversation becomes one
-- ship log entry that names the speaker and the station, and each keeper's current
-- line follows the shift (voices.luau).
local log = require("./log")
local voices = require("./voices")
local teach = require("./teach")
local signs = require("./signs")

-- Who is speaking and where: "Tova Reyes at Lantern Dock".
local function source(ctx, talk)
    local speaker = ctx.get(talk, "engine.talk").speaker
    local port = ctx.get(ctx.get(speaker, "engine.location").place, "port")
    return ctx.get(speaker, "keeper").name .. (port and " at " .. port.name or "")
end

-- A line already kept in the log for this conversation is not logged again.
local function logged(ctx, talk, quoted)
    for _, id in ctx.query("entry") do
        local entry = ctx.get(id, "entry")
        if entry.talk == talk and string.find(entry.text, quoted, 1, true) then return true end
    end
    return false
end

return {
    on = {
        ["engine.talk_start"] = function(event, ctx)
            teach.used(ctx, "talk")
        end,
        -- A line was read and clicked on: it joins this conversation's log entry.
        ["engine.talk_next"] = function(event, ctx)
            local talk, line = event.payload.talk, event.payload.line
            local quoted = "\u{201C}" .. ctx.get(line, "engine.talk_line").text .. "\u{201D}"
            if logged(ctx, talk, quoted) then return end
            local newest = ctx.get("log" .. ctx.get("ship", "pilot").logged, "entry")
            if newest and newest.talk == talk then
                ctx.patch("log" .. ctx.get("ship", "pilot").logged, "entry", { text = newest.text .. " " .. quoted })
            else
                log.say(ctx, source(ctx, talk) .. ": " .. quoted, talk)
            end
        end,
        docked = function(event, ctx)
            local pilot = ctx.get("ship", "pilot")
            voices.refresh(ctx, event.payload.station, pilot.cargo, 0)
            signs.show(ctx, event.payload.station, "", pilot.log_open)
        end,
    },
}
