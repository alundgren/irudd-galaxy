-- What each keeper says after their introduction: one short line that follows the
-- shift, so a second click hears something current instead of the same speech again.
local rules = require("./rules")
local trade = require("./trade")
local fit = require("./fit")
local places = require("./places")
local project = require("./project")
local salvage = require("./salvage")
local history = require("./history")

local voices = {}

-- Ore still in the field's ironstone (ice and glassite are other wares).
function voices.field(ctx)
    local left = 0
    for _, id in ctx.query("ore") do
        local rock = ctx.get(id, "ore")
        if rock.ware == "ore" or rock.ware == "" then left += rock.left end
    end
    return left
end

-- The exchange keeper: about what you carry that they buy (`wanted`, as "6 ore"), the
-- sale you just made, or their own idle line. `wanted` and `sold` are the values after
-- the calling handler's own writes, because reads still see the world before them.
function voices.trader(wanted, sold, idle)
    if wanted ~= "" then return "I'll take your " .. wanted .. " whenever you like." end
    if sold > 0 then return "Thanks. It's all on the pile already." end
    return idle
end

-- The keeper of a building still going up: how much is still to come, in their words;
-- their idle line once nothing is.
function voices.waiting(need, total, ware, idle)
    if need <= 0 then return idle end
    if need >= total then return total .. " " .. ware .. ", and I can light the stove." end
    return need .. " " .. ware .. " to go, and the crews eat hot."
end

-- The office keeper: about what the station is building, then about the field. `made` is
-- what the player's deliveries built here, said first ("Your plate built our galley."),
-- or "".
function voices.master(left, need, ware, building, made)
    if need > 0 then return "The " .. building .. " still needs " .. need .. " " .. ware .. "." end
    local lead = (made or "") ~= "" and made .. " " or ""
    if left > 0 then return lead .. "The field still holds " .. left .. " ore." end
    return lead .. "The field's worked out. Good shift."
end

-- The station's own build, finished (the galley; a project's build is the project's to
-- talk about): its fields, or nil.
local function built(ctx, station)
    local _, job = project.find(ctx)
    for _, id in ctx.query("build") do
        local build = ctx.get(id, "build")
        if build.station == station and build.done and not (job and job.site == id) then return build end
    end
    return nil
end

-- "Your plate built our galley.": what the player's deliveries built at `station`, or ""
-- while nothing stands that they built.
function voices.made(ctx, station)
    local build = built(ctx, station)
    return build and "Your " .. build.ware .. " built our " .. string.lower(build.name) .. "." or ""
end

-- What the exchange keeper says in place of their idle line, remembering what the player
-- has sold them for: the barge's bloom, the project's build, the station's first build.
local function remembered(ctx, keeper, station)
    local says = salvage.remembers(ctx, keeper) or project.remembers(ctx, keeper, station)
    if says then return says end
    local build = built(ctx, station)
    if not build then return nil end
    local who = string.match(ctx.get(build.keeper, "keeper").name, "^%S+")
    return "Your " .. build.ware .. " built " .. who .. "'s " .. string.lower(build.name) .. ". I eat there every shift."
end

