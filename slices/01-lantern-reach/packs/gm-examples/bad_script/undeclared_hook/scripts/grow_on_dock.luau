--!strict
local G = require("@galaxy")

return G.script({
	id = "grow_on_dock",
	on = {
		dock = function(ev: G.DockEvent, ctx: G.Ctx)
			local stage = ctx.stations:stage(ev.station)
			local why = ctx:cause({ summary = "grow on every docking", cites = { ev.fact } })
			ctx.stations:set_stage(ev.station, stage + 1, why)
			ctx.talk:offer({ place = ev.station, text = "It grew.", cites = { ev.fact } }, why)
		end,
	},
})
