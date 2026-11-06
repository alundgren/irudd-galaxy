-- A project someone asks the player's help with, step by step (direction doc 3.3 and 4):
-- after the galley, Ines wants a greenhouse at Lantern Dock. Every step waits on the
-- player's answer to an offer in someone's talk (`offer` on the talk entity), and "Not
-- now" leaves it on offer again from the next docking. Its steps (`step` on `project`):
--
--   ASKED     the giver (Ines) asks once her gift is answered: Accept or Not now.
--   ACCEPTED  the greenhouse's plot is staked out; bring `need` `ware` (glassite) to the
--             maker (Sefa at Cinder Kiln): Hand it over or Not now.
--   FIRING    the maker fires `units` `cargo` (panes) from it; they are ready on the next
--             counted visit to her station (`fired` is its visit count when she began).
--   READY     the maker offers to load them: Take them or Not now. Once loaded they are
--             the giver's, marked so in the hold, and no market buys them.
--   CARRIED   the giver offers to take them: Hand them over or Not now.
--   BUILDING  the site stops waiting: scaffold and glaziers on the deck, and the ware
--             its build takes (ice for the beds) goes in at the exchange.
--   DONE      the greenhouse stands, its grower arrives, the station holds a lamp night
--             (a `happening`) and the giver thanks you.
--
-- Plain rules (no ctx) come first, so tests can call them; the world reads and writes
-- follow. Handlers' reads see the world before their own writes, so callers that have
-- just changed the hold or the step pass the new values in.
local rules = require("./rules")
local fit = require("./fit")
local places = require("./places")
local happen = require("./happen")
local salvage = require("./salvage")

local project = { ASKED = 1, ACCEPTED = 2, FIRING = 3, READY = 4, CARRIED = 5, BUILDING = 6, DONE = 7 }

-- The first name of a full name: "Ines" for "Ines Varga".
function project.first(name)
    return string.match(name, "^%S+") or name
end

-- Whether an offer made at `step` is on: the project is at that step and what it needs
-- holds. `hold` is what the ship carries, `room` the space left in it, and `asked` whether
-- the giver is ready to ask (her gift answered).
function project.wanted(job, step, hold, room, asked)
    if job.step ~= step then return false end
    if step == project.ASKED then return asked end
    if step == project.ACCEPTED then return (hold[job.ware] or 0) >= job.need end
    if step == project.READY then return room >= job.units end
    if step == project.CARRIED then return (hold[job.cargo] or 0) >= job.units end
    return false
end

-- Whether the maker's goods are ready: her station has had a counted visit since she
-- began firing them.
function project.fired(job, visits)
    return job.step == project.FIRING and visits > job.fired
end

