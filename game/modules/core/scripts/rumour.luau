-- A rumour the player follows out and brings back (research: "Discovery and lore drip"):
-- the first strand of a web of open questions, kept to one thread of three beats.
--
--   LEAD   while a job needs the rumour's ware (Ines's greenhouse, glassite), the keeper
--          who buys it (Sefa at Cinder Kiln) says where it grows, in her own words: a
--          bearing and a landmark, never a marker (direction doc 2 and 9).
--   FIND   out there, by what she named, lies something nobody mentioned: an old stone
--          ring. Flying within `near` of it wakes it (`found`): its groove lights and
--          breathes, lilac motes rise off it and the log says what the player sees.
--   TELL   back with the keeper, she asks what the player saw (`tell`, a talk): her answer
--          explains part of it and leaves the rest open (`told`). Then whoever she would
--          tell (Mags, who keeps the charts) has heard of it, and says so once (`charted`).
--
-- Each beat is history (scripts/history.luau): a keeper who knows the player has been out
-- there doesn't send them there again, and nobody asks what the player has told.
-- Plain rules (no ctx) come first, so tests can call them; world reads and writes follow.
local project = require("./project")
local places = require("./places")

local rumour = {}

-- Whether the ship at `at` is close enough to `thing` at `there` to see it plainly.
function rumour.near(at, there, near)
    local dx, dy = at.x - there.x, at.y - there.y
    return dx * dx + dy * dy <= near * near
end

-- What the keeper says while the job needs the ware, in place of the job's own line:
-- remembering the find once the player has been out there, else the lead's landmark once
-- heard; nil until then (the lead itself is a talk of its own).
function rumour.needs(need, ware, cargo, giver, found, heard)
    local first = need .. " " .. ware .. " for " .. giver .. "'s " .. cargo
    if found then return first .. ", from round that old ring." end
    if heard then return first .. ". North of our berth, by the lone buoy." end
    return nil
end

-- World reads ------------------------------------------------------------------------

-- The rumour and its fields, or nil (a save from before it has none).
function rumour.find(ctx)
    for _, id in ctx.query("rumour") do return id, ctx.get(id, "rumour") end
    return nil, nil
end

-- Whether a keeper's introduction has been heard: a lead or a question waits for it,
-- so a stranger says who they are first. Heard to its end means seen, or its
-- conversation closed in the log; `done` is a talk whose last line was just heard.
local function met(ctx, keeper, done)
    local id = keeper .. "_intro"
    local intro = ctx.get(id, "engine.talk")
    if intro == nil or intro.seen or done == id then return true end
    for _, e in ctx.query("entry") do
        local entry = ctx.get(e, "entry")
        if entry.talk == id and not entry.open then return true end
    end
    return false
end

-- The job that needs the rumour's ware, while it is at the step that needs it, or nil.
local function wanting(ctx, r)
    local job = r.job ~= "" and ctx.get(r.job, "project")
    if job and job.step == project.ACCEPTED then return job end
    return nil
end

-- A keeper's line in place of their everyday one while the job needs the ware, or nil.
function rumour.line(ctx, keeper)
    local _, r = rumour.find(ctx)
    if not r or keeper ~= r.keeper then return nil end
    local job = wanting(ctx, r)
    if not job then return nil end
    return rumour.needs(job.need, job.ware, job.cargo, project.first(ctx.get(job.giver, "keeper").name), r.found, r.heard)
end

-- What a keeper met only now says first, before their introduction, and what the one who
-- keeps the charts says once in their everyday talk: they have heard of the find from the
-- keeper the player told. Nil when it is no news to them, or no longer news.
function rumour.news(ctx, keeper)
    local _, r = rumour.find(ctx)
    if not r or not r.told or r.charted or keeper ~= r.charter then return nil end
    return "Your ring's on my oldest chart, unnamed."
end

