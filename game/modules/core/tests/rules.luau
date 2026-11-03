local rules = require("../scripts/rules")
local teach = require("../scripts/teach")
local voices = require("../scripts/voices")
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
    hints_retire = function()
        assert(teach.new({ mine = 0 }, "mine"))
        assert(teach.new({ mine = teach.USES - 1 }, "mine"))
        assert(not teach.new({ mine = teach.USES }, "mine"))
    end,
    keepers_follow_the_shift = function()
        assert(voices.trader(6, 0) == "I'll take your 6 ore whenever you like.")
        assert(voices.trader(0, 72) == "Thanks. Your ore's on the pile already.")
        assert(voices.trader(0, 0) == "Empty hold? The rocks are just outside.")
        assert(voices.master(30) == "The field still holds 30 ore.")
        assert(voices.master(0) == "The field's worked out. Good shift.")
    end,
}
