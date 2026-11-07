-- The ship's own screens in flight: the things you can click (each rock and each
-- station) and the bottom stack. Each kind of thing keeps one look and one place: status
-- top right, the log bottom left (status.luau, logbook.luau), what just happened at the
-- bottom with the key that fits where the ship is, and each station's name under it
-- while it is in view. A room panel or the open log takes the
-- whole focus, so the rest steps aside.
local rules = require("./rules")
local layout = require("./layout")
local kit = require("./kit")
local teach = require("./teach")
local signs = require("./signs")
local trade = require("./trade")
local fit = require("./fit")
local cutter = require("./cutter")
local salvage = require("./salvage")
local notice = require("./notice")

-- One hint at a time, for what a press does right now: Space in a berth's ring, the
-- beam with a rock in its sights, the turn to bring the nose onto one in reach, then the
-- flight keys; each until it has been used a few times (teach.luau).
local function hint(ctx)
    local taught = ctx.get("ship", "taught")
    local pilot = ctx.get("ship", "pilot")
    local full = rules.load(ctx.get("ship", "hold")) >= fit.hold(ctx)
    local ring = string.match(pilot.near, "^ring:(.+)$")
    if ring and ctx.get(ring, "port") and (teach.new(taught, "dock") or salvage.towing(ctx)) then
        return "Space · dock at " .. ctx.get(ring, "port").name
    end
    -- Beside a barge adrift, Space (or a click on her) passes her a line; with one in tow,
    -- 0 (or a click on her) casts it off.
    if pilot.near == "barge" and teach.new(taught, "tow") then return "Space or click her · pass her a line" end
    if salvage.towing(ctx) then
        if teach.new(taught, "cancel") then return "0 or click her · cast off the line" end
        return nil
    end
    -- Under a hand beam the stone has glazed: the move is to another rock.
    if pilot.mining and not pilot.auto and not cutter.has(ctx, cutter.RIG) and teach.new(taught, "swap") then
        local ore = ctx.get(pilot.target, "ore")
        if ore and rules.phase(cutter.ware(ore), ore.heat) == "glazed" then
            return "Click another rock, or turn onto one · this one has glazed, let it cool"
        end
    end
    if not full and not pilot.mining and teach.new(taught, "mine") then
        if pilot.near == "aim" then return "Hold Space · cut the rock ahead" end
        if pilot.near == "reach" then return "A or D · point your nose at the rock" end
    end
    if full and teach.new(taught, "dock") then
        if cutter.has(ctx, cutter.JETS) then return "E · the docking jets fly you in" end
        return "Fly slowly into a berth's glowing ring to dock"
    end
    if teach.new(taught, "thrust") then return "W · fly ahead" end
    if teach.new(taught, "turn") then return "A or D · turn" end
    if teach.new(taught, "boost") then return "Shift · boost" end
    if not full and teach.new(taught, "mine") then return "Fly up to a rock and face it to cut" end
    if ctx.get("ship", "engine.steer").mode ~= "" and teach.new(taught, "cancel") then return "0 · cancel the order" end
    return nil
end

