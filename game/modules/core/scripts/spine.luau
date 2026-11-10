-- The navigation spine in play (scripts/chart.luau has its rules and words, and the
-- chart glass's map, which the status chip shows in flight): Bram's welcome on the first
-- flight by hand, and what the chart glass shows and Orrin says once it is in.
local chart = require("./chart")
local log = require("./log")

-- "the ore drift, Lantern Dock and Cinder Kiln"
local function listed(names)
    if #names <= 1 then return names[1] or "" end
    return table.concat(names, ", ", 1, #names - 1) .. " and " .. names[#names]
end

-- What the chart glass shows the moment it is in: the marked places the ship has already
-- flown near (its memory, kept from the first tick), and fog over the rest.
local function shows(ctx)
    local seen = {}
    for _, m in chart.MARKS do
        local at = ctx.get(m.entity, "engine.position")
        if at and ctx.explored("core.ship", at.x, at.y) then table.insert(seen, m.name) end
    end
    if #seen == 0 then return "The chart glass shows only what she has flown near; the rest lies under fog." end
    return "The chart glass shows what she has flown near: " .. listed(seen) .. ". The rest lies under fog."
end

local function in_reach(ctx)
    return ctx.get("ship", "engine.location").place == "core.reach"
end

-- The first flight by hand: Bram's welcome over the radio, with the goal it gives.
local function flight(event, ctx)
    if not in_reach(ctx) or not chart.first_flight(event) then return end
    local pilot = ctx.get("ship", "pilot")
    if pilot.welcomed then return end
    ctx.patch("ship", "pilot", { welcomed = true })
    log.add(ctx, chart.WELCOME, { goal = chart.GOAL, lesson = chart.LESSON })
end

-- The chart glass is in: Orrin says what comes next, once, on the next click. The galley
-- is one choice of three, so he speaks of it as it stands.
local function fitted(event, ctx)
    local id = chart.part(ctx)
    if not id or event.payload.part ~= id or event.payload.off then return end
    local galley = nil
    for _, b in ctx.query("build") do
        local build = ctx.get(b, "build")
        if build.station == "core.lantern" and build.ware == "plate" then galley = build end
    end
    if galley and galley.done then
        ctx.patch("core.kiln_fitter_next3", "engine.talk_line", { text = "Ines's galley at Lantern Dock feeds the crews hot now." })
    end
    local next = ctx.get("core.kiln_fitter_next", "engine.talk")
    if next and not next.seen then ctx.patch("core.kiln_fitter_next", "engine.talk", { enabled = true }) end
    log.add(ctx, shows(ctx))
end

return {
    on = {
        ahead = flight, back = flight, left = flight, right = flight,
        fitted = fitted,
    },
}
