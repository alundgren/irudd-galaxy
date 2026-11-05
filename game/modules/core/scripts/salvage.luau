-- A barge gone adrift, and the player who brings her in (direction doc 3.3 and 4). The
-- night the greenhouse grows, the barge that hauls the station's ice goes dark out in the
-- drift; its grower asks the player to find her. Steps (`step` on `salvage`):
--
--   ASKED    the barge lies dark where it froze (`x`, `y`), a flare going up now and then;
--            the giver (Noor) asks once her thanks for the greenhouse is heard.
--   SEARCH   accepted. Fly alongside the barge, slow, and pass her a line (Space, or a
--            click on her): she follows on it, the ship slowed and turning wide. A hard
--            knock of the barge on a rock or a hull slips the line, and she drifts where it
--            let go; 0 casts it off. The jets can't berth a tow and the rig can't pass a
--            line: this stretch is flown by hand.
--   MOORED   towed into a station's berth, she lies off it (`at`). That station's
--            dockmaster offers to take her (its `haven`); "Not now", or a line passed again
--            outside, and she can be towed on to the other.
--   NEEDS    the choice is made (`haven`): kept for good at Lantern Dock, or mended at
--            Cinder Kiln. Its work waits on a delivery to its keeper (`need` of `ware`):
--            plate for her pipes, ore for her engine. The barge lies at her mooring.
--   WORKING  handed over, the work is under way: ready on that station's second counted
--            visit after (WORK), while the player goes about their trade.
--   DONE     kept: the greenhouse blooms, the skipper lives aboard the station and it holds
--            a supper. Mended: she sails again once her skipper has thanked you (THANKED).
--
-- No log here: log.luau needs project.luau, which needs this. Callers log what these
-- functions return. Plain rules (no ctx) come first, so tests can call them.
local rules = require("./rules")
local fit = require("./fit")
local places = require("./places")
local happen = require("./happen")

local salvage = {
    QUIET = 0, ASKED = 1, SEARCH = 2, MOORED = 3, NEEDS = 4, WORKING = 5, DONE = 6, THANKED = 7,
    REACH = 8, -- a line reaches this far from the barge's centre: about five units off her hull
    SLOW = 8, -- passing a line wants the ship this slow, units a second
    SPEED = 11, -- the ship's top speed with a barge in tow
    TURN = 120, -- and its turn rate, degrees a second
    BOOST = 1.0, -- the tow won't take a boost
    LINE = 3, -- the barge holds this far off the ship's outline
    STRUCK = 2.5, -- a knock this hard on something fixed slips the line
    FEE = 60, -- what the skipper pays for the tow, once mended
    -- Her wake, as start.toml gives her: off while she lies dark, back once she is whole.
    WAKE = { recipe = "core.wake", x = -2.7, by = "speed" },
    WORK = 2, -- counted visits to the station the work takes, after its delivery
    FLARE = 4, -- seconds between her flares while she lies dark
}

local function first(name)
    return string.match(name, "^%S+") or name
end

-- Whether a line can be passed now: the ship within reach of the barge and slow enough.
function salvage.can_pass(distance, speed)
    return distance <= salvage.REACH and speed <= salvage.SLOW
end

-- Whether the barge takes a line in this step: lying dark after the search is taken on,
-- or lying off a berth, not yet handed over.
function salvage.hitchable(job)
    return not job.towing and (job.step == salvage.SEARCH or job.step == salvage.MOORED)
end

-- Whether a knock slips the line: the barge struck something fixed, hard enough.
function salvage.slips(fixed, speed)
    return fixed and speed >= salvage.STRUCK
end

-- Whether an offer made at `step` is on. `asked` is whether the giver is ready to ask;
-- `haven` the station whose offer it is (nil for any other offer); `has` whether the hold
-- carries what that station's work needs.
function salvage.wanted(job, step, asked, haven, has, kind)
    if job.step ~= step then return false end
    if step == salvage.ASKED then return asked end
    if step == salvage.MOORED then return haven ~= nil and job.at == haven and not job.towing end
    if step == salvage.NEEDS then return haven ~= nil and job.at == haven and has == true end
    -- Only a mended barge's skipper waits to pay for the tow; a kept barge stays.
    if step == salvage.DONE then return kind == "mend" end
    return false
end

