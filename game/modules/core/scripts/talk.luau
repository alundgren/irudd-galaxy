-- The keepers' talk around the engine's bubbles: each heard conversation becomes one
-- ship log entry that names the speaker and the station, and each keeper's current
-- line follows the shift (voices.luau).
local log = require("./log")
local voices = require("./voices")
local teach = require("./teach")
local signs = require("./signs")
local trade = require("./trade")

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

-- The entry of this talk's conversation still being heard, wherever other entries
-- have come in since: a sale between two lines doesn't split the conversation.
local function heard(ctx, talk)
    for _, id in ctx.query("entry") do
        local entry = ctx.get(id, "entry")
        if entry.talk == talk and entry.open then return id, entry end
    end
    return nil, nil
end

-- Signs follow the talk only while the log is open: its speaker keeps their name.
local function sign(ctx, speaker)
    local pilot = ctx.get("ship", "pilot")
    local here = ctx.get("ship", "engine.location").place
    if pilot.log_open and here ~= "" and ctx.get(here, "port") then
        signs.show(ctx, here, pilot.room, true, speaker)
    end
end

return {
    on = {
        ["engine.talk_start"] = function(event, ctx)
            teach.used(ctx, "talk")
            sign(ctx, ctx.get(event.payload.talk, "engine.talk").speaker)
        end,
        -- A line was read and clicked on: it joins this conversation's log entry, which
        -- closes with the last line.
        ["engine.talk_next"] = function(event, ctx)
            local talk, line = event.payload.talk, event.payload.line
            local said = ctx.get(line, "engine.talk_line")
            local open = (said.next or "") ~= ""
            sign(ctx, open and ctx.get(talk, "engine.talk").speaker or nil)
            local quoted = "\u{201C}" .. said.text .. "\u{201D}"
            local id, entry = heard(ctx, talk)
            if logged(ctx, talk, quoted) then
                if id and not open then ctx.patch(id, "entry", { open = false }) end
            elseif id then
                ctx.patch(id, "entry", { text = entry.text .. " " .. quoted, open = open })
            else
                log.say(ctx, source(ctx, talk) .. ": " .. quoted, talk, open)
            end
        end,
        docked = function(event, ctx)
            local pilot = ctx.get("ship", "pilot")
            local _, build = trade.building(ctx, event.payload.station)
            voices.refresh(ctx, event.payload.station, ctx.get("ship", "hold"), 0, build and build.need - build.given or 0)
            signs.show(ctx, event.payload.station, "", pilot.log_open)
        end,
    },
}
