local rules = require("../scripts/rules")
return {
    three_cups_complete_growth = function()
        local water = 0
        for _ = 1, 3 do water = rules.water(water) end
        assert(water == rules.GOAL, "three cups must bloom")
        assert(rules.scale(water) == 1, "bloom reaches full size")
    end,
    watering_a_bloom_does_not_overgrow = function()
        assert(rules.water(3) == 3, "additional cups must not exceed the goal")
        assert(rules.scale(0) < rules.scale(1), "first cup visibly grows the seed")
    end,
}