-- What the log pins while the job is under way, in the giver's terms. `names` holds the
-- barge's, the giver's and the skipper's names, and for the station she lies off or was
-- left at, `at`, its `keeper`; for the chosen haven, `kind`, `haven_at`, what its work
-- needs (`need` of `ware`), how much of it the hold carries (`carry`) and, at work, how
-- many trips out are left (`trips`).
function salvage.step_text(job, names)
    if job.step == salvage.SEARCH then
        if job.towing then
            return "Tow " .. names.barge .. " in by hand: Lantern Dock keeps her tanks for " .. first(names.giver)
                .. "'s beds; Cinder Kiln mends her for " .. first(names.skipper) .. "."
        end
        return "Find " .. names.barge .. ", dark out west past the ice, and pass her a line."
            .. (job.slips > 0 and " She lies where the line slipped." or "")
    elseif job.step == salvage.MOORED then
        return names.barge .. " lies off " .. names.at .. ". See " .. first(names.keeper) .. " in the office, or tow her on."
    elseif job.step == salvage.NEEDS then
        local what = names.kind == "keep" and " for her pipes." or " for her engine."
        return "Bring " .. names.need .. " " .. names.ware .. " to " .. first(names.keeper) .. " at " .. names.haven_at
            .. what .. " You carry " .. names.carry .. "."
    elseif job.step == salvage.WORKING then
        if names.kind == "keep" then
            return "Her tanks are being piped to the beds. Come back to " .. names.haven_at .. " after " .. salvage.trips(names.trips) .. "."
        end
        return "Her engine is being mended. Come back to " .. names.haven_at .. " after " .. salvage.trips(names.trips) .. "."
    end
    return nil
end

-- How many trips out are left, in words: "your next trip out", "two more trips out".
function salvage.trips(n)
    if n <= 1 then return "your next trip out" end
    return (n == 2 and "two" or tostring(n)) .. " more trips out"
end

-- Counted visits to the haven's station still to go before the work is done.
local function trips_left(job, visits)
    return math.max(1, job.since + salvage.WORK - visits)
end

-- World reads -----------------------------------------------------------------------

-- The job and its fields, or nil (a save from before it has none).
function salvage.find(ctx)
    for _, id in ctx.query("salvage") do return id, ctx.get(id, "salvage") end
    return nil, nil
end

-- The haven at `station` for job `job_id`: its ID and fields, or nil.
function salvage.haven_at(ctx, job_id, station)
    for _, id in ctx.query("haven") do
        local haven = ctx.get(id, "haven")
        if haven.job == job_id and haven.station == station then return id, haven end
    end
    return nil, nil
end

-- The haven whose offer to take the barge, or to take its work's delivery, is the talk
-- `talk`, or nil.
local function haven_of(ctx, job_id, talk)
    for _, id in ctx.query("haven") do
        local haven = ctx.get(id, "haven")
        if haven.job == job_id and (haven.offer == talk or haven.deliver == talk) then return id, haven end
    end
    return nil, nil
end

-- Whether the hold carries what the haven's work needs.
local function carries(ctx, haven)
    return haven.need <= 0 or (ctx.get("ship", "hold")[haven.ware] or 0) >= haven.need
end

local function name(ctx, keeper)
    return ctx.get(keeper, "keeper").name
end

local function port_name(ctx, station)
    return (station and station ~= "" and ctx.get(station, "port")) and ctx.get(station, "port").name or ""
end

-- Whether the barge is in tow now.
function salvage.towing(ctx)
    local _, job = salvage.find(ctx)
    return job ~= nil and job.towing
end

-- The barge, if a line could be passed to her from where the ship is now, and her
-- distance; nil and the distance otherwise.
function salvage.near(ctx)
    local _, job = salvage.find(ctx)
    if not job or not salvage.hitchable(job) then return nil, math.huge end
    if ctx.get("ship", "engine.location").place ~= "core.reach" then return nil, math.huge end
    local at = ctx.get("ship", "engine.position")
    local barge = ctx.get(job.barge, "engine.position")
    if not barge then return nil, math.huge end
    return job.barge, math.sqrt(rules.distance2(at, barge))
end

-- The giver is ready to ask once her thanks for the greenhouse are heard.
local function asked(ctx, job)
    if job.after == "" then return true end
    local thanks = ctx.get(job.after, "engine.talk")
    return thanks == nil or thanks.seen
end

-- Writes -----------------------------------------------------------------------------

-- The ship's flight as its fit makes it, for when the line comes off.
function salvage.flight(ctx)
    local parts = {}
    for _, id in ctx.query("part") do
        local p = ctx.get(id, "part")
        p.id = id
        table.insert(parts, p)
    end
    local t = fit.totals(parts, nil, nil, fit.hull_speed(ctx))
    ctx.patch("ship", "engine.flight", { speed = t.top, turn = t.turn, boost = 2.0 })
end

