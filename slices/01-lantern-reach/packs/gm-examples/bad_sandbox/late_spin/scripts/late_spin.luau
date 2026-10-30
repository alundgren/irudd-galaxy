--!strict
local G = require("@galaxy")

return G.script({
	id = "late_spin",
	on = {
		dock = function(_ev: G.DockEvent, ctx: G.Ctx)
			if ctx.counters:get("dockings") < 2 then
				return
			end
			local n = 0
			while n >= 0 do
				n += 1
			end
		end,
	},
})
