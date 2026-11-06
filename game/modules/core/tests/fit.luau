local fit = require("../scripts/fit")
local rules = require("../scripts/rules")

-- The rack as start.toml stocks it.
local function rack()
    return {
        { id = "core.lens", name = "Cutting lens", socket = "drill", price = 3000, power = 2, mass = 1, hold = 0, mine = 1, speed = 0, owned = false, fitted = false },
        { id = "core.pods", name = "Hold pods", socket = "hold", price = 3500, power = 0, mass = 3, hold = 12, mine = 0, speed = 0, owned = false, fitted = false },
        { id = "core.thrusters", name = "Tuned thrusters", socket = "engine", price = 6000, power = 2, mass = 1, hold = 0, mine = 0, speed = 5, owned = false, fitted = false },
    }
end

return {
    bare_hull = function()
        local t = fit.totals(rack())
        assert(t.hold == 24 and t.mine == 1 and t.power == 0 and t.mass == 0)
        assert(t.top == 20 and t.turn == 180, "the bare hull flies as before")
    end,
    parts_add_up = function()
        local parts = rack()
        parts[1].fitted, parts[2].fitted = true, true
        local t = fit.totals(parts)
        assert(t.hold == 36 and t.mine == 2 and t.power == 2 and t.mass == 4)
        -- One unit over the free mass: a tenth slower, and turns a tenth wider.
        assert(t.over == 1 and math.abs(t.top - 18) < 1e-9 and math.abs(t.turn - 162) < 1e-9)
        -- Within the free mass, nothing is lost.
        parts[1].fitted = false
        local pods = fit.totals(parts)
        assert(pods.over == 0 and pods.top == 20 and pods.turn == 180)
        -- Never below half, however heavy.
        assert(fit.totals({ { id = "x", fitted = true, power = 0, mass = 20, hold = 0, mine = 0, speed = 0 } }).factor == 0.5)
    end,
    an_old_save_keeps_its_hull = function()
        -- A save from before the locked flight loads with the old hull's 12 (its `fit`
        -- gets the field's default), and the cradle goes on from what the ship flies.
        -- A save from before the opening hour's retune keeps its hold of 6 the same way.
        local parts = rack()
        parts[3].speed = 3 -- such a save keeps its thrusters as they were: 12 to 15
        parts[2].hold = 3 -- and its pods: 6 to 9
        assert(fit.totals(parts, nil, nil, fit.OLD_SPEED, fit.OLD_HOLD).top == 12, "the old bare hull")
        parts[1].owned, parts[1].fitted = true, true
        local lens = fit.totals(parts, nil, nil, fit.OLD_SPEED, fit.OLD_HOLD)
        assert(lens.top == 12 and lens.turn == 180, "the lens alone is within the free mass")
        assert(fit.ship_text(lens) == "Hold 6 · mines 2 a second · top speed 12 · turns 180° a second")
        -- Taking the lens off changes only the beam, never the flight.
        local off = fit.totals(parts, "core.lens", nil, fit.OLD_SPEED, fit.OLD_HOLD)
        assert(off.top == 12 and off.turn == 180 and off.mine == 1)
        -- The lens and pods: one over, as the old cradle said and the old ship flew.
        parts[2].owned, parts[2].fitted = true, true
        local both = fit.totals(parts, nil, nil, fit.OLD_SPEED, fit.OLD_HOLD)
        assert(math.abs(both.top - 10.8) < 1e-9 and math.abs(both.turn - 162) < 1e-9)
        assert(fit.ship_text(both) == "Hold 9 · mines 2 a second · top speed 10.8 · turns 162° a second")
        -- The old thrusters on the old bare hull: 15, as their card says.
        parts[1].fitted, parts[2].fitted, parts[3].fitted = false, false, true
        assert(fit.totals(parts, nil, nil, fit.OLD_SPEED, fit.OLD_HOLD).top == 15)
    end,
    every_rule_refuses_with_its_reason = function()
        local parts = rack()
        local lens, pods, thrusters = parts[1], parts[2], parts[3]
        -- The wrong socket names the right one.
        assert(fit.verdict(lens, "engine", parts, 500, 0) == "it goes in the drill mount")
        -- Too few credits says what it costs and what you have.
        assert(fit.verdict(lens, "drill", parts, 40, 0) == "it costs 3000 cr; you have 40")
        assert(fit.verdict(lens, "drill", parts, 3000, 0) == "")
        -- Power says what it needs and what is free, before what it costs.
        lens.owned, lens.fitted = true, true
        assert(fit.verdict(thrusters, "engine", parts, 0, 0) == "it needs 2 power; 1 is free")
        assert(fit.verdict(lens, "drill", parts, 0, 0) == "it's fitted here")
        -- An owned part in the locker costs nothing to fit again.
        pods.owned = true
        assert(fit.verdict(pods, "hold", parts, 0, 0) == "")
        -- Taking off: only what's fitted, and only if the cargo still fits.
        assert(fit.verdict(pods, "locker", parts, 0, 0) == "it isn't fitted")
        pods.fitted = true
        assert(fit.verdict(pods, "locker", parts, 0, 36) == "you carry 36; the hold would take 24")
        assert(fit.verdict(pods, "locker", parts, 0, 24) == "")
        assert(fit.verdict(lens, "locker", parts, 0, 36) == "")
        -- A save from before the retune weighs the cargo against its own small hull.
        assert(fit.verdict(pods, "locker", parts, 0, 9, fit.OLD_HOLD) == "you carry 9; the hold would take 6")
    end,
    budgets_in_plain_words = function()
        local parts = rack()
        assert(fit.power_text(fit.totals(parts)) == "Power 0 of 3 in use, 3 free")
        assert(fit.mass_text(fit.totals(parts)) == "Mass 0: the hull carries 3 freely, 3 more before it slows you")
        parts[1].fitted, parts[2].fitted = true, true
        local t = fit.totals(parts)
        assert(fit.mass_text(t) == "Mass 4: the hull carries 3 freely. 1 over: 10% slower, wider turns")
        assert(fit.ship_text(t) == "Hold 36 · mines 2 a second · top speed 18 · turns 162° a second")
        assert(fit.effect(parts[1]) == "mines 2 a second" and fit.cost(parts[1]) == "power 2 · mass 1")
        assert(fit.effect(parts[3]) == "top speed +5" and fit.cost(parts[2]) == "power 0 · mass 3")
    end,
    an_earned_control_waits_on_the_work = function()
        -- The fitter fits a control only once the pilot has cut its full holds, and says
        -- so in his own words before he says what it costs; once owned it is never asked.
        local rig = { id = "core.rig", name = "Reach rig", socket = "rack", price = 15000, holds = 80, power = 0, mass = 0,
            hold = 0, mine = 0, speed = 0, owned = false, fitted = false, brought = 12 }
        local parts = { rig }
        assert(fit.verdict(rig, "rack", parts, 99999, 0) == "Orrin: \"Bring in 80 full holds and I'll fit it.\" You've brought 12")
        rig.brought = 80
        assert(fit.verdict(rig, "rack", parts, 100, 0) == "it costs 15000 cr; you have 100")
        assert(fit.verdict(rig, "rack", parts, 15000, 0) == "")
        rig.owned, rig.brought = true, 0
        assert(fit.verdict(rig, "rack", parts, 0, 0) == "", "a control already bought fits for nothing")
    end,
    the_lens_mines_faster = function()
        assert(rules.transfer(0, 12, 6, 2) == 2)
        assert(rules.transfer(5, 12, 6, 2) == 1, "never past the hold")
        assert(rules.transfer(0, 1, 6, 2) == 1, "never past the rock")
        assert(rules.transfer(6, 12, 9, 1) == 1, "the pods' room")
    end,
}