-- An act of someone's day says something new; whoever is at that act now shows it on
-- their sign at once, not only from their next act on.
local function restate(ctx, act, state)
    ctx.patch(act, "engine.act", { state = state })
    for _, who in ctx.query("keeper") do
        local life = ctx.get(who, "engine.life")
        if life and life.current == act then ctx.patch(who, "engine.being", { state = state }) end
    end
end

-- The night the greenhouse grows the barge goes dark where it was working: off its route,
-- its lamps out, and the giver watching for it.
function salvage.adrift(ctx)
    local id, job = salvage.find(ctx)
    if not job or job.step ~= salvage.QUIET then return end
    local barge = job.barge
    ctx.patch(id, "salvage", { step = salvage.ASKED })
    ctx.patch(barge, "trader", { stopped = true, moored = false })
    ctx.put(barge, "engine.steer", {})
    ctx.put(barge, "engine.helm", {})
    ctx.put(barge, "engine.motion", {})
    ctx.patch(barge, "engine.position", { x = job.x, y = job.y, angle = 2.2 })
    ctx.patch(barge, "engine.model", { model = "core.barge_adrift" })
    ctx.remove(barge, "engine.emitter")
    restate(ctx, job.giver .. "_act", "watching for " .. ctx.get(barge, "trader").name)
    -- Her room's sign says so too, on the deck and on the station key.
    for _, f in ctx.query("facility") do
        if ctx.get(f, "facility").keeper == job.giver then
            ctx.patch(f, "facility", { offer = "watching for " .. ctx.get(barge, "trader").name })
        end
    end
end

-- Passes the barge a line: she follows the ship on it, and the ship flies slower and turns
-- wide. Returns the message and what the skipper says over the radio, for the log.
function salvage.hitch(ctx)
    local id, job = salvage.find(ctx)
    if not job or not salvage.hitchable(job) then return nil, nil end
    local barge = job.barge
    local trader = ctx.get(barge, "trader")
    ctx.patch(id, "salvage", { towing = true, at = "", step = salvage.SEARCH })
    ctx.patch(barge, "engine.body", { fixed = false })
    ctx.put(barge, "engine.steer", { mode = "hold", target = "core.ship", range = salvage.LINE })
    ctx.patch(barge, "engine.flight", { speed = salvage.SPEED + 2, accel = 9, brake = 9, turn = 90 })
    -- The line runs from her bow to the ship.
    ctx.put(barge, "engine.beam", { target = "core.ship", look = "core.tow", x = 2.6, height = 0.6 })
    ctx.patch("ship", "engine.flight", { speed = salvage.SPEED, turn = salvage.TURN, boost = salvage.BOOST })
    local skipper = name(ctx, job.skipper)
    local giver = first(name(ctx, job.giver))
    local said
    if job.slips > 0 or job.step == salvage.MOORED then
        said = first(skipper) .. " on " .. trader.name .. "'s radio: \u{201C}Line's fast. Easy round the ice this time.\u{201D}"
    else
        said = skipper .. " on " .. trader.name .. "'s radio: \u{201C}A line! Bless you.\u{201D} \u{201C}My engine froze hauling ice for "
            .. giver .. "'s beds.\u{201D} \u{201C}" .. giver .. " wants my tanks at Lantern Dock.\u{201D} \u{201C}I'd sooner Orrin mended me at the Kiln.\u{201D} \u{201C}Your tow, your call.\u{201D}"
    end
    return trader.name .. " is in tow. She swings wide: give the ice and the hulls room.", said
end

-- The line comes off: the barge drifts to a stop where she is, and the ship flies as its
-- fit makes it again.
local function release(ctx, id, job)
    ctx.patch(id, "salvage", { towing = false })
    ctx.put(job.barge, "engine.steer", {})
    ctx.put(job.barge, "engine.helm", {})
    ctx.put(job.barge, "engine.beam", {})
    salvage.flight(ctx)
end

-- The barge struck something fixed: the line slips. Returns the message.
function salvage.slip(ctx, struck)
    local id, job = salvage.find(ctx)
    if not job or not job.towing then return nil end
    release(ctx, id, job)
    ctx.patch(id, "salvage", { slips = job.slips + 1 })
    local barge = ctx.get(job.barge, "trader").name
    local what = "a hull"
    local ore = ctx.get(struck, "ore")
    if ore then
        what = ore.ware == "ice" and "the ice" or "a rock"
    end
    return barge .. " struck " .. what .. " and the line slipped. Come alongside, slow, and pass it again."
end

