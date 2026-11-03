local rules = require("../scripts/rules")
local teach = require("../scripts/teach")
local voices = require("../scripts/voices")

local ORE = { role = "buys", base = 12, target = 30, stock = 24, rate = 3 }
local PLATE = { role = "sells", base = 28, target = 24, stock = 24, rate = 2 }

return {
    hold_capacity = function()
        assert(rules.transfer(5, 12) == 1)
        assert(rules.transfer(6, 12) == 0)
        assert(rules.load({ ore = 2, plate = 3, rations = 1 }) == 6)
        assert(rules.list({ ore = 4, plate = 2, rations = 0 }) == "4 ore, 2 plate")
        assert(rules.list({ ore = 0, plate = 0, rations = 0 }) == "nothing")
    end,
    depleted_rock = function()
        assert(rules.transfer(0, 0) == 0)
        assert(rules.transfer(0, 1) == 1)
    end,
    prices_follow_stock = function()
        -- Short of target pays more; at target pays base; over target pays less.
        assert(rules.price(ORE) == 13)
        assert(rules.price({ base = 12, target = 30, stock = 30 }) == 12)
        assert(rules.price({ base = 12, target = 30, stock = 36 }) == 11)
        -- Within 70% to 130% of base, however far the stock strays.
        assert(rules.price({ base = 20, target = 10, stock = 0 }) == 26)
        assert(rules.price({ base = 20, target = 10, stock = 100 }) == 14)
    end,
    selling_walks_the_price_down = function()
        local prices, stock = rules.walk(ORE, 6)
        assert(stock == 30)
        assert(prices[1] == 13 and prices[6] == 12)
        assert(rules.sale(ORE, 6) == rules.total(prices))
        -- Splitting a sale never pays more than selling at once.
        local first, after = rules.walk(ORE, 3)
        local rest = rules.walk({ role = "buys", base = 12, target = 30, stock = after }, 3)
        assert(rules.total(first) + rules.total(rest) == rules.sale(ORE, 6))
        assert(rules.sale(ORE, 0) == 0)
    end,
    buying_walks_the_price_up = function()
        local prices, stock = rules.walk(PLATE, 6)
        assert(stock == 18)
        assert(prices[1] == 28 and prices[6] > prices[1])
    end,
    buying_is_bounded = function()
        -- By the room in the hold, the stock and the credits on hand.
        assert(rules.affordable(PLATE, 0, 1000) == 0)
        assert(rules.affordable(PLATE, 6, 1000) == 6)
        assert(rules.affordable({ role = "sells", base = 28, target = 24, stock = 2 }, 6, 1000) == 2)
        assert(rules.affordable(PLATE, 6, 27) == 0)
        assert(rules.affordable(PLATE, 6, 60) == 2)
    end,
    visits_settle_the_market = function()
        assert(rules.settle({ target = 30, stock = 24, rate = 3 }) == 27)
        assert(rules.settle({ target = 30, stock = 29, rate = 3 }) == 30)
        assert(rules.settle({ target = 30, stock = 36, rate = 3 }) == 33)
        assert(rules.settle({ target = 30, stock = 30, rate = 3 }) == 30)
    end,
    quick_taps_are_not_visits = function()
        assert(rules.counts("", false, "core.lantern"))
        assert(not rules.counts("core.lantern", false, "core.lantern"))
        assert(rules.counts("core.lantern", true, "core.lantern"))
        assert(rules.counts("core.lantern", false, "core.kiln"))
    end,
    hints_retire = function()
        assert(teach.new({ mine = 0 }, "mine"))
        assert(teach.new({ mine = teach.USES - 1 }, "mine"))
        assert(not teach.new({ mine = teach.USES }, "mine"))
    end,
    keepers_follow_the_shift = function()
        assert(voices.trader("6 ore", 0, "Idle.") == "I'll take your 6 ore whenever you like.")
        assert(voices.trader("", 72, "Idle.") == "Thanks. It's all on the pile already.")
        assert(voices.trader("", 0, "Idle.") == "Idle.")
        assert(voices.master(30, 12, "plate", "galley") == "The galley still needs 12 plate.")
        assert(voices.master(30, 0, "", "") == "The field still holds 30 ore.")
        assert(voices.master(0, 0, "", "") == "The field's worked out. Good shift.")
    end,
}