-- What the log pins while the project is under way: the next thing to do, in the giver's
-- terms. `names` holds the giver's, the maker's and their stations' names; `carried` how
-- much of the ware the hold has now. Nil before it is accepted and once it is building
-- (the build's own WANTED pin says what it still needs).
function project.step_text(job, names, carried)
    if job.step == project.ACCEPTED then
        local text = "Bring " .. job.need .. " " .. job.ware .. " to " .. names.maker .. " at " .. names.maker_at
            .. " for the " .. job.cargo .. "."
        if carried > 0 then text ..= " You carry " .. carried .. "." end
        return text
    elseif job.step == project.FIRING then
        return project.first(names.maker) .. " is firing the " .. job.cargo .. ". Go about your work and come back to "
            .. names.maker_at .. "."
    elseif job.step == project.READY then
        return "The " .. job.cargo .. " are ready: " .. project.first(names.maker) .. " has them at " .. names.maker_at .. "."
    elseif job.step == project.CARRIED then
        return "Carry " .. project.first(names.giver) .. "'s " .. job.units .. " " .. job.cargo .. " to her at "
            .. names.giver_at .. "."
    end
    return nil
end

-- "2 ore, Ines's 4 panes": what the hold carries, the giver's goods marked as hers.
-- `owner` is whose they are ("Ines's"), nil when nothing aboard is anyone else's.
function project.list(hold, cargo, owner)
    if not owner or (hold[cargo] or 0) == 0 then return rules.list(hold) end
    local own = table.clone(hold)
    own[cargo] = 0
    local mine = rules.list(own)
    local theirs = owner .. " " .. hold[cargo] .. " " .. cargo
    return mine == "nothing" and theirs or mine .. ", " .. theirs
end

-- World reads -----------------------------------------------------------------------

-- The project and its fields, or nil (a save from before it has none).
function project.find(ctx)
    for _, id in ctx.query("project") do return id, ctx.get(id, "project") end
    return nil, nil
end

local function name(ctx, keeper)
    return ctx.get(keeper, "keeper").name
end

-- The station a keeper works at, wherever they stand now: their facility's station.
local function home(ctx, keeper)
    for _, id in ctx.query("facility") do
        local facility = ctx.get(id, "facility")
        if facility.keeper == keeper then return facility.station end
    end
    return places.of(ctx, keeper)
end

local function port_name(ctx, station)
    return station and ctx.get(station, "port").name or ""
end

-- Whose goods the hold carries, as "Ines's", and which ware: nil unless the giver's
-- cargo is aboard for her.
function project.owner(ctx)
    local _, job = project.find(ctx)
    if not job or job.step ~= project.CARRIED then return nil, nil end
    return project.first(name(ctx, job.giver)) .. "'s", job.cargo
end

-- What the hold carries, in words, with the giver's goods marked as hers.
function project.carried(ctx, hold)
    local owner, cargo = project.owner(ctx)
    return project.list(hold, cargo or "", owner)
end

-- The giver is ready to ask once her gift is answered (or she has none).
local function asked(ctx, job)
    if job.after == "" then return true end
    local gift = ctx.get(job.after, "gift")
    return gift == nil or gift.answered
end

-- Whether the giver's ask has been said to the player.
local function heard(ctx, job_id)
    for _, id in ctx.query("offer") do
        local offer = ctx.get(id, "offer")
        if offer.project == job_id and offer.step == project.ASKED and offer.heard then return true end
    end
    return false
end

-- A keeper's everyday line while the project needs something said, or nil for their
-- usual one. `hold` is after the caller's writes.
function project.line(ctx, keeper, hold, station)
    local job_id, job = project.find(ctx)
    if not job then return nil end
    local lasting = happen.line(ctx, keeper, station)
    if lasting then return lasting end
    -- The job after this one (salvage.luau) has its own lines once it is under way.
    local next_job = salvage.line(ctx, keeper, station)
    if next_job then return next_job end
    local step = job.step
    local room = fit.hold(ctx) - rules.load(hold)
    if keeper == job.maker then
        if step == project.ACCEPTED then
            return "Ines's panes? I need " .. job.need .. " " .. job.ware .. " for them."
        elseif step == project.FIRING then
            return "Her panes are in the kiln, cooling slow."
        elseif step == project.READY and room < job.units then
            return "Make room for " .. job.units .. " " .. job.cargo .. " and they're yours."
        end
    elseif keeper == job.giver then
        if step == project.ACCEPTED then
            return "Glassite grows out north-east, past the ore."
        elseif step == project.FIRING or step == project.READY then
            return "I can almost taste the greens."
        elseif step == project.BUILDING then
            return "The beds will drink ice; Tova takes it."
        end
    elseif keeper == job.planner then
        if step == project.ASKED and asked(ctx, job) then
            -- Heard her out already ("Not now"): he doesn't send the player to ask.
            if heard(ctx, job_id) then return "Ines told you her plan? There's room on the deck for it." end
            return "Ines has plans for the galley. Ask her."
        elseif step >= project.ACCEPTED and step <= project.CARRIED then
            return "I've staked out the greenhouse plot."
        end
    end
    return nil
end

-- What a keeper met only now says first, before their introduction (history.fold): the
-- maker of the giver's goods and the planner who staked its plot have news of the player's
-- part in it. Nil when the project is no news to them.
function project.news(ctx, keeper)
    local _, job = project.find(ctx)
    if not job or job.step <= project.ACCEPTED then return nil end
    local giver = project.first(name(ctx, job.giver))
    if keeper == job.maker then
        if job.step == project.FIRING then return giver .. "'s " .. job.cargo .. " are in the kiln, from your " .. job.ware .. "." end
        if job.step == project.READY then return "Your " .. job.ware .. " fired clear." end
        if job.step == project.CARRIED then return "Mind " .. giver .. "'s " .. job.cargo .. ". Glass doesn't bounce." end
        if job.step < project.DONE then return "Did " .. giver .. "'s " .. job.cargo .. " travel well? I fired them thick." end
        return "My glass is in " .. giver .. "'s greenhouse, from your " .. job.ware .. "."
    elseif keeper == job.planner then
        if job.step < project.DONE then return "I've staked out " .. giver .. "'s greenhouse plot for you." end
        return "Your greenhouse is the pride of our deck."
    end
    return nil
end

-- A keeper's line in place of their idle one, remembering the player's part in the
-- project once their own part is over: the maker of its goods, and whoever bought the ware
-- that finished it (the exchange of its station). Nil for nothing to say.
function project.remembers(ctx, keeper, station)
    local _, job = project.find(ctx)
    if not job then return nil end
    local giver = project.first(name(ctx, job.giver))
    if keeper == job.maker and job.step >= project.CARRIED then
        if job.step == project.CARRIED then return "Mind " .. giver .. "'s " .. job.cargo .. ". Glass doesn't bounce." end
        if job.step < project.DONE then return "Did " .. giver .. "'s " .. job.cargo .. " travel well? I fired them thick." end
        return "My glass is in " .. giver .. "'s greenhouse. Go and look."
    end
    local build = ctx.get(job.site, "build")
    if job.step == project.DONE and build and build.station == station then
        local exchange = ctx.get(station, "port").exchange
        local facility = exchange and ctx.get(exchange, "facility")
        if facility and facility.keeper == keeper then
            return "Your " .. build.ware .. " filled " .. project.first(name(ctx, build.keeper)) .. "'s beds. I still take it."
        end
    end
    return nil
end

-- The giver's gift was answered (`taken` or declined): her ask after it begins by
-- remembering which, before what it is about.
function project.gifted(ctx, gift, taken)
    for _, id in ctx.query("offer") do
        local offer = ctx.get(id, "offer")
        local job = ctx.get(offer.project, "project")
        if job and job.after == gift and offer.step == project.ASKED then
            local first = ctx.get(id, "engine.talk").first
            ctx.patch(first, "engine.talk_line", { text = taken and "That ration was plain, I know. The crews are tired of them."
                or "You turned my ration down. The crews are tired of them too." })
        end
    end
end

-- Writes -----------------------------------------------------------------------------

-- Turns each of the project's offers on or off at `station`: on while the project is at
-- its step, what it needs holds and its speaker stands there, off after "Not now" until
-- the next docking. A speaker's everyday talk (`instead`) rests while one of theirs is
-- on. A talk being heard is never cut short: voices.retire comes back for it. `hold`
-- and `job` are after the caller's writes (job defaults to the world's).
function project.offers(ctx, station, hold, job)
    local job_id, now = project.find(ctx)
    job = job or now
    if not job then return end
    local room = fit.hold(ctx) - rules.load(hold)
    local ready = asked(ctx, job)
    local on_for, instead_of = {}, {}
    for _, id in ctx.query("offer") do
        local offer = ctx.get(id, "offer")
        if offer.project ~= job_id then continue end -- another job's (salvage.offers)
        local said = ctx.get(id, "engine.talk")
        local on = not offer.later and places.of(ctx, said.speaker) == station
            and project.wanted(job, offer.step, hold, room, ready)
        instead_of[said.speaker] = offer.instead
        if on then on_for[said.speaker] = true end
        if said.enabled ~= on and not said.active then ctx.patch(id, "engine.talk", { enabled = on }) end
    end
    for speaker, instead in instead_of do
        local said = ctx.get(instead, "engine.talk")
        local want = not on_for[speaker]
        if said and said.enabled ~= want and not said.active then
            -- A keeper's unanswered gift keeps their everyday talk resting (voices.gifts).
            local gift = false
            for _, g in ctx.query("gift") do
                local held = ctx.get(g, "gift")
                if held.instead == instead and not held.answered and ctx.get(g, "engine.talk").enabled then gift = true end
            end
            if not (want and gift) then ctx.patch(instead, "engine.talk", { enabled = want }) end
        end
    end