-- 0 casts the line off. Returns the message.
function salvage.cast_off(ctx)
    local id, job = salvage.find(ctx)
    if not job or not job.towing then return nil end
    release(ctx, id, job)
    return "You cast off " .. ctx.get(job.barge, "trader").name .. "'s line. She drifts where she is."
end

-- The ship docked at `station`: a barge in tow is brought in and lies off the berth.
-- Returns a line for the docking's log entry, or nil.
function salvage.docked(ctx, station)
    local id, job = salvage.find(ctx)
    if not job or not job.towing then return nil end
    local _, haven = salvage.haven_at(ctx, id, station)
    release(ctx, id, job)
    local barge = ctx.get(job.barge, "trader").name
    if not haven then
        return barge .. " drifts off the berth: nobody here can take her."
    end
    ctx.patch(id, "salvage", { step = salvage.MOORED, at = station })
    ctx.put(job.barge, "engine.steer", { mode = "dock", target = haven.tow })
    return "You towed " .. barge .. " in: she lies off the berth. " .. first(name(ctx, haven.keeper)) .. " will see to her."
end

-- Turns the job's offers on or off at `station`, as project.offers does for its own: on
-- while the job is at its step, what it needs holds and its speaker stands there, off
-- after "Not now" until the next docking. A speaker's everyday talk rests meanwhile.
function salvage.offers(ctx, station)
    local id, job = salvage.find(ctx)
    if not job then return end
    local ready = asked(ctx, job)
    local on_for, instead_of = {}, {}
    for _, o in ctx.query("offer") do
        local offer = ctx.get(o, "offer")
        if offer.project == id then
            local said = ctx.get(o, "engine.talk")
            local _, haven = haven_of(ctx, id, o)
            local chosen = job.haven ~= "" and ctx.get(job.haven, "haven") or nil
            local on = not offer.later and places.of(ctx, said.speaker) == station
                and salvage.wanted(job, offer.step, ready, haven and haven.station or nil, haven ~= nil and carries(ctx, haven),
                    chosen and chosen.kind or nil)
            instead_of[said.speaker] = offer.instead
            if on then on_for[said.speaker] = true end
            if said.enabled ~= on and not said.active then ctx.patch(o, "engine.talk", { enabled = on }) end
        end
    end
    for speaker, instead in instead_of do
        local said = ctx.get(instead, "engine.talk")
        local want = not on_for[speaker]
        if said and said.enabled ~= want and not said.active then ctx.patch(instead, "engine.talk", { enabled = want }) end
    end
end

-- The act of the day from `first` that is somewhere else than `who` is now, and how many
-- acts on it is: the skipper and the giver share the one clock, so starting her there
-- keeps them a room apart, and one room's signs uncrowded.
local function apart(ctx, first, who)
    local at = ctx.get(who, "engine.location")
    local act, n = first, 0
    while act ~= "" and n < 16 do
        local plan = ctx.get(act, "engine.act")
        if not at or plan.place ~= at.place then return act, n end
        act, n = plan.next or "", n + 1
    end
    return first, 0
end

-- The skipper comes aboard the station: she stands at her first act there, starting the
-- day where the shared clock has it, a room apart from the giver.
local function aboard(ctx, job, haven)
    local now, on = apart(ctx, haven.life, job.giver)
    local start = places.first_act(ctx, haven.life, -on)
    local plan = ctx.get(now, "engine.act")
    ctx.put(job.skipper, "engine.being", {})
    ctx.put(job.skipper, "engine.location", { place = plan.place, spot = plan.spot })
    ctx.put(job.skipper, "engine.position", { angle = plan.angle })
    ctx.put(job.skipper, "engine.model", { model = ctx.get(job.skipper, "keeper").figure, look = "core.prop" })
    ctx.put(job.skipper, "engine.life", { clock = "core.day", first = start, speed = 1.6, presentation = "instant", name = "" })
end

-- The station's dockmaster takes the barge: she goes to her mooring there, her skipper
-- aboard the station meanwhile, and the work on her waits on its delivery.
local function take(ctx, id, job, hid, haven)
    local barge = job.barge
    ctx.patch(id, "salvage", { step = salvage.NEEDS, haven = hid, at = haven.station })
    local moor = ctx.get(haven.moor, "engine.position")
    ctx.put(barge, "engine.steer", {})
    ctx.put(barge, "engine.helm", {})
    ctx.put(barge, "engine.motion", {})
    ctx.patch(barge, "engine.position", { x = moor.x, y = moor.y, angle = moor.angle or 0 })
    ctx.patch(barge, "engine.body", { fixed = true })
    aboard(ctx, job, haven)
    local trader = ctx.get(barge, "trader").name
    local keeper = first(name(ctx, haven.keeper))
    local need = haven.need .. " " .. haven.ware
    if haven.kind == "keep" then
        ctx.patch(barge, "trader", { about = "moored off " .. port_name(ctx, haven.station) .. " for good, waiting on her pipes" })
        return keeper .. " moors " .. trader .. " off " .. port_name(ctx, haven.station) .. " for good. To pipe her tanks to "
            .. first(name(ctx, job.giver)) .. "'s beds, his crew needs " .. need .. "."
    end
    ctx.patch(barge, "trader", { about = "moored off " .. port_name(ctx, haven.station) .. ", waiting on her engine" })
    return trader .. " goes to Orrin's quay at " .. port_name(ctx, haven.station) .. ". To mend her engine he needs "
        .. need .. ": " .. keeper .. " takes it for him."
