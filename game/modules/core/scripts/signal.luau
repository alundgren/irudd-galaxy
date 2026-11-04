-- The radio's rules, with no world access, so tests can call them directly. The radio
-- (radio.luau) reads them once a second; nothing about them is timed by frames.
local signal = {
    -- The share of a channel's range, from its source, at full signal.
    NEAR = 0.3,
    -- Another channel takes over only when it is this much stronger, every second for
    -- STEADY seconds running.
    MARGIN = 0.25,
    STEADY = 3,
    -- The tuned channel is kept this many seconds out of range before it is let go, so
    -- a brief gap never flashes "No signal".
    GRACE = 5,
    -- Seconds each line shows on the ticker before the next.
    BIT_SECONDS = 12,
    -- Where each shown level starts: Faint above nothing, Fair, Strong. A level only
    -- changes once the signal is BAND past its threshold, so it never flickers on one.
    LEVELS = { 0, 0.35, 0.7 },
    BAND = 0.05,
    WORDS = { "Faint", "Fair", "Strong" },
    -- One lit bar per level, so the bars always match the word: Strong is all of them.
    BARS = { "|··", "||·", "|||" },
}

-- How many bars are lit at `level`: none when nothing is tuned or the radio is off.
function signal.lit(level, on)
    if not on or level < 1 then return 0 end
    return level
end

-- The word beside the bars: the level's, "None" with nothing tuned, or "Off". Bars and
-- word both come from `level`, so they always agree.
function signal.word(level, on)
    if not on then return "Off" end
    if level < 1 then return "None" end
    return signal.WORDS[level]
end

-- The signal as text: bars and a word for `level`; empty bars with "None" when nothing
-- is tuned, or "Off".
function signal.shown(level, on)
    local lit = signal.lit(level, on)
    local bars = lit > 0 and signal.BARS[lit] or "···"
    return bars .. "  " .. signal.word(level, on)
end

-- The signal from 0 (out of range) to 1 at `distance` from a source of `range`.
function signal.strength(distance, range)
    local near = signal.NEAR * range
    if distance <= near then return 1 end
    if distance >= range then return 0 end
    return 1 - (distance - near) / (range - near)
end

-- The level `strength` shows plainly: 0 for nothing, else 1 to 3.
function signal.raw(strength)
    local level = 0
    for i, at in signal.LEVELS do
        if strength > at then level = i end
    end
    return level
end

-- The level shown now, from the level shown before (`was`, 0 for a fresh channel) and
-- the strength: it moves only once the strength is clearly past a threshold. A tuned
-- channel never shows less than Faint; the radio lets it go after its grace instead.
function signal.level(was, strength)
    if was < 1 then return math.max(1, signal.raw(strength)) end
    local level = was
    while level < #signal.LEVELS and strength >= signal.LEVELS[level + 1] + signal.BAND do
        level += 1
    end
    while level > 1 and strength < signal.LEVELS[level] - signal.BAND do
        level -= 1
    end
    return level
end

-- The strongest channel in `strengths` ({ [id] = strength }) above nothing, in
-- `order` (a list of ids) for ties; nil when none is in range.
function signal.strongest(order, strengths)
    local best, found = 0, nil
    for _, id in order do
        if strengths[id] > best then best, found = strengths[id], id end
    end
    return found
end

-- One second of automatic tuning. `tuned` is the channel ("" for none), `lost` and
-- `better` its counted seconds out of range and outclassed. Returns the channel kept
-- or taken, and the new counts. A hand-tuned channel (`hand`) is never outclassed,
-- only lost.
function signal.tune(order, strengths, tuned, lost, better, hand)
    if tuned ~= "" then
        local now = strengths[tuned] or 0
        lost = now > 0 and 0 or lost + 1
        if lost < signal.GRACE then
            local best = signal.strongest(order, strengths)
            if hand or not best or best == tuned or strengths[best] - now < signal.MARGIN then
                return tuned, lost, 0
            end
            better += 1
            if better < signal.STEADY then return tuned, lost, better end
            return best, 0, 0
        end
    end
    return signal.strongest(order, strengths) or "", 0, 0
end

-- The next choice when stepping `by` (1 or -1) through `choices` from `current`;
-- `current` missing from them steps from before the first.
function signal.step(choices, current, by)
    local at = 0
    for i, id in choices do
        if id == current then at = i end
    end
    if at == 0 and by < 0 then at = 1 end
    return choices[(at - 1 + by) % #choices + 1]
end

return signal
