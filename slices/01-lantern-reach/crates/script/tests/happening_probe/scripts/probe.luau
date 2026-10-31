--!strict
-- At the first docking at the Wharf, writes a happening there that waits for that
-- docking's fact (it exists, so the next docking there starts it).
local G = require("@galaxy")

local WHARF = "station.lantern_wharf"
local ID = "gm.happening_probe.crowd"

return G.script({
	id = "probe",
	on = {
		dock = function(ev: G.DockEvent, ctx: G.Ctx)
			if ev.station ~= WHARF or ctx.happenings:get(ID) ~= nil then
				return
			end
			local why = ctx:cause({ summary = "a crowd gathers at the Wharf", cites = { ev.fact } })
			ctx.happenings:put({
				id = ID,
				station = WHARF,
				show = { "crowd:quay.west" },
				lasts = { dockings = 1 },
				cause = { facts = { { id = ev.fact } } },
				line = {
					id = "line.ilse_marrow.probe_crowd",
					speaker = "npc.ilse_marrow",
					text = "There's a crowd on the quay tonight. I've no idea why.",
				},
			}, why)
		end,
	},
})