end

-- The glaziers working the site: on the deck by the scaffold while it is built, the first
-- carrying panes from the crates to the frame and back (station.luau walks it).
local GLAZIERS = {
    { id = "glazier1", spot = "core.lantern_glaziers_a", angle = 0.6, a = "core.lantern_glaziers_a", b = "core.lantern_glaziers_b" },
    { id = "glazier2", spot = "core.lantern_glaziers_c", angle = 2.4 },
}

local function glaziers(ctx, site, on)
    local station = ctx.get(site, "facility").station
    for _, g in GLAZIERS do
        if on then
            ctx.put(g.id, "engine.being", {})
            -- A spot placed from a script doesn't move the being onto it: stand them there.
            local spot = ctx.get(g.spot, "engine.spot")
            ctx.put(g.id, "engine.location", { place = station, spot = g.spot })
            ctx.put(g.id, "engine.position", { x = spot.x, y = spot.y, angle = g.angle })
            ctx.put(g.id, "engine.model", { model = "core.person_glazier", look = "core.prop", scale = 1.2 })
            if g.a then ctx.put(g.id, "skiff", { a = g.a, b = g.b }) end
        elseif ctx.has(g.id, "engine.being") then
            ctx.remove(g.id)
        end
    end
end

-- The player answered one of the project's offers. Taken, the project moves on: what
-- changes hands does so here, with its receipt (returned, for the conversation's log
-- entry), and the hold after it. Any other answer leaves the offer for a later docking.
-- `open` is whether the talk goes on after the answer. Nil when the talk is no
-- project's or the answer is stale.
function project.chosen(ctx, talk, choice, open, hold)
    local offer = ctx.get(talk, "offer")
    if not offer then return nil, hold end
    local id, job = project.find(ctx)
    if not job or offer.project ~= id or job.step ~= offer.step then return nil, hold end
    local function put_away()
        if open then return end -- voices.retire, once its last line is heard
        ctx.patch(talk, "engine.talk", { enabled = false })
        ctx.patch(offer.instead, "engine.talk", { enabled = true })
    end
    if choice ~= offer.take then
        ctx.patch(talk, "offer", { later = true, heard = true })
        put_away()
        return nil, hold
    end
    local giver, maker = name(ctx, job.giver), name(ctx, job.maker)
    local gave, maker_at = project.first(giver) .. "'s", port_name(ctx, home(ctx, job.maker))
    local receipt
    local step = job.step
    if step == project.ASKED then
        -- The dockmaster stakes the plot: it is on the station from now on, and on its key.
        ctx.patch(job.site, "facility", { hidden = false })
        ctx.put(job.site, "engine.model", { model = job.plot, look = "core.station" })
        receipt = "Took on " .. gave .. " greenhouse: bring " .. job.need .. " " .. job.ware .. " to " .. maker
            .. " at " .. maker_at .. " for its " .. job.cargo .. "."
    elseif step == project.ACCEPTED then
        if (hold[job.ware] or 0) < job.need then return nil, hold end
        hold[job.ware] -= job.need
        local station = home(ctx, job.maker)
        ctx.patch(id, "project", { fired = ctx.get(station, "port").visits })
        places.restate(ctx, job.maker_act, "firing " .. gave .. " " .. job.cargo)
        places.go(ctx, job.maker, job.maker_act) -- the room shows it: she takes it to the kiln
        receipt = "Gave " .. job.need .. " " .. job.ware .. " for " .. gave .. " " .. job.cargo .. ": " .. maker .. " fires them."
    elseif step == project.READY then
        if fit.hold(ctx) - rules.load(hold) < job.units then return nil, hold end
        hold[job.cargo] = (hold[job.cargo] or 0) + job.units
        places.restate(ctx, job.maker_act, job.maker_state)
        receipt = "Loaded " .. gave .. " " .. job.units .. " " .. job.cargo .. " for " .. port_name(ctx, home(ctx, job.giver))
            .. ": they stay hers until she has them."
    elseif step == project.CARRIED then
        if (hold[job.cargo] or 0) < job.units then return nil, hold end
        hold[job.cargo] -= job.units
        local build = ctx.get(job.site, "build")
        ctx.patch(job.site, "build", { waiting = false })
        ctx.patch(job.site, "engine.model", { model = build.site_model })
        glaziers(ctx, job.site, true)
        receipt = "Delivered " .. gave .. " " .. job.units .. " " .. job.cargo .. " to her. The glaziers start on the "
            .. string.lower(build.name) .. "."
    else
        return nil, hold
    end
    ctx.patch(id, "project", { step = step + 1 })
    ctx.patch(talk, "offer", { heard = true, later = false })
    put_away()
    ctx.put("ship", "hold", hold)
    ctx.record("confirmed", {})
    return receipt, hold