end

-- The keeper takes the work's delivery out of the hold, and the work begins: her pipes
-- laid, or sparks off her engine while she is mended. Returns the receipt, or nil when the
-- hold is short.
local function deliver(ctx, id, job, haven)
    local hold = ctx.get("ship", "hold")
    if (hold[haven.ware] or 0) < haven.need then return nil end
    ctx.patch("ship", "hold", { [haven.ware] = hold[haven.ware] - haven.need })
    ctx.patch(id, "salvage", { step = salvage.WORKING, since = ctx.get(haven.station, "port").visits })
    restate(ctx, haven.act, haven.work)
    local trader = ctx.get(job.barge, "trader").name
    local gave = "Gave " .. first(name(ctx, haven.keeper)) .. " " .. haven.need .. " " .. haven.ware
    -- Sparks off her while the work goes on: her pipes welded, or her engine mended.
    ctx.put(job.barge, "engine.emitter", { recipe = "core.weld", x = -0.4, height = 1.0, rate = 1.0 })
    if haven.kind == "keep" then
        ctx.patch(job.barge, "trader", { about = "moored off " .. port_name(ctx, haven.station) .. " for good: her tanks water the greenhouse beds" })
        return gave .. ": the crew pipes " .. trader .. "'s tanks to the beds."
    end
    ctx.patch(job.barge, "trader", { about = "moored off " .. port_name(ctx, haven.station) .. ", her engine being mended" })
    return gave .. " for Orrin: he starts on " .. trader .. "'s engine."
end

