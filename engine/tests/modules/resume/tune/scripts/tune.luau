--!strict
-- `play` starts the theme and remembers it is playing. A resumed save plays no earlier
-- cue again, so `engine.resumed` cues the theme once more when it was playing.

return {
	on = {
		play = function(event, ctx)
			ctx.patch("box", "state", { playing = true })
			ctx.record("theme", {})
		end,
		["engine.resumed"] = function(event, ctx)
			print("resumed on tick " .. event.tick .. " from " .. tostring(event.source))
			local state = ctx.get("box", "state")
			if state and state.playing then
				ctx.record("theme", {})
			end
		end,
	},
}
