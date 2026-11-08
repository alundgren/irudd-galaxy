--!strict
-- The chart: a ui.map the engine paints every frame. This screen runs again only when the
-- chart is opened or closed, never because the ship moved.

local function held(event): boolean
	return event.payload.down == true
end

local function helm(field: string, value: number)
	return function(event, ctx)
		local v = if held(event) then value else 0
		ctx.patch("ship", "engine.helm", { [field] = v })
		if held(event) then
			-- The hands take over from the route.
			ctx.patch("ship", "engine.steer", { mode = "" })
		end
	end
end

return {
	on = {
		chart = function(event, ctx)
			if not held(event) then
				return
			end
			local chart = ctx.get("ship", "chart")
			if chart then
				ctx.patch("ship", "chart", { open = not chart.open })
			end
		end,
		thrust_ahead = helm("thrust", 1),
		turn_left = helm("turn", 1),
		turn_right = helm("turn", -1),
	},
	screen = function(ctx, ui)
		local chart = ctx.get("ship", "chart")
		if not chart or not chart.open then
			return nil
		end
		return ui.panel {
			id = "chart",
			align = "top-right",
			x = 20,
			y = 20,
			width = 292,
			children = {
				ui.text { text = "Chart", size = 20 },
				ui.map {
					id = "map",
					width = 260,
					height = 260,
					memory = "ship",
					follow = "ship",
					span = 240,
					marks = {
						{ entity = "hub", text = "Hub", always = true },
						{ entity = "beacon", text = "Beacon" },
						{ entity = "rock1" },
						{ entity = "rock2" },
						{ entity = "rock3" },
						{ entity = "crate", text = "Crate", always = true },
					},
				},
			},
		}
	end,
}
