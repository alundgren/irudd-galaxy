-- Sound cues for what the player does. Key bindings send a fact on press and another on
-- release, and audio.toml plays a sound on every fact of its event, so this script
-- records one cue event per real change instead: thrust starting and stopping, boost,
-- leaving a dock, a panel opening or closing, a click, a refusal, a mining chip, a full
-- hold, a paid receipt and a talk line appearing. It is listed first in the manifest, so
-- its handlers read the world as it was before the other scripts acted on the same
-- input. It writes nothing but these facts and its own note of the last talk line it
-- cued, so it never changes what happens. Facts that already mark a moment (docked,
-- mined, sold, bought, fitted, grown, the radio's channel cues, a talk starting and an
-- answer chosen) are cued straight from audio.toml.
local rules = require("./rules")
local fit = require("./fit")

local MOVES = { ahead = true, back = true, left = true, right = true }
local OTHER = { ahead = "back", back = "ahead" }

local function pressed(event)
    return event.payload.down ~= false
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

-- A room opens when it isn't the one already open (station.luau).
local function open(event, ctx)
    if not pressed(event) or not docked(ctx) then return end
    local id = event.payload.facility or ctx.get(here(ctx), "port").exchange
    local facility = ctx.get(id, "facility")
    if not facility or facility.station ~= here(ctx) then return end
    if ctx.get("ship", "pilot").room ~= id then ctx.record("panel_open", {}) end
end

-- Back closes the open room, else the open log (station.luau).
local function close(event, ctx)
    if not pressed(event) then return end
    local pilot = ctx.get("ship", "pilot")
    if (docked(ctx) and pilot.room ~= "") or pilot.log_open then ctx.record("panel_close", {}) end
end

-- L opens or shuts the log, except inside a room (hud.luau).
local function log(event, ctx)
    if not pressed(event) then return end
    local pilot = ctx.get("ship", "pilot")
    if pilot.room ~= "" then return end
    ctx.record(pilot.log_open and "panel_close" or "panel_open", {})
end

local function click_in_flight(event, ctx)
    if pressed(event) and flying(ctx) then ctx.record("ui_click", {}) end
end

-- Mining with a full hold is refused (main.luau); otherwise the order is taken.
local function mine(event, ctx)
    if not pressed(event) or not flying(ctx) then return end
    if rules.load(ctx.get("ship", "hold")) >= fit.hold(ctx) then
        ctx.record("refused", {})
    else
        ctx.record("ui_click", {})
    end
end

return {
    on = {
        ahead = flight, back = flight, left = flight, right = flight, boost = flight,
        launch = launch, open = open, close = close, log = log,
        dock = click_in_flight, radio = click_in_flight, mine = mine,
        ["engine.talk_next"] = talk_next,
        ["engine.talk_start"] = talk_reset,
        ["engine.talk_chosen"] = talk_reset,
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
