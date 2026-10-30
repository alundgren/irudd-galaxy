--!strict
local G = require("@galaxy")

return G.script({
	id = "later",
	after = {
		half_minute = G.after({ seconds = 30 } :: any, function(_ctx: G.Ctx, _fire: G.TriggerFire) end),
	},
})