-- The keeper's idle line once she has been told: she remembers it, below anything more
-- pressing. Nil before.
function rumour.remembers(ctx, keeper)
    local _, r = rumour.find(ctx)
    if not r or not r.told or keeper ~= r.keeper then return nil end
    return "Pell won't cut near your ring. She says it hums."
end

-- Writes -----------------------------------------------------------------------------

-- Puts the lead and the question on or off at `station`: each is on while its keeper
-- stands there and has been met, the lead while the job needs the ware and nobody has
-- been out there, the question once the player has found it and not yet told. Both are
-- story talks, so the keeper's next click says them before anything else; both wait
-- while the keeper has a job's offer on (`busy`, speakers by id), so business comes
-- first. A talk being heard is never cut short, except the one whose last line was just
-- heard (`done`): voices.retire comes back for the rest.
function rumour.offers(ctx, station, done, busy)
    local _, r = rumour.find(ctx)
    if not r then return end
    local here = places.of(ctx, r.keeper) == station and met(ctx, r.keeper, done)
        and not (busy and busy[r.keeper])
    local want = {
        [r.lead] = here and not r.found and not r.heard and done ~= r.lead and wanting(ctx, r) ~= nil,
        [r.tell] = here and r.found and not r.told,
    }
    for talk, on in want do
        local said = ctx.get(talk, "engine.talk")
        if said and said.enabled ~= on and (not said.active or talk == done) then
            ctx.patch(talk, "engine.talk", { enabled = on })
        end
    end
end

-- A line of one of the rumour's talks was heard (`open` while more follow). The lead heard
-- to its end is heard; the line the chart keeper's news is in, heard, is no longer news.
-- `text` is the line's. Returns true when it changed what someone says next.
function rumour.said(ctx, talk, line, text, open)
    local id, r = rumour.find(ctx)
    if not r then return false end
    if talk == r.lead and not open and not r.heard then
        ctx.patch(id, "rumour", { heard = true })
        return true
    end
    if r.told and not r.charted and ctx.get(talk, "engine.talk").speaker == r.charter
        and text == rumour.news(ctx, r.charter) then
        ctx.patch(id, "rumour", { charted = true })
        return true
    end
    return false
end

-- The player answered the question: told, whatever was chosen, since the answer is
-- what they saw. The talk now starts at the reply (`reply`, the answer's next line), so
-- left before it ends, it picks up there and the question is never asked again.
-- Returns true when this was the rumour's.
function rumour.chosen(ctx, talk, reply)
    local id, r = rumour.find(ctx)
    if not r or talk ~= r.tell or r.told then return false end
    ctx.patch(id, "rumour", { told = true })
    if reply then ctx.patch(talk, "engine.talk", { first = reply }) end
    return true
end

-- What the find says in the log.
rumour.SEEN = "Lilac rocks by a lone buoy: the glassite drift. Beside them lies a ring of old stone, and its groove lights as you pass."

-- The ship flew close to what the rumour leads to: it wakes, once. Returns the log's line,
-- or nil when nothing happened.
function rumour.arrive(ctx, at)
    local id, r = rumour.find(ctx)
    if not r or r.found then return nil end
    local there = ctx.get(r.thing, "engine.position")
    if not there or not rumour.near(at, there, r.near) then return nil end
    ctx.patch(id, "rumour", { found = true })
    ctx.patch(r.thing, "engine.model", { model = r.found_model, look = r.found_look })
    ctx.put(r.thing, "engine.emitter", { recipe = r.motes, height = 0.8 })
    ctx.put(r.thing, "engine.burst", { recipe = r.wake, tick = ctx.tick, height = 0.6 })
    return rumour.SEEN
end

-- What pointing at the thing says: the find in plain words, once found.
function rumour.hover(ctx)
    local _, r = rumour.find(ctx)
    if not r then return nil, nil end
    if r.found then return r.thing, "A ring of old stone, grown through with glassite. Its groove glows." end
    return r.thing, "A ring of dark stone among the glassite."
end

return rumour
