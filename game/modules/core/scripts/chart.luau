-- The navigation spine (direction doc 2.2, 5, 13 and 14): who the pilot is, a way to chart
-- the reach, the drawing in the glass tag and the chart glass Orrin makes from it.
--
--   OPENING  A new world opens on memory 1 (start.toml, log1). The first time the pilot
--            flies by hand, Bram welcomes them on the radio: nobody sells a chart of the
--            reach, so the pilot's own goal is a way to chart it, and Orrin at Cinder
--            Kiln fits out tugs. The log pins the goal until the chart glass is in.
--   FULL     Flying off with the first full hold, Mags says on the radio where both
--            stations lie (scripts/traffic.luau, with the skippers' hails).
--   TAG      Orrin's introduction spots the glass tag on the canopy frame; held to the
--            light, it releases memory 3 and shows the chart glass's drawing (the part's
--            `found`), so Orrin offers to make it (scripts/cradle.luau, fit.verdict).
--   MINIMAP  Made and fitted, the chart glass shows the ship's memory of where it has
--            flown (engine.explored, kept from the first tick) under fog, in the
--            status chip in flight (chart.map, scripts/chip.luau), and Orrin says what
--            comes next, once (scripts/spine.luau).
--
-- Nothing here points anywhere: directions are said by people, vague but true, and the
-- minimap only shows what the ship has flown near. Plain texts first, then world reads.
local layout = require("./layout")

local chart = {}

-- What Bram says the first time the pilot flies by hand, the goal it gives and what the
-- log does with it, each shown on its own (scripts/notice.luau).
chart.WELCOME = "Bram on Lantern Dock's radio: \u{201C}Welcome, pilot. Nobody sells a chart of the reach; we fly by eye. Orrin fits out tugs at Cinder Kiln, south-east of the ore drift.\u{201D}"
chart.GOAL = "Find a way to chart the reach"
chart.LESSON = "Your log keeps your goal."

-- What Mags says on the radio as the pilot flies off with the first full hold: where the
-- buyers lie, from the drift.
chart.FULL = "Mags on Cinder Kiln's radio: \u{201C}Full hold. Cinder Kiln buys ore, south-east of the drift. Lantern Dock does too, south-west.\u{201D}"

-- Memory 3 (beings/player.md), released by the tag held to the light: facts only.
chart.MEMORY = "A small chart glowing in front of a child's face, a way drawn on it, fading."

-- The movement keys: the first one pressed by hand in the reach is the first flight.
local MOVES = { ahead = true, back = true, left = true, right = true }

-- Whether `event` is the pilot's first flight by hand: a movement key pressed, not a
-- script's order.
function chart.first_flight(event)
    local key = string.sub(event.kind, 6)
    return MOVES[key] == true and event.payload.down == true and event.source ~= "script"
end

-- What the drawing in the tag says in the log once held to the light: the part it shows
-- and what Orrin makes it from.
function chart.drawing(p)
    local from = (p.units or 0) > 0 and p.units .. " " .. p.ware .. " and " .. p.price .. " cr" or p.price .. " cr"
    return "Under the light, lines show in the tag's glass: the drawing of a " .. string.lower(p.name)
        .. ". Orrin can make it from " .. from .. "."
end

-- What the log pins while the goal is open: the goal, then the part once its drawing is
-- found, with what is still to bring. `p` is the drawn part (nil before it exists), `have`
-- the units of its ware aboard and `credits` the pilot's.
function chart.pin(p, have, credits)
    if not p or not p.found then
        return { kind = "GOAL", who = "A chart of the reach", where = "",
            text = "Nobody sells one. Bram: Orrin fits out tugs at Cinder Kiln, south-east of the ore drift." }
    end
    local short = {}
    if (p.units or 0) > have then table.insert(short, (p.units - have) .. " more " .. p.ware) end
    if p.price > credits then table.insert(short, (p.price - credits) .. " more cr") end
    local still = #short > 0 and " Still to bring: " .. table.concat(short, " and ") .. "." or " You have what it takes."
    return { kind = "GOAL", who = p.name, where = "Cinder Kiln",
        text = "Orrin makes it from the tag's drawing: " .. p.units .. " " .. p.ware .. " and " .. p.price .. " cr." .. still }