end

-- The ship docked at `station` (`counted` when it is a visit, its port's `visits` after
-- it): offers set aside with "Not now" come back, the maker's goods are ready on her
-- station's next visit, and a happening there counts the visit (a quick launch and
-- redock doesn't use it up). Returns a line for the docking's log entry, or nil.
function project.arrived(ctx, station, counted, visits)
    for _, id in ctx.query("offer") do
        if ctx.get(id, "offer").later then ctx.patch(id, "offer", { later = false }) end
    end
    local id, job = project.find(ctx)
    if job and counted and station == home(ctx, job.maker) and project.fired(job, visits) then
        ctx.patch(id, "project", { step = project.READY })
        places.restate(ctx, job.maker_act, "packing " .. project.first(name(ctx, job.giver)) .. "'s " .. job.cargo)
    end
    return happen.arrived(ctx, station, counted)
end

-- The project's build finished (the sale that completed it): the glaziers leave, the
-- station is seen from afar with its greenhouse, the giver's thanks go on offer, and
-- the station holds its lamp night, its lamp strings hung on the deck. Returns the
-- happening's words for the sale's log entry, or nil.
function project.grown(ctx, site)
    local id, job = project.find(ctx)
    if not job or job.site ~= site then return nil end
    local note = nil
    ctx.patch(id, "project", { step = project.DONE })
    glaziers(ctx, site, false)
    if job.exterior ~= "" then ctx.patch(job.exterior, "engine.model", { model = job.exterior_model }) end
    if job.thanks ~= "" then ctx.patch(job.thanks, "engine.talk", { enabled = true }) end
    ctx.patch(job.giver, "keeper", { idle = job.giver_idle })
    for _, h in ctx.query("happening") do
        if ctx.get(h, "happening").site == site then note = happen.begin(ctx, h) end
    end
    -- The same night the barge that hauls its ice goes dark out in the drift (salvage.luau).
    salvage.adrift(ctx)
    return note
