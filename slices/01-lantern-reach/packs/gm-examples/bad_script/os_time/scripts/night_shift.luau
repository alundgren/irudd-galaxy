--!strict
local G = require("@galaxy")

return G.script({
	id = "night_shift",
	on = {
		dock = function(ev: G.DockEvent, ctx: G.Ctx)
			local hour = (os.time() // 3600) % 24
			if hour < 6 then
				local why = ctx:cause({ summary = "night shift", cites = { ev.fact } })
				ctx.stations:set_stage(ev.station, 1, why)
			end
		end,
	},
})
