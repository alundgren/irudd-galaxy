--!strict
-- Counts stroll's climbs and turns.

return {
	on = {
		["stroll.climbed"] = function(event, ctx)
			local count = ctx.get("board", "count")
			if count then
				ctx.patch("board", "count", { climbs = count.climbs + 1, last = event.payload.walker })
			end
		end,
		["stroll.turn"] = function(event, ctx)
			local count = ctx.get("board", "count")
			if count then
				ctx.patch("board", "count", { turns = count.turns + 1 })
			end
		end,
	},
}
