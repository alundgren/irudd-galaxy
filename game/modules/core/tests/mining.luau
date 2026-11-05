-- Mining by hand: the cutter's reach and aim, how beam time becomes ore, a berth's ring,
-- fresh stone after a visit, and what the pilot hears when no rock has ore.
local rules = require("../scripts/rules")

local ROCK = { x = 12, y = 0 }

return {
    reach_is_to_the_surface = function()
        local r = rules.rock_radius(1, 0)
        assert(r == rules.ROCK_RADIUS)
        assert(rules.rock_radius(2, 0) == 2 * rules.ROCK_RADIUS, "a bigger rock reaches further")
        assert(rules.rock_radius(1, 1) < r, "a worked rock is smaller")
        local edge = 12 - r
        assert(math.abs(rules.gap({ x = edge - 3, y = 0 }, ROCK, r) - 3) < 1e-9)
        assert(rules.cuttable({ x = edge - rules.REACH, y = 0, angle = 0 }, ROCK, r, rules.REACH))
        assert(not rules.cuttable({ x = edge - rules.REACH - 0.1, y = 0, angle = 0 }, ROCK, r, rules.REACH),
            "just out of reach")
        assert(rules.cuttable({ x = edge - 12, y = 0, angle = 0 }, ROCK, r, rules.RIG_REACH), "the rig reaches further")
    end,
    nose_must_be_on_the_rock = function()
        local r = rules.ROCK_RADIUS
        local at = { x = 6, y = 0, angle = 0 }
        assert(rules.off_aim(at, ROCK, r) == 0, "pointing at its centre")
        -- The rock spans about 27 degrees either side from 6 units off; within the slack
        -- past its edge still cuts, a quarter turn away doesn't.
        at.angle = math.rad(30)
        assert(rules.cuttable(at, ROCK, r, rules.REACH), "just past its edge")
        at.angle = math.rad(90)
        assert(rules.off_aim(at, ROCK, r) > 60)
        assert(not rules.cuttable(at, ROCK, r, rules.REACH), "a quarter turn off")
        assert(rules.cuttable(at, ROCK, r, rules.REACH, true), "a beam that aims itself")
        at.angle = math.rad(180)
        assert(not rules.cuttable(at, ROCK, r, rules.REACH), "facing away")
    end,
    beam_time_becomes_ore = function()
        -- One ore a second: the first comes loose half-way through the first second.
        local units, rest = rules.cut(14)
        assert(units == 0 and rest == 14)
        units, rest = rules.cut(15)
        assert(units == 1 and rest == -15, "the first unit at half a second")
        units, rest = rules.cut(-15 + 29)
        assert(units == 0 and rest == 14, "the next not before a whole second more")
        units = rules.cut(-15 + 30)
        assert(units == 1, "the next a whole second after the first")
        units, rest = rules.cut(45)
        assert(units == 2 and rest == -15)
        -- With the lens (two a second) the work doubles, never counted twice.
        units, rest = rules.cut(30 * 2)
        assert(units == 2 and rest == 0)
        -- However the beam time is broken up, a minute cuts sixty.
        local total, work = 0, 0
        for _ = 1, 120 do
            work += 15
            local u
            u, work = rules.cut(work)
            total += u
        end
        assert(total == 60, "half-second bursts add up to one a second")
    end,
    the_berth_ring_takes_a_slow_ship = function()
        local berth = { x = -14, y = -12 }
        assert(rules.in_ring({ x = -14, y = -12 }, berth, 0))
        assert(rules.in_ring({ x = -14 + rules.RING - 0.01, y = -12 }, berth, rules.RING_SPEED))
        assert(not rules.in_ring({ x = -14 + rules.RING + 0.01, y = -12 }, berth, 0), "outside the ring")
        assert(not rules.in_ring(berth, berth, rules.RING_SPEED + 1), "flying through it")
    end,
    worked_out_rocks_come_back = function()
        assert(rules.regrow(0, 12) == 6, "half its stone")
        assert(rules.regrow(5, 12) == 5, "a rock still holding ore is left as it is")
        assert(rules.regrow(0, 9) == 4)
    end,
    worked_out_says_what_you_carry = function()
        local empty = rules.worked_out(0, "nothing", {})
        assert(string.find(empty, "Your hold is empty", 1, true), empty)
        assert(not string.find(empty, "Your ore", 1, true), "never claims ore an empty hold doesn't carry")
        assert(string.find(empty, "fresh stone drifts in", 1, true), "says what comes next")
        local one = rules.worked_out(4, "4 ore", { "Cinder Kiln" })
        assert(string.find(one, "You carry 4 ore: Cinder Kiln buys it.", 1, true), one)
        local two = rules.worked_out(4, "4 ore", { "Lantern Dock", "Cinder Kiln" })
        assert(string.find(two, "Lantern Dock and Cinder Kiln buy it.", 1, true), two)
        local none = rules.worked_out(2, "2 rations", {})
        assert(string.find(none, "You carry 2 rations.", 1, true), none)
    end,
}
