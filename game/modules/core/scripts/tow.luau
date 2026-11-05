-- The tow line, in flight (scripts/salvage.luau has the job): passing a line to a barge
-- adrift, the line slipping when she strikes something fixed, her flares while she lies
-- dark. A mended barge sails when you launch (launch.luau).
local log = require("./log")
local teach = require("./teach")
local cutter = require("./cutter")
local salvage = require("./salvage")

-- The line goes across: logged with the skipper's word over the radio.
local function across(ctx)
    local text, said = salvage.hitch(ctx)
    if not text then return end
    log.add_all(ctx, { said, text })
    ctx.record("hitched", { barge = (select(2, salvage.find(ctx))).barge })
end

-- A line is passed by hand: the ship alongside her and slow. Space (main.luau records
-- `hitch` for it) or a click on her says why not when it can't be done yet.
local function pass(ctx)
    local barge, gap = salvage.near(ctx)
    if not barge then return end
    teach.used(ctx, "tow")
    local name = ctx.get(barge, "trader").name
    if gap > salvage.REACH then
        ctx.patch("ship", "pilot", { message = "Fly alongside " .. name .. " to pass her a line: within "
            .. salvage.REACH .. " units of her." })
        return
    end
    if cutter.speed(ctx) > salvage.SLOW then
        ctx.patch("ship", "pilot", { message = "Too fast to pass a line. Slow down beside " .. name .. "." })
        return
    end
    across(ctx)
end

local function hitch(event, ctx)
    if ctx.get("ship", "engine.location").place ~= "core.reach" then return end
    if event.source == "script" then
        -- A script's order flies itself, as its Mine and Dock do: out to her, then the line.
        local barge = salvage.near(ctx)
        if not barge then return end
        ctx.put("ship", "engine.helm", {})
        ctx.put("ship", "engine.steer", { mode = "hold", target = barge, range = 2 })
        ctx.patch("ship", "pilot", { message = "Heading out to " .. ctx.get(barge, "trader").name .. "." })
        return
    end
    pass(ctx)
end

return {
    on = {
        hitch = hitch,
        -- A script's order has brought the ship alongside: the line goes across.
        ["engine.arrived"] = function(event, ctx)
            if event.payload.entity ~= "core.ship" or event.payload.mode ~= "hold" then return end
            local barge = salvage.near(ctx)
            if not barge or event.payload.target ~= barge then return end
            ctx.put("ship", "engine.steer", {})
            across(ctx)
        end,
        -- The barge struck something fixed (a rock, a station's hull) hard enough: the line
        -- slips. Bumps with the ship or the traders don't.
        ["engine.contact"] = function(event, ctx)
            local _, job = salvage.find(ctx)
            if not job or not job.towing then return end
            local a, b = event.payload.entity, event.payload.other
            local other = a == job.barge and b or (b == job.barge and a or nil)
            if not other then return end
            local body = ctx.get(other, "engine.body")
            if not salvage.slips(body ~= nil and body.fixed, event.payload.speed or 0) then return end
            local text = salvage.slip(ctx, other)
            if text then
                log.add(ctx, text)
                ctx.record("slipped", { barge = job.barge, struck = other })
            end
        end,
    },
    -- While she lies dark, a flare goes up from her every few seconds.
    cadence = function(ctx)
        local _, job = salvage.find(ctx)
        if not job or job.towing or job.at ~= "" then return end
        if job.step ~= salvage.ASKED and job.step ~= salvage.SEARCH then return end
        if (ctx.tick // 30) % salvage.FLARE ~= 0 then return end
        ctx.put(job.barge, "engine.burst", { recipe = "core.flare", tick = ctx.tick, height = 0.4 })
    end,
}
