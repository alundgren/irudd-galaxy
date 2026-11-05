-- What each keeper says after their introduction: one short line that follows the
-- shift, so a second click hears something current instead of the same speech again.
local rules = require("./rules")
local trade = require("./trade")
local fit = require("./fit")
local places = require("./places")
local project = require("./project")
local salvage = require("./salvage")

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

-- The office keeper: about what the station is building, then about the field.
function voices.master(left, need, ware, building)
    if need > 0 then return "The " .. building .. " still needs " .. need .. " " .. ware .. "." end
    if left > 0 then return "The field still holds " .. left .. " ore." end
    return "The field's worked out. Good shift."
end

-- The office keeper's introduction ends on the station's first build. Once it is
-- finished the intro's last line says so, so a first meeting after it never tells of
-- raising it. `finished` names a build the caller has just finished (its reads still see
-- it unfinished); unset, a build already marked done is looked for. Later builds (a
-- project's) leave the intro as it is.
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
            local keeper = ctx.get(facility.keeper, "keeper")
            local text
            local says = project.line(ctx, facility.keeper, hold, station)
            if says then
                text = says
            elseif id == exchange then
                local wanted = {}
                for _, line in trade.lines(ctx, id) do
                    local market = ctx.get(line, "market")
                    if market.role == "buys" then wanted[market.ware] = hold[market.ware] end
                end
                local list = rules.list(wanted)
                text = voices.trader(list == "nothing" and "" or list, sold, keeper.idle)
            elseif #trade.lines(ctx, id) > 0 then
                text = keeper.idle
            elseif facility.fits then
                text = nil -- the fitter's line follows the last fit (cradle.luau)
            elseif ctx.get(id, "build") then
                text = keeper.idle -- a built site's keeper (the greenhouse's grower)
            else
                text = voices.master(left, build and need or 0, build and build.ware or "", build and string.lower(build.name) or "")
                if not build then
                    voices.intro_close(ctx, facility.keeper, station)
                elseif need <= 0 then
                    -- This very sale finished it.
                    voices.intro_close(ctx, facility.keeper, station, build.name)
                end
            end
            if text then ctx.patch(facility.keeper .. "_now", "engine.talk_line", { text = text }) end
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

-- A keeper's unanswered gift is on offer, in place of their everyday talk, while they
-- stand at `station` and the hold has room for it. `hold` is after the caller's writes.
function voices.gifts(ctx, station, hold)
    for _, id in ctx.query("gift") do
        local gift = ctx.get(id, "gift")
        local talk = ctx.get(id, "engine.talk")
        if gift.answered then
            if talk.enabled then put_away(ctx, id, gift) end
        elseif places.of(ctx, talk.speaker) == station then
            local room = fit.hold(ctx) - rules.load(hold) >= gift.units
            if talk.enabled ~= room then
                ctx.patch(id, "engine.talk", { enabled = room })
                ctx.patch(gift.instead, "engine.talk", { enabled = not room })
            end
        end
    end
end

return voices