end

-- What the log pins for the project: its offer once heard and not yet answered, and the
-- next step while it is under way. Each is { kind, who, where, text }.
function project.pins(ctx, hold)
    local pins = {}
    local _, job = project.find(ctx)
    if not job then return pins end
    local giver = name(ctx, job.giver)
    local giver_at = port_name(ctx, home(ctx, job.giver))
    if job.step == project.ASKED then
        for _, id in ctx.query("offer") do
            local offer = ctx.get(id, "offer")
            if offer.step == project.ASKED and offer.heard then
                table.insert(pins, { kind = "OFFER", who = giver, where = giver_at,
                    text = "A greenhouse for " .. giver_at .. ". Answer Accept or Not now." })
            end
        end
        return pins
    end
    local text = project.step_text(job, { giver = giver, giver_at = giver_at, maker = name(ctx, job.maker),
        maker_at = port_name(ctx, home(ctx, job.maker)) }, hold[job.ware] or 0)
    if text then table.insert(pins, { kind = "JOB", who = giver, where = giver_at, text = text }) end
    for _, p in salvage.pins(ctx) do table.insert(pins, p) end
    return pins
end

-- The handover a room's panel offers for the talk `talk` (its gold button, beside the
-- talk), or nil: what the player came to give or take, said plainly. A greenhouse step,
-- or the next job's.
function project.verb(ctx, talk)
    local offer = ctx.get(talk, "offer")
    if not offer then return nil end
    local id, job = project.find(ctx)
    if not job or offer.project ~= id then return salvage.verb(ctx, talk) end
    if job.step ~= offer.step then return nil end
    local giver, maker = project.first(name(ctx, job.giver)), project.first(name(ctx, job.maker))
    if offer.step == project.ACCEPTED then
        return "Hand " .. maker .. " " .. job.need .. " " .. job.ware .. " for " .. giver .. "'s " .. job.cargo
    elseif offer.step == project.READY then
        return "Load " .. giver .. "'s " .. job.units .. " " .. job.cargo
    elseif offer.step == project.CARRIED then
        return "Hand " .. giver .. " her " .. job.units .. " " .. job.cargo
    end
    return nil
end

return project
