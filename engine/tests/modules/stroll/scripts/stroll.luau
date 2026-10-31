--!strict
-- Walkers count their steps once a second, step onto the deck (layer 1) when the ramp
-- has lifted them to its height, and turn around when told to.

local rules = require("./rules")

return {
	on = {
		["engine.start"] = function(event, ctx)
			ctx.record("started", { walkers = #ctx.query("walker") })
		end,
		turn = function(event, ctx)
			local walker = event.payload.walker
			local motion = ctx.get(walker, "engine.motion")
			if motion then
				ctx.patch(walker, "engine.motion", { vx = -motion.vx })
			end
		end,
	},
	cadence = function(ctx)
		for _, id in ctx.query("walker") do
			local walker = ctx.get(id, "walker")
			local at = ctx.get(id, "engine.position")
			if walker and at then
				local steps = walker.steps + rules.STEP
				ctx.patch(id, "walker", { steps = steps })
				if rules.reached_deck(at) then
					ctx.patch(id, "engine.position", { layer = 1 })
					ctx.patch(id, "walker", { steps = steps, climbs = walker.climbs + 1 })
					ctx.record("climbed", { walker = id, x = at.x, height = at.height })
				end
				if steps % rules.PACED_EVERY == 0 then
					ctx.record("paced", { walker = id, roll = ctx.random_int(1, 6), drift = ctx.random() })
				end
			end
		end
	end,
}
