--!strict
-- Flight keys set the ship's helm; number keys give it orders. Effects follow the
-- orders: the beam lands on a rock once a mine order holds there, and docking bursts a
-- puff and kicks the camera. Runs only on events, never per frame: the engine flies the
-- ship and draws the effects.

local AXES = {
	["effects.thrust_ahead"] = "ahead",
	["effects.thrust_back"] = "back",
	["effects.turn_left"] = "left",
	["effects.turn_right"] = "right",
	["effects.boost"] = "boost",
}

local ROCKS = { "effects.rock1", "effects.rock2", "effects.rock3" }

-- Taking the controls or giving another order switches the beam off.
local function beam_off(ctx, id: string)
	ctx.patch(id, "engine.beam", { target = "" })
end

-- A key held counts once; two keys for one way (W and Up) add up, so letting go of one
-- keeps the other.
local function flight_key(event, ctx)
	local which = AXES[event.kind]
	for _, id in ctx.query("keys") do
		local keys = ctx.get(id, "keys")
		if keys then
			local held = math.max(0, keys[which] + (event.payload.down and 1 or -1))
			keys[which] = held
			ctx.patch(id, "keys", { [which] = held })
			local function on(name: string): number
				return if keys[name] > 0 then 1 else 0
			end
			ctx.patch(id, "engine.helm", {
				thrust = on("ahead") - on("back"),
				turn = on("left") - on("right"),
				boost = keys.boost > 0,
			})
			beam_off(ctx, id)
		end
	end
end

local function order(mode: string, target: string)
	return function(event, ctx)
		if not event.payload.down then
			return
		end
		for _, id in ctx.query("keys") do
			ctx.put(id, "engine.steer", { mode = mode, target = target, range = 0 })
			beam_off(ctx, id)
		end
	end
end

-- Holds on the nearest rock; the beam starts once it is there.
local function mine(event, ctx)
	if not event.payload.down then
		return
	end
	for _, id in ctx.query("keys") do
		local at = ctx.get(id, "engine.position")
		local best, nearest = math.huge, ""
		for _, rock in ROCKS do
			local p = ctx.get(rock, "engine.position")
			if at and p then
				local d = (p.x - at.x) ^ 2 + (p.y - at.y) ^ 2
				if d < best then
					best, nearest = d, rock
				end
			end
		end
		ctx.put(id, "engine.steer", { mode = "hold", target = nearest, range = 3.0 })
		beam_off(ctx, id)
	end
end

local function arrived(event, ctx)
	local id = event.payload.entity
	if event.payload.mode == "hold" then
		ctx.patch(id, "engine.beam", { target = event.payload.target })
	elseif event.payload.mode == "dock" then
		ctx.patch(id, "engine.burst", { recipe = "effects.puff", tick = ctx.tick, x = -0.5 })
		ctx.patch(id, "engine.camera_feel", { shake = 0.025, shake_tick = ctx.tick, shake_seconds = 0.45 })
	end
end

return {
	on = {
		thrust_ahead = flight_key,
		thrust_back = flight_key,
		turn_left = flight_key,
		turn_right = flight_key,
		boost = flight_key,
		order_dock = order("dock", "effects.berth"),
		order_mine = mine,
		order_none = order("", ""),
		["engine.arrived"] = arrived,
	},
}
