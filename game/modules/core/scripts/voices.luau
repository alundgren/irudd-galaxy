-- What each keeper says after their introduction: one short line that follows the
-- shift, so a second click hears something current instead of the same speech again.
local rules = require("./rules")
local trade = require("./trade")
local fit = require("./fit")

local voices = {}

-- Ore still in the field's rocks.
function voices.field(ctx)
    local left = 0
    for _, id in ctx.query("ore") do
        left += ctx.get(id, "ore").left
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
            if id == exchange then
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
            else
                text = voices.master(left, build and need or 0, build and build.ware or "", build and string.lower(build.name) or "")
            end
            if text then ctx.patch(facility.keeper .. "_now", "engine.talk_line", { text = text }) end
        end
    end
    voices.gifts(ctx, station, hold)
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
end

-- A keeper's unanswered gift is on offer, in place of their everyday talk, while they
-- stand at `station` and the hold has room for it. `hold` is after the caller's writes.
function voices.gifts(ctx, station, hold)
    for _, id in ctx.query("gift") do
        local gift = ctx.get(id, "gift")
        local talk = ctx.get(id, "engine.talk")
        local at = ctx.get(talk.speaker, "engine.location")
        if gift.answered then
            if talk.enabled then put_away(ctx, id, gift) end
        elseif at and at.place == station then
            local room = fit.hold(ctx) - rules.load(hold) >= gift.units
            if talk.enabled ~= room then
                ctx.patch(id, "engine.talk", { enabled = room })
                ctx.patch(gift.instead, "engine.talk", { enabled = not room })
            end
        end
    end
end

return voices
