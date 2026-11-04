-- The ship log (direction doc 4): one entity per entry, newest kept, oldest dropped,
-- grouped into visits for its screen (logbook.luau). Closed, the log is a small book button at the bottom left with an
-- "N new" tag. Open (L or the book), it shows what is still waiting at the top,
-- pinned: an offer you haven't answered, under who made it and where, and what a
-- station is building from your deliveries. Below, one line per visit, newest first:
-- where, who you heard, and what happened there; Lines opens a visit to show what was
-- said and done in order. What happened in flight between two dockings is one line too.
local log = { KEEP = 60 }

-- Where an entry belongs: the station the ship is at, or "" in flight. The docking's
-- own entry is written on the tick the ship arrives, still out in the reach, so a ship
-- finishing a dock order already belongs to the station it docks at.
local function at(ctx)
    local place = ctx.get("ship", "engine.location").place
    if place ~= "" and ctx.get(place, "port") then return place end
    local steer = ctx.get("ship", "engine.steer")
    if steer and steer.mode == "dock" then
        for _, id in ctx.query("port") do
            if ctx.get(id, "port").berth == steer.target then return id end
        end
    end
    return ""
end

-- Adds one entry. Call at most once per handler: reads see the world as it was when
-- the handler started. `talk` marks an entry that holds a conversation, and `open` one
-- whose conversation goes on.
local function put(ctx, text, talk, open)
    local n = ctx.get("ship", "pilot").logged + 1
    ctx.put("log" .. n, "entry", { n = n, text = text, talk = talk or "", open = open or false, at = at(ctx) })
    if n > log.KEEP then ctx.remove("log" .. (n - log.KEEP)) end
    return n
end

-- What just happened to the ship: logged and shown as the current message.
function log.add(ctx, text)
    ctx.patch("ship", "pilot", { logged = put(ctx, text), message = text })
end

-- Something a person said: logged only, so talk never follows the ship into space.
function log.say(ctx, text, talk, open)
    ctx.patch("ship", "pilot", { logged = put(ctx, text, talk, open) })
end

-- The entries in order, oldest first.
function log.entries(ctx)
    local out = {}
    for _, id in ctx.query("entry") do table.insert(out, ctx.get(id, "entry")) end
    table.sort(out, function(a, b) return a.n < b.n end)
    return out
end

-- Whether a conversation has been heard at all.
local function heard(list, talk)
    for _, entry in list do
        if entry.talk == talk then return true end
    end
    return false
end

-- Visits: runs of entries in one place, newest first. Each is { at, first, entries }.
function log.visits(list)
    local visits = {}
    for _, entry in list do
        local last = visits[#visits]
        if last and last.at == entry.at then
            table.insert(last.entries, entry)
        else
            table.insert(visits, { at = entry.at, first = entry.n, entries = { entry } })
        end
    end
    local newest = {}
    for i = #visits, 1, -1 do table.insert(newest, visits[i]) end
    return newest
end

-- A sentence as part of a summary: its first letter lower, its full stop gone.
local function clause(text)
    text = string.gsub(text, "%.$", "")
    return string.lower(string.sub(text, 1, 1)) .. string.sub(text, 2)
end

-- What a visit comes to in one line: what happened, without the comings and goings,
-- and who was heard. Returns the summary and the speakers' names.
function log.summary(ctx, visit)
    local done, people, seen = {}, {}, {}
    local port = visit.at ~= "" and ctx.get(visit.at, "port")
    -- The visit's line already names the station, so its clauses needn't.
    local function trim(text)
        if not port then return text end
        for _, joint in { " at ", " to ", " from " } do
            local s, e = string.find(text, joint .. port.name, 1, true)
            if s then text = string.sub(text, 1, s - 1) .. string.sub(text, e + 1) end
        end
        return text
    end
    for _, entry in visit.entries do
        if entry.talk ~= "" then
            local talk = ctx.get(entry.talk, "engine.talk")
            local name = talk and talk.title or ""
            if name ~= "" and not seen[name] then
                seen[name] = true
                table.insert(people, name)
            end
        elseif not string.find(entry.text, "^Docked at ") and not string.find(entry.text, "^Launched from ") then
            table.insert(done, clause(trim(entry.text)))
        end
    end
    local summary = table.concat(done, "; ")
    if summary == "" then summary = #people > 0 and "talked" or "docked and left" end
    return summary, people
end

-- What is still waiting: offers made and not answered, and buildings under way at a
-- station you have been to. Each is { kind, who, text }.
function log.pinned(ctx, list)
    local pins = {}
    for _, talk in ctx.query("gift") do
        local gift = ctx.get(talk, "gift")
        if not gift.answered and heard(list, talk) then
            local said = ctx.get(talk, "engine.talk")
            local speaker = said.speaker
            local port = ctx.get(ctx.get(speaker, "engine.location").place, "port")
            local take = ctx.get(gift.take, "engine.talk_choice")
            local answers = {}
            for _, id in ctx.query("engine.talk_choice") do
                local choice = ctx.get(id, "engine.talk_choice")
                if choice.line == take.line then table.insert(answers, choice) end
            end
            table.sort(answers, function(a, b) return (a.order or 0) < (b.order or 0) end)
            local words = {}
            for _, choice in answers do table.insert(words, choice.text) end
            table.insert(pins, { kind = "OFFER", who = ctx.get(speaker, "keeper").name, where = port and port.name or "",
                text = gift.units .. " " .. gift.ware .. " on the house. Answer " .. table.concat(words, " or ") .. "." })
        end
    end
    for _, id in ctx.query("build") do
        local build = ctx.get(id, "build")
        local visited = false
        for _, entry in list do
            if entry.at == build.station then visited = true end
        end
        if not build.done and visited then
            local port = ctx.get(build.station, "port")
            table.insert(pins, { kind = "WANTED", who = build.name, where = port.name,
                text = build.given .. " of " .. build.need .. " " .. build.ware .. " in. Sell " .. build.ware
                    .. " at " .. port.name .. " to build it." })
        end
    end
    return pins
end

return log
