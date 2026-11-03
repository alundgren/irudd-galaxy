local fit = require("../scripts/fit")
local rules = require("../scripts/rules")

-- The rack as start.toml stocks it.
local function rack()
    return {
        { id = "core.lens", name = "Cutting lens", socket = "drill", price = 90, power = 2, mass = 1, hold = 0, mine = 1, speed = 0, owned = false, fitted = false },
        { id = "core.pods", name = "Hold pods", socket = "hold", price = 70, power = 0, mass = 3, hold = 3, mine = 0, speed = 0, owned = false, fitted = false },
        { id = "core.thrusters", name = "Tuned thrusters", socket = "engine", price = 110, power = 2, mass = 1, hold = 0, mine = 0, speed = 5, owned = false, fitted = false },
    }
end

return {
    bare_hull = function()
        local t = fit.totals(rack())
        assert(t.hold == 6 and t.mine == 1 and t.power == 0 and t.mass == 0)
        assert(t.top == 20 and t.turn == 180, "the bare hull flies as before")
    end,
    parts_add_up = function()
        local parts = rack()
        parts[1].fitted, parts[2].fitted = true, true
        local t = fit.totals(parts)
        assert(t.hold == 9 and t.mine == 2 and t.power == 2 and t.mass == 4)
        -- One unit over the free mass: a tenth slower, and turns a tenth wider.
        assert(t.over == 1 and math.abs(t.top - 18) < 1e-9 and math.abs(t.turn - 162) < 1e-9)
        -- Within the free mass, nothing is lost.
        parts[1].fitted = false
        local pods = fit.totals(parts)
        assert(pods.over == 0 and pods.top == 20 and pods.turn == 180)
        -- Never below half, however heavy.
        assert(fit.totals({ { id = "x", fitted = true, power = 0, mass = 20, hold = 0, mine = 0, speed = 0 } }).factor == 0.5)
    end,
    every_rule_refuses_with_its_reason = function()
        local parts = rack()
        local lens, pods, thrusters = parts[1], parts[2], parts[3]
        -- The wrong socket names the right one.
        assert(fit.verdict(lens, "engine", parts, 500, 0) == "it goes in the drill mount")
        -- Too few credits says what it costs and what you have.
        assert(fit.verdict(lens, "drill", parts, 40, 0) == "it costs 90 cr; you have 40")
        assert(fit.verdict(lens, "drill", parts, 90, 0) == "")
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
        assert(fit.verdict(pods, "locker", parts, 0, 9) == "you carry 9; the hold would take 6")
        assert(fit.verdict(pods, "locker", parts, 0, 6) == "")
        assert(fit.verdict(lens, "locker", parts, 0, 9) == "")
    end,
    budgets_in_plain_words = function()
        local parts = rack()
        assert(fit.power_text(fit.totals(parts)) == "Power 0 of 3 in use, 3 free")
        assert(fit.mass_text(fit.totals(parts)) == "Mass 0: the hull carries 3 freely, 3 more before it slows you")
        parts[1].fitted, parts[2].fitted = true, true
        local t = fit.totals(parts)
        assert(fit.mass_text(t) == "Mass 4: the hull carries 3 freely. 1 over: 10% slower, wider turns")
        assert(fit.ship_text(t) == "Hold 9 · mines 2 a second · top speed 18 · turns 162° a second")
        assert(fit.effect(parts[1]) == "mines 2 a second" and fit.cost(parts[1]) == "power 2 · mass 1")
        assert(fit.effect(parts[3]) == "top speed +5" and fit.cost(parts[2]) == "power 0 · mass 3")
    end,
    the_lens_mines_faster = function()
        assert(rules.transfer(0, 12, 6, 2) == 2)
        assert(rules.transfer(5, 12, 6, 2) == 1, "never past the hold")
        assert(rules.transfer(0, 1, 6, 2) == 1, "never past the rock")
        assert(rules.transfer(6, 12, 9, 1) == 1, "the pods' room")
    end,
}
