-- The keepers' talk around the engine's bubbles: each heard conversation becomes one
-- ship log entry that names the speaker and the station, and each keeper's current
-- line follows the shift (voices.luau).
local log = require("./log")
local voices = require("./voices")
local teach = require("./teach")
local signs = require("./signs")
local trade = require("./trade")
local rules = require("./rules")
local fit = require("./fit")
local places = require("./places")
local project = require("./project")
local salvage = require("./salvage")

-- Who is speaking and where: "Tova Reyes at Lantern Dock".
local function source(ctx, talk)
    local speaker = ctx.get(talk, "engine.talk").speaker
    local station = places.of(ctx, speaker)
    return ctx.get(speaker, "keeper").name .. (station and " at " .. ctx.get(station, "port").name or "")
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

-- Signs follow the talk: while someone speaks only they keep their name sign.
local function sign(ctx, speaker)
    local pilot = ctx.get("ship", "pilot")
    local station = places.docked(ctx)
    if station then signs.show(ctx, station, pilot.room, pilot.log_open, speaker) end
end

-- Every keeper's current line at the station the ship is docked at, as it stands now.
local function refresh(ctx)
    local station = places.docked(ctx)
    if not station then return end
    local _, build = trade.building(ctx, station)
    voices.refresh(ctx, station, ctx.get("ship", "hold"), 0, build and build.need - build.given or 0)
end

return {
    on = {
        ["engine.talk_start"] = function(event, ctx)
            -- The open log hides the keepers: a click on one through it starts nothing.
            if ctx.get("ship", "pilot").log_open then
                signs.hush(ctx)
                return
            end
            teach.used(ctx, "talk")
            sign(ctx, ctx.get(event.payload.talk, "engine.talk").speaker)
            voices.retire(ctx)
        end,
        -- A line was read and clicked on: it joins this conversation's log entry, which
        -- closes with the last line.
        ["engine.talk_next"] = function(event, ctx)
            local talk, line = event.payload.talk, event.payload.line
            local said = ctx.get(line, "engine.talk_line")
            local open = (said.next or "") ~= ""
            sign(ctx, open and ctx.get(talk, "engine.talk").speaker or nil)
            if not open then voices.retire(ctx) end
            local quoted = "\u{201C}" .. said.text .. "\u{201D}"
            -- An offer once said stays pinned in the log until it is answered (log.pinned).
            local gift = ctx.get(talk, "gift")
            if gift and not gift.heard then ctx.patch(talk, "gift", { heard = true }) end
            local offer = ctx.get(talk, "offer")
            if offer and not offer.heard then ctx.patch(talk, "offer", { heard = true }) end
            local id, entry = heard(ctx, talk)
            if logged(ctx, talk, quoted) then
                if id and not open then ctx.patch(id, "entry", { open = false }) end
            elseif id then
                ctx.patch(id, "entry", { text = entry.text .. " " .. quoted, open = open })
            else
                log.say(ctx, source(ctx, talk) .. ": " .. quoted, talk, open)
            end
        end,
        -- An answer to a line that offers a choice: the line and the answer join the
        -- conversation's entry. A gift taken goes into the hold with its own receipt;
        -- taken or declined, the offer is over, and once the talk ends the keeper's
        -- everyday talk returns. A project's offer taken moves the project on, with its
        -- receipt; "Not now" leaves it for a later docking (project.chosen).
        ["engine.talk_chosen"] = function(event, ctx)
            local talk, line, choice = event.payload.talk, event.payload.line, event.payload.choice
            local answer = ctx.get(choice, "engine.talk_choice")
            local open = (answer.next or "") ~= ""
            sign(ctx, open and ctx.get(talk, "engine.talk").speaker or nil)
            local said = "\u{201C}" .. ctx.get(line, "engine.talk_line").text .. "\u{201D} You: " .. answer.text .. "."
            local gift = ctx.get(talk, "gift")
            local receipt = nil
            if gift and not gift.answered then
                local hold = ctx.get("ship", "hold")
                local taken = choice == gift.take and fit.hold(ctx) - rules.load(hold) >= gift.units
                local over = taken or choice ~= gift.take
                ctx.patch(talk, "gift", { answered = over, heard = true })
                if over then
                    if not open then
                        ctx.patch(talk, "engine.talk", { enabled = false })
                        ctx.patch(gift.instead, "engine.talk", { enabled = true })
                    end
                end
                if taken then
                    ctx.patch("ship", "hold", { [gift.ware] = hold[gift.ware] + gift.units })
                    receipt = ctx.get(ctx.get(talk, "engine.talk").speaker, "keeper").name .. " gave you "
                        .. gift.ware .. ": " .. gift.units .. " into the hold."
                end
            end
            if not receipt then receipt = project.chosen(ctx, talk, choice, open, ctx.get("ship", "hold")) end
            if not receipt then receipt = salvage.chosen(ctx, talk, choice, open) end
            local id, entry = heard(ctx, talk)
            if id then
                ctx.patch(id, "entry", { text = entry.text .. " " .. said, open = open })
                if receipt then log.add(ctx, receipt) end
            else
                -- One new entry per handler: the receipt joins the conversation's.
                log.say(ctx, source(ctx, talk) .. ": " .. said .. (receipt and " " .. receipt or ""), talk, open)
            end
        end,
        -- The day moved on: someone may have come into the room you are in, visiting
        -- (the grower with greens for the galley), and wears their sign there.
        ["engine.clock_advanced"] = function(_, ctx)
            local station = places.docked(ctx)
            if not station then return end
            local pilot = ctx.get("ship", "pilot")
            signs.show(ctx, station, pilot.room, pilot.log_open, signs.speaking(ctx))
            -- The grower and the skipper talk of where they are: at their work or visiting.
            local _, job = salvage.find(ctx)
            if job then
                for _, who in { job.giver, job.skipper } do
                    local says = places.of(ctx, who) == station and salvage.line(ctx, who, station)
                    if says then ctx.patch(who .. "_now", "engine.talk_line", { text = says }) end
                end
            end
        end,
        -- A project moved on (an answer's receipt) or a build finished: the keepers'
        -- lines follow it. The handler that changed it still read the world before.
        confirmed = function(_, ctx) refresh(ctx) end,
        grown = function(_, ctx) refresh(ctx) end,
        docked = function(event, ctx)
            local pilot = ctx.get("ship", "pilot")
            local _, build = trade.building(ctx, event.payload.station)
            voices.refresh(ctx, event.payload.station, ctx.get("ship", "hold"), 0, build and build.need - build.given or 0)
            signs.show(ctx, event.payload.station, pilot.room, pilot.log_open)
        end,
    },
}