-- The office keeper's introduction ends on the station's first build. Once it is
-- finished the intro's last line says so, so a first meeting after it never tells of
-- raising it. `finished` names a build the caller has just finished (its reads still see
-- it unfinished); unset, a build already marked done is looked for. Later builds (a
-- project's) leave the intro as it is. Who built it is history, said first (refresh).
function voices.intro_close(ctx, keeper, station, finished)
    local name = finished
    for _, id in ctx.query("build") do
        local build = ctx.get(id, "build")
        if build.station == station and build.done then
            if finished then return end -- not the station's first
            name = build.name
            break
        end
    end
    if not name then return end
    local intro = ctx.get(keeper .. "_intro", "engine.talk")
    local line = intro and intro.first or ""
    while line ~= "" do
        local said = ctx.get(line, "engine.talk_line")
        if not said then return end
        if (said.next or "") == "" then
            ctx.patch(line, "engine.talk_line", { text = "Our " .. string.lower(name) .. "'s open now. Go and eat." })
            return
        end
        line = said.next
    end
end

-- Sets the current line of every keeper at `station`, given the hold, the credits a
-- sale just paid and what the station's build still needs, all after the caller's
-- writes.
function voices.refresh(ctx, station, hold, sold, need)
    local exchange = ctx.get(station, "port").exchange
    local _, build = trade.building(ctx, station)
    local left = voices.field(ctx)
    for _, id in ctx.query("facility") do
        local facility = ctx.get(id, "facility")
        if facility.station == station and facility.keeper ~= "" then
            local who = facility.keeper
            local keeper = ctx.get(who, "keeper")
            -- What they have to say about the player's doings (`memory`): the jobs' news,
            -- else the history their everyday talk carries, found below.
            local memory = salvage.news(ctx, who) or project.news(ctx, who)
            local text
            local says = project.line(ctx, who, hold, station)
            -- A job's line heard already as news in their introduction gives way to their own.
            if history.told(ctx, who, says) then says = nil end
            local site = ctx.get(id, "build")
            if says then
                text = says
            elseif site and not site.done and site.keeper == facility.keeper then
                -- Their building still going up (`need` is after this sale; at 0 it is built).
                local left = (build and build.keeper == site.keeper) and need or site.need - site.given
                text = voices.waiting(left, site.need, site.ware, keeper.idle)
                -- The first of it just went in (this sale, which still reads none given) or
                -- has yet to be heard: they say so first, until the player hears it, in
                -- their introduction or here.
                local first = site.first_due or (site.given == 0 and left < site.need)
                if first and history.told(ctx, who, site.first_word) then
                    first = false
                    if site.first_due then ctx.patch(id, "build", { first_due = false }) end
                end
                if first and left > 0 and (site.first_word or "") ~= "" then
                    text = site.first_word .. " " .. text
                    memory = memory or site.first_word
                end
            elseif id == exchange then
                local wanted = {}
                for _, line in trade.lines(ctx, id) do
                    local market = ctx.get(line, "market")
                    if market.role == "buys" then wanted[market.ware] = hold[market.ware] end
                end
                local list = rules.list(wanted)
                local remembers = remembered(ctx, who, station)
                memory = memory or remembers
                if history.told(ctx, who, remembers) then remembers = nil end
                text = voices.trader(list == "nothing" and "" or list, sold, remembers or keeper.idle)
            elseif #trade.lines(ctx, id) > 0 then
                text = keeper.idle
            elseif facility.fits then
                text = nil -- the fitter's line follows the last fit (cradle.luau)
            elseif ctx.get(id, "build") then
                text = keeper.idle -- a built site's keeper (the greenhouse's grower)
            else
                local made = voices.made(ctx, station)
                if build and need <= 0 then made = "Your " .. build.ware .. " built our " .. string.lower(build.name) .. "." end
                if made ~= "" then memory = memory or made end
                if history.told(ctx, who, made) then made = "" end
                text = voices.master(left, build and need or 0, build and build.ware or "", build and string.lower(build.name) or "", made)
                if not build then
                    voices.intro_close(ctx, who, station)
                elseif need <= 0 then
                    -- This very sale finished it.
                    voices.intro_close(ctx, who, station, build.name)
                end
            end
            -- History comes before an introduction they haven't given yet.
            history.fold(ctx, who, memory)
            if text then ctx.patch(who .. "_now", "engine.talk_line", { text = text }) end
        end
    end
    -- Someone aboard who keeps no room (a barge's skipper) says their own line too.
    local _, job = salvage.find(ctx)
    if job and places.of(ctx, job.skipper) == station then
        local says = salvage.line(ctx, job.skipper, station)
        if says then ctx.patch(job.skipper .. "_now", "engine.talk_line", { text = says }) end
    end
    voices.gifts(ctx, station, hold)
    project.offers(ctx, station, hold)
    salvage.offers(ctx, station)
end

-- An answered gift's talk is put away and the keeper's everyday talk comes back.
-- Disabling a talk interrupts it, so this waits until the answer's last line is heard.
local function put_away(ctx, id, gift)
    ctx.patch(id, "engine.talk", { enabled = false })
    ctx.patch(gift.instead, "engine.talk", { enabled = true })
end

-- Puts away every answered gift still on offer: when its talk ends, or when another
-- starts in its place.
function voices.retire(ctx)
    for _, id in ctx.query("gift") do
        local gift = ctx.get(id, "gift")
        if gift.answered and ctx.get(id, "engine.talk").enabled then put_away(ctx, id, gift) end
    end
    -- A project's offer answered or no longer due gives way to the everyday talk, and
    -- the next one due comes on (project.offers).
    local station = places.docked(ctx)
    if station then
        project.offers(ctx, station, ctx.get("ship", "hold"))
        salvage.offers(ctx, station)
    end
end

-- Whether `keeper` still waits on their own building: a gift for it waits too. The sale
-- that finishes it still reads it unfinished, and leaves the offer as build_up set it.
local function waiting_on(ctx, keeper)
    for _, id in ctx.query("build") do
        local build = ctx.get(id, "build")
        if build.keeper == keeper and not build.done then return true end
    end
    return false
end

-- A keeper's unanswered gift is on offer, in place of their everyday talk, while they
-- stand at `station`, their building stands and the hold has room for it. `hold` is
-- after the caller's writes.
function voices.gifts(ctx, station, hold)
    for _, id in ctx.query("gift") do
        local gift = ctx.get(id, "gift")
        local talk = ctx.get(id, "engine.talk")
        if gift.answered then
            -- Still saying the answer: put away once it ends (voices.retire).
            if talk.enabled and not talk.active then put_away(ctx, id, gift) end
        elseif places.of(ctx, talk.speaker) == station and not waiting_on(ctx, talk.speaker) then
            local room = fit.hold(ctx) - rules.load(hold) >= gift.units
            if talk.enabled ~= room then
                ctx.patch(id, "engine.talk", { enabled = room })
                ctx.patch(gift.instead, "engine.talk", { enabled = not room })
            end
        end
    end
end

return voices
