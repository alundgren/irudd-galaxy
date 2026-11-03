--!strict
-- Flight keys set the ship's helm; 2 holds on the ore rock, and while the hold is there
-- the slow cadence cuts the rock a sixth smaller, six times. The screen is a status panel
-- in the top-right corner and the trader's name label, which yields: it isn't drawn
-- while it would cover the panel or leave the window. Runs on keys and the cadence,
-- never per frame.

local AXES = {
	["ambient.thrust_ahead"] = "ahead",
	["ambient.thrust_back"] = "back",
	["ambient.turn_left"] = "left",
	["ambient.turn_right"] = "right",
}

local function flight_key(event, ctx)
	local which = AXES[event.kind]
	local keys = ctx.get("ship", "keys")
	if not keys then
		return
	end
	local held = math.max(0, keys[which] + (event.payload.down and 1 or -1))
	keys[which] = held
	ctx.patch("ship", "keys", { [which] = held })
	local function on(name: string): number
		return if keys[name] > 0 then 1 else 0
	end
	ctx.patch("ship", "engine.helm", {
		thrust = on("ahead") - on("back"),
		turn = on("left") - on("right"),
	})
end

local function order(mode: string, target: string)
	return function(event, ctx)
		if event.payload.down then
			ctx.put("ship", "engine.steer", { mode = mode, target = target, range = 0 })
		end
	end
end

return {
	screen = function(ctx, ui)
		local cuts = ctx.get("state", "cuts")
		local steer = ctx.get("ship", "engine.steer")
		local doing = if steer and steer.mode == "hold" then "Holding on the ore" else "Flying"
		return ui.column { children = {
			ui.panel { id = "status", align = "top-right", x = 16, y = 16, width = 280, children = {
				ui.text { text = doing },
				ui.text { text = "Cuts: " .. (if cuts then cuts.count else 0) .. " of 6" },
			} },
			ui.text { id = "trader-label", text = "Trader", width = 90, anchor = "trader_label#label", yield = true },
		} }
	end,
	on = {
		thrust_ahead = flight_key,
		thrust_back = flight_key,
		turn_left = flight_key,
		turn_right = flight_key,
		order_mine = order("hold", "ambient.ore"),
		order_none = order("", ""),
	},
	cadence = function(ctx)
		local steer = ctx.get("ship", "engine.steer")
		local cuts = ctx.get("state", "cuts")
		if not (steer and cuts) or steer.mode ~= "hold" or not steer.done or cuts.count >= 6 then
			return
		end
		local count = cuts.count + 1
		ctx.patch("state", "cuts", { count = count })
		ctx.patch("ore", "engine.model", { morph = count / 6 })
	end,
}
