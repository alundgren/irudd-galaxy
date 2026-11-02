--!strict
-- One test passes and one fails: sums.step() is 2.
local sums = require("../scripts/sums")

return {
	adds = function()
		assert(sums.add(2, 3) == 5, "2 + 3 is 5")
	end,

	a_bump_adds_one = function()
		assert(sums.step() == 1, "a bump adds one")
	end,
}
