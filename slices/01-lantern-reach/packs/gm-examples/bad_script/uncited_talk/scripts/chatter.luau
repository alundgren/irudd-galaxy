--!strict
local G = require("@galaxy")

return G.script({
	id = "chatter",
	on = {
		dock = function(ev: G.DockEvent, ctx: G.Ctx)
			local why = ctx:cause({ summary = "small talk" })
			ctx.talk:offer({ speaker = "npc.ilse", place = ev.station, text = "Nice day for it." }, why)
		end,
	},
})
