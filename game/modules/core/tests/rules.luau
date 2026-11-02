local rules = require("../scripts/rules")
return {
    hold_capacity = function()
        assert(rules.transfer(5, 12) == 1)
        assert(rules.transfer(6, 12) == 0)
    end,
    depleted_rock = function()
        assert(rules.transfer(0, 0) == 0)
        assert(rules.transfer(0, 1) == 1)
    end,
    posted_prices = function()
        assert(rules.sale(6, 12) == 72)
        assert(rules.sale(6, 16) == 96)
        assert(rules.sale(0, 16) == 0)
    end,
}
