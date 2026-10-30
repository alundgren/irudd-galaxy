--!nocheck
local G = require("@galaxy")

local value: number = "this is a string"

return G.script({
	id = "unchecked",
	on = {
		dock = function(_ev: G.DockEvent, _ctx: G.Ctx)
			print(value)
		end,
	},
})
