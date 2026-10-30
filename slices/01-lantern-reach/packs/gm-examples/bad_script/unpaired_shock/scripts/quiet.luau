--!strict
local G = require("@galaxy")

return G.script({
	id = "quiet",
	on = {
		dock = function(_ev: G.DockEvent, _ctx: G.Ctx) end,
	},
})