end

-- What the chart glass marks, once flown near: the stations, and the drifts by the rock
-- or buoy at their heart. `name` is how the log says it.
chart.MARKS = {
    { entity = "core.rock1", text = "Ore", icon = "ore", name = "the ore drift" },
    { entity = "core.lantern_exterior", text = "Lantern Dock", icon = "station", name = "Lantern Dock" },
    { entity = "core.kiln_exterior", text = "Cinder Kiln", icon = "station", name = "Cinder Kiln" },
    { entity = "core.buoy_ice", text = "Ice", icon = "ice", name = "the ice drift" },
    { entity = "core.buoy_glassite", text = "Glassite", icon = "glassite", name = "the glassite drift" },
}

-- The chart glass's map: SIZE logical pixels square (the status chip's width inside its
-- padding, style.toml's chip pad of 12), SPAN world units across (from the drift, the ore
-- and both stations fit).
chart.SIZE, chart.SPAN = layout.CHIP - 24, 640

-- World reads ------------------------------------------------------------------------

-- The part made from the tag's drawing (the chart glass), its ID and fields, or nil (a
-- save from before it has none).
function chart.part(ctx)
    for _, id in ctx.query("part") do
        local p = ctx.get(id, "part")
        if p.blueprint then return id, p end
    end
    return nil, nil
end

-- Whether the chart glass is fitted: the minimap shows, and what comes next opens up.
function chart.fitted(ctx)
    local _, p = chart.part(ctx)
    return p ~= nil and p.fitted
end

-- The log's pin for the goal, or nil once the chart glass is fitted or before Bram gave it.
function chart.pins(ctx)
    local pilot = ctx.get("ship", "pilot")
    local _, p = chart.part(ctx)
    if not pilot or not pilot.welcomed or (p and p.fitted) then return {} end
    local hold = ctx.get("ship", "hold") or {}
    return { chart.pin(p, p and (hold[p.ware] or 0) or 0, pilot.credits) }
end

-- The tag held to the light (`choice`, answering Orrin's line in `talk`): the drawing is
-- found, once. Returns the log's lines (the drawing, then memory 3), or nil when this was
-- not the tag or it was held up before. They reach the log only: notices are hidden in
-- rooms, and the launch's message replaces them. The talk picks up after the answer if
-- left.
function chart.held(ctx, talk, choice)
    if choice ~= "core.kiln_fitter_hold" then return nil end
    local id, p = chart.part(ctx)
    local answer = ctx.get(choice, "engine.talk_choice")
    if answer and (answer.next or "") ~= "" then ctx.patch(talk, "engine.talk", { first = answer.next }) end
    if not id or p.found then return nil end
    ctx.patch(id, "part", { found = true })
    ctx.record("drawn", { part = id })
    return { chart.drawing(p), chart.MEMORY }
end

-- The chart glass in flight, under the credits and the hold in the status chip: the
-- engine's ui.map of the ship's memory (engine.explored, kept from the first tick), clear
-- where it has flown and fog elsewhere, north up like the flight view, the ship's heading
-- at its centre. The marks show only on ground the ship has flown near. Nil until the
-- chart glass is fitted, outside the open reach and while the log is open.
function chart.map(ctx, ui)
    if not chart.fitted(ctx) or ctx.get("ship", "engine.location").place ~= "core.reach" then return nil end
    if ctx.get("ship", "pilot").log_open then return nil end
    local marks = {}
    for _, m in chart.MARKS do
        if ctx.get(m.entity, "engine.position") then table.insert(marks, { entity = m.entity, text = m.text, icon = m.icon }) end
    end
    return ui.map { id = "chart_glass", width = chart.SIZE, height = chart.SIZE, span = chart.SPAN, memory = "core.ship",
        follow = "core.ship", marks = marks }
end

return chart