-- The bottom of the screen in flight: one surface per kind of thing, a gap apart, so a
-- glance tells them apart (direction doc 1.5; the looks are style.toml's). From the top:
--   the latest message: a person on the radio (the radio's look, their name on the amber
--     tag before their words) or what just happened (a lighter plum card, the ship's icon)
--   the goal it gives, with its WANTED tag, and a lesson about the game (a paper card)
--   where the stations off screen lie (a small dark label, like their names), and the
--     one key that fits now (the gold-framed pill, the key standing out), side by side
--     when both fit
-- The message and its goal and lesson are notice.luau's; a hint with no key in it is a
-- lesson too. A short line's surface is only as wide as its words, so the stack covers
-- no more of the reach than one panel holding them all.
-- Small print runs at most SMALL px a character and body text BODY (counted in bytes,
-- so a curly quote or a dot only overcounts); a surface's own frame and icon add FRAME
-- (a label, a goal, a lesson, a note) or PILL (the key pill's rounder ends).
local GAP, SMALL, BODY, FRAME, PILL = 6, 7.6, 9.2, 36, 16
local function fits(text, extra, room, per)
    return #text * (per or SMALL) + extra <= room
end

local function bottom(ctx, ui, pilot)
    local wide = layout.MESSAGE
    local stack = {}
    local goal, lesson, press = notice.parts(pilot)
    if pilot.message ~= "" then
        local who, from, words = notice.speech(pilot.message)
        if who then
            -- Who speaks first, on the name tag, then where from, quieter, then the words.
            local spans = kit.speaker(who, from)
            table.insert(spans, "  " .. words)
            table.insert(stack, kit.radio(ui, { id = "message", width = wide, children = {
                ui.text { id = "said", width = wide - 16, icon = "radio", spans = spans, size = kit.SIZE.body, cue = "flash" },
            } }))
        elseif fits(pilot.message, FRAME, wide, BODY) then
            table.insert(stack, ui.text { id = "message", look = kit.LOOK.note, icon = "ship", text = pilot.message,
                size = kit.SIZE.body, cue = "flash" })
        else
            table.insert(stack, kit.note(ui, { id = "message", width = wide, children = {
                ui.text { id = "said", width = wide - 16, icon = "ship", text = pilot.message, size = kit.SIZE.body, cue = "flash" },
            } }))
        end
    end
    if goal ~= "" then
        table.insert(stack, ui.text { id = "goal", look = kit.LOOK.goal, size = kit.SIZE.small,
            width = not fits("WANTED  " .. goal, FRAME, wide) and wide or nil,
            spans = { { text = "WANTED", look = kit.LOOK.kind }, "  " .. goal } })
    end
    local key = press ~= "" and press or hint(ctx)
    -- A hint with no key in it ("Fly slowly into a berth's glowing ring to dock") says how
    -- the game works: a lesson, unless the message brought one.
    if key and not string.find(key, " · ", 1, true) then
        if lesson == "" then lesson = key end
        key = nil
    end
    if lesson ~= "" then
        table.insert(stack, ui.text { id = "lesson", look = kit.LOOK.lesson, size = kit.SIZE.small, text = lesson,
            width = not fits(lesson, FRAME, wide) and wide or nil })
    end
    -- The stations off screen and the way each lies: no arrows, but the way home is never
    -- lost in a reach several screens across.
    local ways = pilot.bearings ~= "" and kit.label(ui, { id = "bearings", icon = "station", text = pilot.bearings,
        size = kit.SIZE.small }) or nil
    local keys = key and ui.text { id = "hints", look = kit.LOOK.pill, pad = 4, spans = kit.hint(key), size = kit.SIZE.small }
        or nil
    if ways and keys and fits(pilot.bearings .. key, FRAME + PILL + GAP, layout.BOTTOM) then
        -- Side by side, each keeps the width of its words.
        ways.width = math.ceil(#pilot.bearings * SMALL + FRAME)
        ways.height = 28 -- as tall as the pill beside it
        keys.width = math.ceil(#key * SMALL + PILL)
        table.insert(stack, ui.row { id = "ways", gap = GAP, children = { ways, keys } })
    else
        if ways then table.insert(stack, ways) end
        if keys then table.insert(stack, keys) end
    end
    return ui.column { id = "bottom", align = "bottom", y = layout.M, width = layout.BOTTOM, gap = GAP, children = stack }
end

local function where(ctx)
    local pilot = ctx.get("ship", "pilot")
    local here = ctx.get("ship", "engine.location").place
    local docked = here ~= "" and ctx.get(here, "port") ~= nil
    return pilot, here == "core.reach", docked
end

-- Each station wears its name under it while it is in view, as the traders do
-- (traffic.luau), on a small dark tag (kit.label) so it reads as a name, not a panel:
-- `yield`, so it is left undrawn rather than sit on the HUD.
local function station_names(ctx, ui, children)
    for _, id in ctx.query("port") do
        local name = ctx.get(id, "port").name
        table.insert(children, kit.label(ui, { id = "name_" .. string.match(id, "%.(%w+)$"), anchor = id .. "_exterior#label",
            width = #name * 9 + 16, text = name, size = kit.SIZE.small, yield = true }))
    end
end

-- Everything in flight is also a click on the thing itself (direction doc 9, "Mouse
-- first"): a rock lifts and outlines under the pointer and a click cuts it, a station
-- docks. What a click does is said on hover; what is in reach depends on the fit.
local function targets(ctx, ui, children)
    local rig = cutter.has(ctx, cutter.RIG)
    local pilot = ctx.get("ship", "pilot")
    for _, id in ctx.query("ore") do
        if ctx.get(id, "engine.anchor") then
            local ore = ctx.get(id, "ore")
            -- Each rock names its own ware: ore, ice or glassite.
            local ware = ore.ware ~= "" and ore.ware or "ore"
            local text
            if ore.left <= 0 then
                text = "Cut down to pebbles"
            elseif pilot.mining and pilot.target == id then
                text = ore.left .. " of " .. ore.total .. " " .. ware .. " left · click to stop the beam"
            elseif rig then
                text = ore.left .. " of " .. ore.total .. " " .. ware .. " · click: the rig flies out and cuts it"
            else
                text = ore.left .. " of " .. ore.total .. " " .. ware .. " · click to cut it, nose on it and in reach"
            end
            table.insert(children, ui.target { id = "rock_" .. string.match(id, "%.(%w+)$"), anchor = id .. "#rock",
                text = text, on = "mine", payload = { rock = id } })
        end
    end
    local jets = cutter.has(ctx, cutter.JETS) and not salvage.towing(ctx)
    for _, id in ctx.query("port") do
        local how = jets and "click: the docking jets fly you in" or "fly slowly into its berth's ring to dock"
        table.insert(children, ui.target { id = "dock_" .. string.match(id, "%.(%w+)$"), anchor = id .. "_exterior#label",
            text = trade.posted(ctx, id) .. " · " .. how, on = "dock", payload = { station = id } })
    end
end

return {
    on = {
        log = function(event, ctx)
            if event.payload.down == false then return end
            local pilot = ctx.get("ship", "pilot")
            if pilot.room ~= "" then return end
            -- The open log takes the whole focus: a bubble would sit on it.
            signs.hush(ctx)
            ctx.patch("ship", "pilot", { log_open = not pilot.log_open })
            local here = ctx.get("ship", "engine.location").place
            if here ~= "" and ctx.get(here, "port") then signs.show(ctx, here, "", not pilot.log_open, nil) end
        end,
    },
    screen = function(ctx, ui)
        local pilot, space, docked = where(ctx)
        if not space and not docked then return nil end
        -- The status chip and the log are their own screens (status.luau, logbook.luau).
        if pilot.log_open then return nil end
        if docked and pilot.room ~= "" then return nil end
        local children = {}
        if space then
            station_names(ctx, ui, children)
            targets(ctx, ui, children)
            table.insert(children, bottom(ctx, ui, pilot))
        end
        return layout.frame(ui, "hud", children)
    end,
}
