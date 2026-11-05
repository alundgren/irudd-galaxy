-- The ship log (direction doc 4): one entity per entry, newest kept, oldest dropped,
-- grouped into visits for its screen (logbook.luau). Closed, the log is a small book button at the bottom left with an
-- "N new" tag. Open (L or the book), it shows what is still waiting at the top,
-- pinned: an offer you haven't answered, under who made it and where, and what a
-- station is building from your deliveries. Below, one line per visit, newest first:
-- where, then one short line: who you heard and what happened there; Lines opens a
-- visit to show what was said and done in order. What happened in flight between two
-- dockings is one line too. Each docking is a visit of its own, even back at the same
-- station with nothing logged in between.
-- KEEP entries are kept; a visit's short line holds at most SHORT characters.
local log = { KEEP = 60, SHORT = 38 }

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

-- Adds one entry. Call at most once per handler (log.add_all logs several): reads see
-- the world as it was when the handler started. `talk` marks an entry that holds a
-- conversation, `open` one whose conversation goes on, and `leaves` the one the ship
-- leaves the station with. `after` is the entry it follows: the last logged, by default.
local function put(ctx, text, talk, open, leaves, after)
    local n = (after or ctx.get("ship", "pilot").logged) + 1
    ctx.put("log" .. n, "entry", { n = n, text = text, talk = talk or "", open = open or false, at = at(ctx),
        leaves = leaves or false })
    if n > log.KEEP then ctx.remove("log" .. (n - log.KEEP)) end
    return n
end

-- What just happened to the ship: logged and shown as the current message.
function log.add(ctx, text)
    ctx.patch("ship", "pilot", { logged = put(ctx, text), message = text })
end

-- Several things that happened at once, logged in order; the last is the message.
function log.add_all(ctx, texts)
    local n = ctx.get("ship", "pilot").logged
    for _, text in texts do n = put(ctx, text, nil, nil, nil, n) end
    ctx.patch("ship", "pilot", { logged = n, message = texts[#texts] })
end

-- Leaving the station: logged and shown like log.add, and the visit ends with it.
function log.leave(ctx, text)
    ctx.patch("ship", "pilot", { logged = put(ctx, text, nil, nil, true), message = text })
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

-- Visits: runs of entries in one place, newest first, a new one after each leaving.
-- Each is { at, first, entries }.
function log.visits(list)
    local visits = {}
    for _, entry in list do
        local last = visits[#visits]
        local before = last and last.entries[#last.entries]
        if last and last.at == entry.at and not before.leaves then
            table.insert(last.entries, entry)
        else
            table.insert(visits, { at = entry.at, first = entry.n, entries = { entry } })
        end
    end
    local newest = {}
    for i = #visits, 1, -1 do table.insert(newest, visits[i]) end
    return newest
end

-- What an entry comes to in a few words: its first clause, lower case, without the
-- station the visit's line already names, the price or the explanation after it
-- ("Sold 6 ore to Cinder Kiln for 97 cr. ..." is "sold 6 ore").
local function short(text, port)
    if port then
        for _, joint in { " at ", " to ", " from " } do
            local s, e = string.find(text, joint .. port.name, 1, true)
            if s then text = string.sub(text, 1, s - 1) .. string.sub(text, e + 1) end
        end
    end
    local cut = #text + 1
    for _, stop in { " for ", ":", ".", ",", ";" } do
        local s = string.find(text, stop, 1, true)
        if s and s < cut then cut = s end
    end
    text = string.sub(text, 1, cut - 1)
    return string.lower(string.sub(text, 1, 1)) .. string.sub(text, 2)
end

-- Clauses joined by ", " in at most `room` characters; what doesn't fit is counted
-- ("+2 more") rather than shown.
local function fit_line(clauses, room)
    local line, shown = "", 0
    for i, c in clauses do
        local next = line == "" and c or line .. ", " .. c
        local rest = #clauses - i
        local tail = rest > 0 and " +" .. rest .. " more" or ""
        if #next + #tail > room then break end
        line, shown = next, i
    end
    if shown == 0 and #clauses > 0 then
        -- Even the first is too long: as much of it as fits.
        local tail = #clauses > 1 and " +" .. (#clauses - 1) .. " more" or ""
        return string.sub(clauses[1], 1, room - #tail - 3) .. "..." .. tail
    end
    if shown < #clauses then line ..= " +" .. (#clauses - shown) .. " more" end
    return line
end

-- What a visit comes to in one short line: who was heard (the first, and how many
-- more) and what happened, without the comings and goings, each said once. Returns the
-- names and the line after them; together they fit in log.SHORT characters.
function log.summary(ctx, visit)
    local done, people, seen = {}, {}, {}
    local port = visit.at ~= "" and ctx.get(visit.at, "port")
    for _, entry in visit.entries do
        if entry.talk ~= "" then
            local talk = ctx.get(entry.talk, "engine.talk")
            local name = talk and talk.title or ""
            if name ~= "" and not seen[name] then
                seen[name] = true
                table.insert(people, name)
            end
        elseif not entry.leaves and not string.find(entry.text, "^Docked at ") and not string.find(entry.text, "^Launched from ") then
            local c = short(entry.text, port)
            if c ~= "" and not seen[c] then
                seen[c] = true
                table.insert(done, c)
            end
        end
    end
    local who = people[1] or ""
    if #people > 1 then who ..= " +" .. (#people - 1) end
    local room = log.SHORT - (who ~= "" and #who + 3 or 0)
    local line = fit_line(done, room)
    if line == "" then line = who ~= "" and "talked" or "docked and left" end
    return who, line
end

-- What is still waiting: offers made and not answered, and buildings under way at a
-- station you have been to. Each is { kind, who, text }. An offer stays pinned once
-- heard (its gift is marked heard), and a building once its station has been docked
-- at, however much older history the log has dropped since.
function log.pinned(ctx, list)
    local pins = {}
    for _, talk in ctx.query("gift") do
        local gift = ctx.get(talk, "gift")
        if not gift.answered and (gift.heard or heard(list, talk)) then
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
        local port = ctx.get(build.station, "port")
        if not build.done and (visited or port.visits > 0) then
            table.insert(pins, { kind = "WANTED", who = build.name, where = port.name,
                text = build.given .. " of " .. build.need .. " " .. build.ware .. " in. Sell " .. build.ware
                    .. " at " .. port.name .. " to build it." })
        end
    end
    return pins
end

return log
