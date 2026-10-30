--!strict
local G = require("@galaxy")

local KETTLE = "station.kettle_dock"

return G.script({
	id = "silent_owner",
	on = {
		dock = function(ev: G.DockEvent, ctx: G.Ctx)
			if ev.station ~= KETTLE then
				return
			end
			local why = ctx:cause({ summary = "Kettle Dock changes hands, quietly", cites = { ev.fact } })
			ctx.stations:set_owner(KETTLE, "faction.tollway_compact", why)
		end,
	},
})
