-- The radio's rules (scripts/signal.luau): signal by distance, the shown level's
-- hysteresis, and tuning that never flickers between two stations.
local signal = require("../scripts/signal")

local ORDER = { "lantern", "kiln" }

-- Runs `seconds` of automatic tuning from `tuned` with strengths from `at(second)`, and
-- returns the channel each second and how many times it changed.
local function fly(tuned, seconds, at, hand)
    local lost, better, changes, seen = 0, 0, 0, {}
    for i = 1, seconds do
        local now
        now, lost, better = signal.tune(ORDER, at(i), tuned, lost, better, hand)
        if now ~= tuned then changes += 1 end
        tuned = now
        seen[i] = now
    end
    return seen, changes
end

return {
    strength_falls_off_with_distance = function()
        assert(signal.strength(0, 80) == 1, "full at the source")
        assert(signal.strength(24, 80) == 1, "full within the inner 30% of the range")
        assert(math.abs(signal.strength(52, 80) - 0.5) < 1e-9, "half way across the rest")
        assert(signal.strength(80, 80) == 0, "nothing at the edge")
        assert(signal.strength(200, 80) == 0, "nothing beyond it")
    end,

    words_and_bars_share_one_level = function()
        assert(#signal.WORDS == #signal.LEVELS, "a word for every level")
        assert(signal.WORDS[#signal.WORDS] == "Strong", "the strongest level is Strong, all bars")
        assert(signal.raw(1) == 3 and signal.raw(0.5) == 2 and signal.raw(0.2) == 1, "Strong, Fair, Faint")
        assert(signal.raw(0) == 0, "nothing in range")
        for level, word in signal.WORDS do
            local bars = signal.shown(level, true)
            local lit = select(2, string.gsub(bars, "|", ""))
            assert(lit == level, word .. " lights " .. level .. " bars, got " .. lit)
            assert(string.find(bars, word, 1, true), "the bars sit beside their word")
        end
        assert(signal.shown(3, true) == "|||  Strong", "Strong is every bar")
        assert(signal.shown(0, true) == "···  None" and signal.shown(2, false) == "···  Off", "no bars with no signal or off")
    end,

    a_fresh_channel_shows_its_level_plainly = function()
        assert(signal.level(0, 0.9) == 3, "Strong")
        assert(signal.level(0, 0.5) == 2, "Fair")
        assert(signal.level(0, 0.1) == 1, "Faint")
        assert(signal.level(0, 0) == 1, "a tuned channel shows at least Faint")
    end,

    the_level_never_flickers_on_a_threshold = function()
        -- Wavering either side of Strong's threshold keeps whichever level it had.
        local strong, fair = 3, 2
        for i = 1, 40 do
            local s = 0.7 + (i % 2 == 0 and 0.03 or -0.03)
            strong = signal.level(strong, s)
            fair = signal.level(fair, s)
            assert(strong == 3, "Strong holds while it wavers on the threshold")
            assert(fair == 2, "Fair holds while it wavers on the threshold")
        end
        assert(signal.level(2, 0.76) == 3, "clearly past it: Strong")
        assert(signal.level(3, 0.64) == 2, "clearly below it: Fair")
        assert(signal.level(3, 0.1) == 1, "a fall steps down as far as it goes")
        assert(signal.level(1, 0) == 1, "never below Faint while tuned")
    end,

    two_even_stations_never_flicker = function()
        -- Over the field both channels come in about evenly, swapping the lead every
        -- second: the radio keeps the one it has for a whole minute.
        local seen, changes = fly("lantern", 60, function(i)
            local lead = i % 2 == 0 and 0.12 or -0.12
            return { lantern = 0.7 - lead, kiln = 0.7 + lead }
        end)
        assert(changes == 0, "no switch, got " .. changes)
        assert(seen[60] == "lantern", "still Lantern Dock")
    end,

    a_clearly_stronger_station_must_be_steady = function()
        -- Clearly stronger for two seconds, then not: no switch.
        local _, changes = fly("lantern", 10, function(i)
            local ahead = (i % 3 ~= 0) and 0.3 or 0
            return { lantern = 0.6, kiln = 0.6 + ahead }
        end)
        assert(changes == 0, "a lead that keeps breaking never takes over")
        -- Clearly stronger every second: it takes over once, on the third.
        local seen, once = fly("lantern", 10, function() return { lantern = 0.6, kiln = 0.9 } end)
        assert(once == 1, "one switch, got " .. once)
        assert(seen[2] == "lantern" and seen[3] == "kiln" and seen[10] == "kiln", "taken on the third second and kept")
    end,

    a_brief_gap_keeps_the_channel = function()
        -- Out of every range for four seconds: the radio keeps its channel.
        local seen, changes = fly("lantern", 8, function(i)
            if i <= 4 then return { lantern = 0, kiln = 0 } end
            return { lantern = 0.4, kiln = 0 }
        end)
        assert(changes == 0 and seen[8] == "lantern", "kept through the gap")
        -- Out of range for good: let go on the fifth second.
        local lost, gone = fly("lantern", 6, function() return { lantern = 0, kiln = 0 } end)
        assert(gone == 1 and lost[4] == "lantern" and lost[5] == "", "No signal only after the grace")
        -- Lost, with another in range: the other is taken.
        local other = fly("lantern", 6, function() return { lantern = 0, kiln = 0.3 } end)
        assert(other[5] == "kiln", "the other station after the grace")
    end,

    a_chosen_channel_is_never_outclassed = function()
        local seen, changes = fly("kiln", 30, function() return { lantern = 1, kiln = 0.2 } end, true)
        assert(changes == 0 and seen[30] == "kiln", "the player's choice holds while in range")
        local lost = fly("kiln", 6, function() return { lantern = 1, kiln = 0 } end, true)
        assert(lost[4] == "kiln" and lost[5] == "lantern", "and is let go only once it is lost")
    end,

    no_signal_tunes_the_strongest = function()
        local seen = fly("", 1, function() return { lantern = 0.3, kiln = 0.5 } end)
        assert(seen[1] == "kiln", "the strongest in range")
        local none = fly("", 1, function() return { lantern = 0, kiln = 0 } end)
        assert(none[1] == "", "nothing in range")
    end,

    stepping_cycles_through_the_choices = function()
        local choices = { "lantern", "kiln", "off" }
        assert(signal.step(choices, "lantern", 1) == "kiln", "Next")
        assert(signal.step(choices, "kiln", 1) == "off", "Next past the last: Off")
        assert(signal.step(choices, "off", 1) == "lantern", "Next from Off: the first")
        assert(signal.step(choices, "lantern", -1) == "off", "Prev from the first: Off")
        assert(signal.step(choices, "kiln", -1) == "lantern", "Prev")
        assert(signal.step(choices, "gone", 1) == "lantern", "a channel out of range steps to the first")
        assert(signal.step(choices, "gone", -1) == "off", "or back to Off")
    end,
}
