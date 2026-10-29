--!strict
local G = require("@galaxy")

return G.script({
	id = "tally",
	on = {
		dock = function(ev: G.DockEvent, _ctx: G.Ctx)
			local seen: { [string]: boolean } = { [ev.station] = true }
			for station in seen do
				print(station)
			end
		end,
	},
})
