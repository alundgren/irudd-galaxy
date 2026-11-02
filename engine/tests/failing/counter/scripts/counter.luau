--!strict
-- Each bump adds sums.step() to the board's count.

local sums = require("./sums")

return {
	on = {
		bump = function(event, ctx)
			local count = ctx.get("board", "count")
			if count then
				ctx.patch("board", "count", { n = sums.add(count.n, sums.step()) })
			end
		end,
	},
}
