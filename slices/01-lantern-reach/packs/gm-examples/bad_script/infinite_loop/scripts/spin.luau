--!strict
local G = require("@galaxy")

return G.script({
	id = "spin",
	on = {
		dock = function(ev: G.DockEvent, ctx: G.Ctx)
			local why = ctx:cause({ summary = "this never applies", cites = { ev.fact } })
			ctx.stations:set_stage(ev.station, 9, why)
			local n = 0
			while n >= 0 do
				n += 1
			end
		end,
	},
})
