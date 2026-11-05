-- Sound cues for what the player does. Key bindings send a fact on press and another on
-- release, and audio.toml plays a sound on every fact of its event, so this script
-- records one cue event per real change instead: thrust starting and stopping, boost,
-- leaving a dock, a click, a refusal, a mining chip, a full hold, a paid receipt, a talk
-- line appearing and a bump. It is listed first in the manifest, so its handlers read
-- the world as it was before the other scripts acted on the same input. It writes nothing but these facts and its own note of the last talk line it
-- cued, so it never changes what happens. Facts that already mark a moment (docked,
-- mined, sold, bought, fitted, grown, the radio's channel cues and an answer chosen) are
-- cued straight from audio.toml; a talk starting blips as its bubble opens ([ui]).
--
-- Screens sound from audio.toml's [ui] table on the frame they change: a click on a
-- screen (an event from the source "ui") is its press, and a room's panel or the log
-- opening or closing sounds as it appears or goes, whether a key or a click opened it.
-- So the clicks here are for keys only, and panels have no cue here at all. Bumps: a
-- contact the ship is part of knocks on the hull. The drive's and the stations' loops
-- start again after a resume or a hot load (`drive_on`), unless the ship is docked.
local rules = require("./rules")
local fit = require("./fit")

local MOVES = { ahead = true, back = true, left = true, right = true }
local OTHER = { ahead = "back", back = "ahead" }

local function pressed(event)
    return event.payload.down ~= false
end

-- A screen click: [ui] in audio.toml sounds it.
local function clicked(event)
    return event.source == "ui"
end

local function here(ctx)
    return ctx.get("ship", "engine.location").place
end

local function docked(ctx)
    local place = here(ctx)
    return place ~= "" and ctx.get(place, "port") ~= nil
end

local function flying(ctx)
    return here(ctx) == "core.reach"
end

local function held(ctx)
    return ctx.get("ship", "held") or { ahead = false, back = false, left = false, right = false, boost = false }
end

-- W, A, S, D and Shift. Docked, a movement key flies the ship off the dock (main.luau).
local function flight(event, ctx)
    local key = string.sub(event.kind, 6)
    local down = event.payload.down == true
    local keys = held(ctx)
    if docked(ctx) then
        if not down or not MOVES[key] then return end
        ctx.record("undocked", {})
        if OTHER[key] then ctx.record("thrust_on", {}) end
        return
    end
    if not flying(ctx) then return end
    if key == "boost" then
        if down and not keys.boost then ctx.record("boost_on", {}) end
        return
    end
    -- Thrust is W less S (main.luau): cue when it starts or stops, whichever key moved
    -- it, so S pressed against a held W stops it and let go starts it again.
    if not OTHER[key] then return end
    local before = (keys.ahead and 1 or 0) - (keys.back and 1 or 0)
    keys[key] = down
    local after = (keys.ahead and 1 or 0) - (keys.back and 1 or 0)
    if before == 0 and after ~= 0 then
        ctx.record("thrust_on", {})
    elseif before ~= 0 and after == 0 then
        ctx.record("thrust_off", {})
    end
end

-- A line clicked on: the engine records the click even when it is a stale repeat that
-- moves nothing on, so only a click that advanced the talk (to the line's next, or to
-- its end) and isn't the line just cued blips. A talk starting or an answer chosen
-- clears the note, so a later run through the same lines blips again.
local function talk_next(event, ctx)
    local talk = ctx.get(event.payload.talk, "engine.talk")
    local said = ctx.get(event.payload.line, "engine.talk_line")
    if not talk or not said then return end
    local last = ctx.get("ship", "blipped")
    if last and last.line == event.payload.line then return end
    local following = said.next or ""
    local advanced = if following ~= "" then talk.current == following else not talk.active
    if not advanced then return end
    ctx.put("ship", "blipped", { line = event.payload.line })
    ctx.record("talk_line", {})
end

local function talk_reset(_, ctx)
    if ctx.has("ship", "blipped") then ctx.remove("ship", "blipped") end
end

local function launch(event, ctx)
    if pressed(event) and docked(ctx) then ctx.record("undocked", {}) end
end

local function click_in_flight(event, ctx)
    if pressed(event) and flying(ctx) and not clicked(event) then ctx.record("ui_click", {}) end
end

-- Mining with a full hold is refused (main.luau); otherwise the order is taken.
local function mine(event, ctx)
    if not pressed(event) or not flying(ctx) then return end
    if rules.load(ctx.get("ship", "hold")) >= fit.hold(ctx) then
        ctx.record("refused", {})
    elseif not clicked(event) then
        ctx.record("ui_click", {})
    end
end

-- A bump the ship is part of, flown into or hit by.
local SHIP = { ship = true, ["core.ship"] = true }

local function contact(event, ctx)
    local p = event.payload
    if SHIP[p.entity] or SHIP[p.other] then
        ctx.record("bump", { entity = SHIP[p.other] and p.entity or p.other, speed = p.speed })
    end
end

-- Loops don't come back on their own after a resume or a hot load.
local function drive_on(_, ctx)
    if ctx.has("ship", "engine.location") and not docked(ctx) then ctx.record("drive_on", {}) end
end

return {
    on = {
        ahead = flight, back = flight, left = flight, right = flight, boost = flight,
        launch = launch,
        dock = click_in_flight, radio = click_in_flight, mine = mine,
        ["engine.talk_next"] = talk_next,
        ["engine.talk_start"] = talk_reset,
        ["engine.talk_chosen"] = talk_reset,
        ["engine.contact"] = contact,
        ["engine.resumed"] = drive_on,
        ["engine.hot_loaded"] = drive_on,
        -- A chip for each second's ore, one of three in turn so it never ticks like a
        -- clock; the unit that fills the hold chimes.
        mined = function(_, ctx)
            local hold = ctx.get("ship", "hold")
            ctx.record("chip" .. (hold.ore % 3 + 1), {})
            if rules.load(hold) >= fit.hold(ctx) then ctx.record("hold_full", {}) end
        end,
        -- A part bought at the fitter gets its receipt; one from the locker doesn't.
        fitted = function(event, ctx)
            if (event.payload.credits or 0) > 0 then ctx.record("confirmed", {}) end
        end,
    },
}