-- The player answered one of the job's offers. Taken, the job moves on, with its receipt
-- (returned, for the conversation's log entry). Any other answer sets the offer aside
-- until a later docking; the skipper's fee, declined, is simply over. Nil when the talk
-- is not this job's or the answer is stale. `open` is whether the talk goes on.
function salvage.chosen(ctx, talk, choice, open)
    local offer = ctx.get(talk, "offer")
    if not offer then return nil end
    local id, job = salvage.find(ctx)
    if not job or offer.project ~= id or job.step ~= offer.step then return nil end
    local function put_away()
        if open then return end -- voices.retire, once its last line is heard
        ctx.patch(talk, "engine.talk", { enabled = false })
        ctx.patch(offer.instead, "engine.talk", { enabled = true })
    end
    if choice ~= offer.take then
        if offer.step == salvage.DONE then
            ctx.patch(id, "salvage", { step = salvage.THANKED })
            ctx.patch(talk, "offer", { heard = true })
        else
            ctx.patch(talk, "offer", { later = true, heard = true })
        end
        put_away()
        return nil
    end
    local receipt
    if offer.step == salvage.ASKED then
        ctx.patch(id, "salvage", { step = salvage.SEARCH })
        receipt = "Took on " .. first(name(ctx, job.giver)) .. "'s search: find " .. ctx.get(job.barge, "trader").name
            .. ", dark out west past the ice, and tow her in."
    elseif offer.step == salvage.MOORED then
        local hid, haven = haven_of(ctx, id, talk)
        if not haven or haven.offer ~= talk or job.at ~= haven.station or job.towing then return nil end
        receipt = take(ctx, id, job, hid, haven)
    elseif offer.step == salvage.NEEDS then
        local hid, haven = haven_of(ctx, id, talk)
        if not haven or haven.deliver ~= talk or job.haven ~= hid then return nil end
        receipt = deliver(ctx, id, job, haven)
        if not receipt then return nil end
    elseif offer.step == salvage.DONE then
        local pilot = ctx.get("ship", "pilot")
        ctx.patch("ship", "pilot", { credits = pilot.credits + salvage.FEE })
        ctx.patch(id, "salvage", { step = salvage.THANKED })
        receipt = name(ctx, job.skipper) .. " paid you " .. salvage.FEE .. " cr for the tow."
    else
        return nil
    end
    ctx.patch(talk, "offer", { heard = true, later = false })
    put_away()
    ctx.record("confirmed", {})
    return receipt
end

-- The work is done (the haven station's WORK-th counted visit after its delivery). Kept: the barge is the
-- station's cistern, the greenhouse blooms, its station buys more ice, the skipper
-- settles in and the giver thanks you; mended: the barge is whole and her skipper waits to
-- thank you. Either way the station holds its happening. Returns the docking's note.
local function finish(ctx, id, job)
    local haven = ctx.get(job.haven, "haven")
    ctx.patch(id, "salvage", { step = salvage.DONE })
    restate(ctx, haven.act, haven.rest)
    restate(ctx, job.giver .. "_act", "tending the beds")
    local trader = ctx.get(job.barge, "trader").name
    for _, f in ctx.query("facility") do
        local facility = ctx.get(f, "facility")
        if facility.keeper == job.giver then
            ctx.patch(f, "facility", { offer = haven.kind == "keep" and "in bloom, watered from " .. trader .. "'s tanks" or "grows greens" })
        end
    end
    restate(ctx, haven.life, haven.settled)
    ctx.patch(job.barge, "engine.model", { model = "core.barge" })
    ctx.put(job.barge, "engine.emitter", salvage.WAKE)
    local note
    if haven.kind == "keep" then
        if haven.site ~= "" then ctx.patch(haven.site, "engine.model", { model = haven.site_model }) end
        if (haven.room or "") ~= "" then ctx.patch(haven.room, "engine.model", { model = haven.room_model }) end
        if haven.site ~= "" and (haven.told or "") ~= "" then ctx.patch(haven.site, "build", { built = haven.told }) end
        if haven.exterior ~= "" then ctx.patch(haven.exterior, "engine.model", { model = haven.exterior_model }) end
        if haven.market ~= "" then ctx.patch(haven.market, "market", { target = haven.target }) end
        if haven.thanks ~= "" then ctx.patch(haven.thanks, "engine.talk", { enabled = true }) end
        note = trader .. "'s tanks water the greenhouse now, and its beds are in bloom."
    else
        ctx.patch(job.barge, "trader", { about = "mended, waiting off " .. port_name(ctx, haven.station) .. " for her skipper" })
        note = "Orrin has mended " .. trader .. ". " .. first(name(ctx, job.skipper)) .. " waits in the office to thank you."
    end
    local cause = haven.happening ~= "" and happen.begin(ctx, haven.happening) or nil
    if cause then note ..= " " .. cause end
    return note
end

-- The ship docked at `station` (`counted` when it is a visit, its port's `visits` after
-- it): the work on the barge is done on the chosen station's WORK-th counted visit after
-- its delivery. Returns the docking's note, or nil.
function salvage.arrived(ctx, station, counted, visits)
    local id, job = salvage.find(ctx)
    if not job or job.step ~= salvage.WORKING or not counted then return nil end
    local haven = ctx.get(job.haven, "haven")
    if haven.station ~= station or visits < job.since + salvage.WORK then return nil end
    return finish(ctx, id, job)
end

-- The ship left `station`: a mended barge whose skipper has thanked you sails with her,
-- back onto her route. Returns a line for the log, or nil.
function salvage.undocked(ctx, station)
    local _, job = salvage.find(ctx)
    if not job or job.step ~= salvage.THANKED or job.haven == "" then return nil end
    local haven = ctx.get(job.haven, "haven")
    if haven.kind ~= "mend" or haven.station ~= station or not ctx.has(job.skipper, "engine.being") then return nil end
    for _, c in { "engine.life", "engine.model", "engine.location", "engine.position", "engine.being" } do
        ctx.remove(job.skipper, c)
    end
    local barge = job.barge
    ctx.patch(barge, "engine.body", { fixed = false })
    ctx.patch(barge, "engine.flight", { speed = 6, accel = 7, brake = 7, turn = 70 })
    ctx.put(barge, "engine.emitter", salvage.WAKE)
    ctx.patch(barge, "trader", { stopped = false, moored = false, leg = "home", about = haven.about })
    ctx.put(barge, "engine.steer", { mode = "follow", target = haven.route })
    local trader = ctx.get(barge, "trader").name
    return trader .. " casts off from " .. port_name(ctx, station) .. " and heads for the ice camp, " .. first(name(ctx, job.skipper)) .. " at her wheel."
end

-- A keeper's everyday line while the job has something to say, or nil. `station` is
-- where they stand.
function salvage.line(ctx, keeper, station)
    local id, job = salvage.find(ctx)
    if not job or job.step == salvage.QUIET then return nil end
    local step = job.step
    local haven = job.haven ~= "" and ctx.get(job.haven, "haven") or nil
    local kind = haven and haven.kind or ""
    local barge = ctx.get(job.barge, "trader").name
    local skipper = first(name(ctx, job.skipper))
    local giver = first(name(ctx, job.giver))
    local here = ctx.get(keeper, "engine.location")
    local visiting = here and ctx.get(keeper .. "_act2", "engine.act") ~= nil
        and here.place == ctx.get(keeper .. "_act2", "engine.act").place
    if keeper == job.giver then
        if visiting then
            -- At the galley with her greens, she talks about the cook and the skipper.
            local cook = job.cook ~= "" and first(name(ctx, job.cook)) or "The cook"
            if step == salvage.ASKED then return cook .. " saved a bowl for " .. barge .. "'s skipper." end
            if step == salvage.SEARCH then return cook .. " saved " .. skipper .. " a bowl. It's gone cold." end
            if step < salvage.DONE then return cook .. " says I'm to eat. I'll eat when she's in." end
            if kind == "keep" then return skipper .. " won't say it, but she likes my greens." end
            return cook .. " sends a pot of greens out with every haul."
        end
        if step == salvage.ASKED then return barge .. " should be in by now." end
        if step == salvage.SEARCH then return "Out west, past the ice. That's where her lamps went." end
        if step == salvage.MOORED then
            local _, keeps = salvage.haven_at(ctx, id, station)
            if job.at == station and keeps then return "She's here! Will " .. first(name(ctx, keeps.keeper)) .. " moor her for my beds?" end
            return "She's at the Kiln? Then " .. skipper .. "'s safe, at least."
        end
        if step == salvage.NEEDS then
            if kind == "keep" then return first(name(ctx, haven.keeper)) .. "'s crew needs " .. haven.ware .. " before they can pipe her." end
            return skipper .. "'s safe. My beds can wait for ice."
        end
        if step == salvage.WORKING then
            if kind == "keep" then return first(name(ctx, haven.keeper)) .. "'s piping her tanks to my beds." end
            return skipper .. "'s safe. My beds can wait for ice."
        end
        if kind == "keep" then return "The beds drink from her tanks now. Look at them!" end
        return skipper .. "'s hauling again. The beds will get their ice."
    elseif keeper == job.skipper then
        if kind == "keep" then
            if step == salvage.NEEDS or step == salvage.WORKING then return "Pipe my tanks, they say. Thirty years I hauled." end
            if visiting then return (job.cook ~= "" and first(name(ctx, job.cook)) or "The cook") .. " feeds me like a crew of three." end
            return giver .. "'s beds drink my tanks dry. Fine by me."
        end
        if step == salvage.NEEDS then return "Orrin wants " .. haven.ware .. " for a new pump. Then she'll purr." end
        if step == salvage.WORKING then return "Orrin says a trip or two and she'll purr." end
        return barge .. "'s whole again. I'm off when you are."
    end
    -- The dockmasters, the cook and the fitter.
    local _, at_haven = salvage.haven_at(ctx, id, station)
    if at_haven and keeper == at_haven.keeper then
        if step == salvage.ASKED or step == salvage.SEARCH then
            if at_haven.kind == "keep" then return barge .. "'s lamps went out past the ice." end
            return barge .. "'s late. The ice drift's thick this shift."
        end
        if step == salvage.MOORED and job.at == station then return barge .. " can lie off our berth while you decide." end
        if haven and haven.station == station then
            if step == salvage.NEEDS then
                if kind == "keep" then return "My crew needs " .. haven.need .. " " .. haven.ware .. " for her pipes. The Kiln sells it." end
                return "Orrin wants " .. haven.need .. " " .. haven.ware .. " for her pump. Leave it with me."
            end
            if step == salvage.WORKING then
                return kind == "keep" and "My crew's piping her tanks to the greenhouse." or "Orrin's under her engine. " .. skipper .. " paces in here."
            end
            if step == salvage.DONE and kind == "mend" then return skipper .. "'s waiting to thank you before she sails." end
            if kind == "keep" then return barge .. "'s our cistern now. " .. skipper .. " checks her every hour." end
        end
    end
    if haven and haven.act == keeper .. "_act2" then
        if step == salvage.NEEDS then return "Bring Mags the " .. haven.ware .. " and I'll cast her a pump." end
        if step == salvage.WORKING then return "Her engine's older than me. Give me two trips." end
    end
    if keeper == job.cook then
        if step == salvage.ASKED then return giver .. "'s off her food. Ask her what's wrong." end
        if step <= salvage.MOORED then return giver .. "'s off her food. Bring " .. skipper .. " home." end
        if kind == "keep" then return skipper .. " eats like a crew of three." end
        if step >= salvage.DONE then return skipper .. " sends ice for my pots. No charge, she says." end
    end
    return nil
end

-- What the log pins for the job: its ask once heard and not yet answered, then the next
-- thing to do. Each is { kind, who, where, text }.
function salvage.pins(ctx)
    local pins = {}
    local id, job = salvage.find(ctx)
    if not job or job.step == salvage.QUIET then return pins end
    local giver = name(ctx, job.giver)
    local where = "Lantern Dock"
    for _, f in ctx.query("facility") do
        local facility = ctx.get(f, "facility")
        if facility.keeper == job.giver then where = port_name(ctx, facility.station) end
    end
    local barge = ctx.get(job.barge, "trader").name
    if job.step == salvage.ASKED then
        for _, o in ctx.query("offer") do
            local offer = ctx.get(o, "offer")
            if offer.project == id and offer.step == salvage.ASKED and offer.heard then
                table.insert(pins, { kind = "OFFER", who = giver, where = where,
                    text = "Find " .. barge .. ", dark out west. Answer Accept or Not now." })
            end
        end
        return pins
    end
    local names = { barge = barge, giver = giver, skipper = name(ctx, job.skipper), at = port_name(ctx, job.at) }
    local _, at = salvage.haven_at(ctx, id, job.at)
    names.keeper = at and name(ctx, at.keeper) or ""
    if job.haven ~= "" then
        local haven = ctx.get(job.haven, "haven")
        names.kind, names.haven_at = haven.kind, port_name(ctx, haven.station)
        names.need, names.ware = haven.need, haven.ware
        names.carry = ctx.get("ship", "hold")[haven.ware] or 0
        names.trips = trips_left(job, ctx.get(haven.station, "port").visits)
    end
    local text = salvage.step_text(job, names)
    if text then table.insert(pins, { kind = "JOB", who = giver, where = where, text = text }) end
    return pins
end

-- The handover a room's panel offers for the talk `talk` (its gold button), or nil: the
-- dockmaster's offer to take the barge in, then the delivery its work needs.
function salvage.verb(ctx, talk)
    local id, job = salvage.find(ctx)
    if not job then return nil end
    local _, haven = haven_of(ctx, id, talk)
    if not haven then return nil end
    if job.step == salvage.NEEDS and haven.deliver == talk then
        local what = haven.kind == "keep" and " for her pipes" or " for Orrin"
        return "Hand " .. first(name(ctx, haven.keeper)) .. " " .. haven.need .. " " .. haven.ware .. what
    end
    if job.step ~= salvage.MOORED or haven.offer ~= talk then return nil end
    -- The press settles her fate, so the button says what it costs, not only who it helps.
    local barge = ctx.get(job.barge, "trader").name
    local skipper = first(name(ctx, job.skipper))
    if haven.kind == "keep" then return "Keep " .. barge .. " here for good: " .. skipper .. " won't sail her again" end
    return "Have " .. barge .. " mended for " .. skipper .. ": she sails on"
end

-- The barge's label suffix and what pointing at her says, while the job has her: nil
-- when she is just a trader on her route.
function salvage.tag(ctx, trader_id)
    local _, job = salvage.find(ctx)
    if not job or job.barge ~= trader_id or not ctx.get(trader_id, "trader").stopped then return nil, nil end
    if job.towing then return " · in tow", "in tow on your line: fly slowly into a berth's ring to bring her in; 0 or a click on her casts off" end
    if job.step <= salvage.SEARCH then
        return " · adrift", "adrift, her engine frozen: fly alongside, slow, and press Space or click her to pass a line"
    end
    return nil, nil
end

-- What a click on the barge does while the job has her, so every tow action has a click
-- as well as a key: the event it sends ("hitch" passes a line, as Space does; "cancel"
-- casts one off, as 0 does) and what pointing at her says. Nil, nil when a click on her
-- does nothing.
function salvage.click(ctx, trader_id)
    local _, job = salvage.find(ctx)
    if not job or job.barge ~= trader_id then return nil, nil end
    local _, about = salvage.tag(ctx, trader_id)
    if job.towing then return "cancel", about end
    if not salvage.hitchable(job) then return nil, nil end
    if job.step == salvage.MOORED then
        return "hitch", "lying off the berth: fly alongside, slow, and press Space or click her to pass the line again"
    end
    return "hitch", about
end

return salvage
