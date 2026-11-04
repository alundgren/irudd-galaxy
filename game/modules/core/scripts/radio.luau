-- The ship's radio: each station broadcasts a channel with a range, and in flight the
-- radio plays one at the bottom right with its signal, a ticker of what the station
-- says, and Prev, Off and Next (R). Once a second it measures every channel and keeps
-- the one it has until that is clearly lost or another is clearly and steadily
-- stronger (signal.luau), so it never flickers between stations. Docked, it falls
-- quiet under the station's own sound. Its music is cued by recorded events
-- (audio.toml), so the same channel plays the same track in every save.
local signal = require("./signal")
local layout = require("./layout")

local function channels(ctx)
    local ids = ctx.query("channel")
    table.sort(ids, function(a, b) return ctx.get(a, "channel").order < ctx.get(b, "channel").order end)
    return ids
end

-- Every channel's strength where the ship is now.
local function strengths(ctx, order)
    local at, out = ctx.get("ship", "engine.position"), {}
    for _, id in order do
        local channel = ctx.get(id, "channel")
        local from = ctx.get(channel.source, "engine.position")
        local d = math.sqrt((at.x - from.x) ^ 2 + (at.y - from.y) ^ 2)
        out[id] = signal.strength(d, channel.range)
    end
    return out
end

local function flying(ctx)
    return ctx.get("ship", "engine.location").place == "core.reach"
end

-- The radio's fields once `channel` is tuned: a new channel starts its first line and
-- shows its signal afresh.
local function retune(radio, channel, strength)
    if channel == radio.channel then return {} end
    return { channel = channel, level = channel ~= "" and signal.level(0, strength) or 0,
        bit = 1, shown = 0, lost = 0, better = 0, hand = false }
end

-- Starts the music of what should be playing, or stops it, when that changed.
local function cue(ctx, radio, want)
    if want == radio.playing then return {} end
    ctx.record(want == "" and "radio_off" or ctx.get(want, "channel").cue, { channel = want })
    return { playing = want }
end

-- Cues the channel that was playing once more, after the music stopped without the
-- radio being touched. Docked, off or with no signal, nothing was playing.
local function replay(_, ctx)
    if not ctx.has("ship", "radio") or not flying(ctx) then return end
    local radio = ctx.get("ship", "radio")
    if radio.playing == "" then return end
    ctx.record(ctx.get(radio.playing, "channel").cue, { channel = radio.playing })
end

local function merge(into, from)
    for k, v in from do into[k] = v end
    return into
end

-- The lines a channel says, in order.
local function bits(ctx, channel)
    local out = {}
    for _, id in ctx.query("bit") do
        local bit = ctx.get(id, "bit")
        if bit.channel == channel then out[bit.n] = bit.text end
    end
    return out
end

-- One second of the radio in flight.
local function second(ctx)
    local radio = ctx.get("ship", "radio")
    local order = channels(ctx)
    local s = strengths(ctx, order)
    -- Coming back on (a new game's first second, or out of a station): straight to
    -- the strongest station, unless the player chose one.
    local tuned, lost, better
    if radio.on and radio.playing == "" and not radio.hand then
        tuned, lost, better = signal.strongest(order, s) or "", 0, 0
    else
        tuned, lost, better = signal.tune(order, s, radio.channel, radio.lost, radio.better, radio.hand)
    end
    local change = retune(radio, tuned, s[tuned] or 0)
    if next(change) == nil then
        change = { lost = lost, better = better }
        if tuned ~= "" then change.level = signal.level(radio.level, s[tuned]) end
        local lines = #bits(ctx, tuned)
        if lines > 0 and radio.shown + 1 >= signal.BIT_SECONDS then
            change.bit, change.shown = radio.bit % lines + 1, 0
        elseif lines > 0 then
            change.shown = radio.shown + 1
        end
    end
    merge(change, cue(ctx, radio, radio.on and tuned or ""))
    ctx.patch("ship", "radio", change)
end

-- Prev and Next step through the channels in range, then Off; R steps on.
local function tune(event, ctx)
    if event.payload.down == false or not flying(ctx) or not ctx.has("ship", "radio") then return end
    local radio = ctx.get("ship", "radio")
    local order = channels(ctx)
    local s = strengths(ctx, order)
    local change
    if event.payload.tune == "toggle" then
        if radio.on then
            change = { on = false }
        else
            local back = radio.channel ~= "" and radio.channel or signal.strongest(order, s) or ""
            change = merge({ on = true }, retune(radio, back, s[back] or 0))
        end
    else
        local choices = {}
        for _, id in order do
            if s[id] > 0 then table.insert(choices, id) end
        end
        table.insert(choices, "off")
        local pick = signal.step(choices, radio.on and radio.channel or "off", event.payload.tune == "prev" and -1 or 1)
        if pick == "off" then
            change = { on = false }
        else
            change = merge(merge({ on = true }, retune(radio, pick, s[pick])), { hand = true })
        end
    end
    local now = merge(table.clone(radio), change)
    merge(change, cue(ctx, radio, now.on and now.channel or ""))
    ctx.patch("ship", "radio", change)
end

local function panel(ctx, ui, radio)
    local name, level, ticker = "No signal", 0, "Fly nearer a station to pick up its radio."
    if not radio.on then
        name, ticker = "Radio off", "Press On or R to listen again."
    elseif radio.channel ~= "" then
        name, level = ctx.get(radio.channel, "channel").name, radio.level
        ticker = bits(ctx, radio.channel)[radio.bit] or ""
    end
    return ui.panel { id = "radio", align = "bottom-right", x = layout.M, y = layout.M, width = layout.RADIO, gap = 8, children = {
        ui.text { id = "radio_name", text = name, size = 20, height = 26 },
        ui.text { id = "radio_signal", text = signal.shown(level, radio.on), size = 16 },
        ui.text { id = "radio_ticker", text = ticker, size = 16, height = 44 },
        ui.row { id = "radio_buttons", gap = 8, children = {
            ui.button { id = "radio_prev", text = "Prev", width = 56, height = 44, pad = 6, on = "radio", payload = { tune = "prev" } },
            ui.button { id = "radio_toggle", text = radio.on and "Off" or "On", width = 48, height = 44, pad = 6, on = "radio", payload = { tune = "toggle" } },
            ui.button { id = "radio_next", text = "Next · R", width = 96, height = 44, pad = 6, on = "radio", payload = { tune = "next" } },
        } },
    } }
end

return {
    on = {
        radio = tune,
        -- Docked, the radio falls quiet under the station's own sound.
        docked = function(_, ctx)
            if not ctx.has("ship", "radio") then return end
            ctx.patch("ship", "radio", cue(ctx, ctx.get("ship", "radio"), ""))
        end,
        -- A hot load stops all music, and a resumed save starts with none.
        ["engine.hot_loaded"] = replay,
        ["engine.resumed"] = replay,
    },
    cadence = function(ctx)
        -- A save from before the radio has none, and plays without it.
        if not ctx.has("ship", "radio") then return end
        if flying(ctx) then second(ctx) end
    end,
    screen = function(ctx, ui)
        local radio = ctx.get("ship", "radio")
        if not radio or not flying(ctx) or ctx.get("ship", "pilot").log_open then return nil end
        return panel(ctx, ui, radio)
    end,
}
