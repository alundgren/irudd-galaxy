--!strict
-- Flight keys set the ship's helm; number keys give it orders. Runs only when a key goes
-- down or up, never per frame: the engine flies the ship.

local AXES = {
	["drift.thrust_ahead"] = "ahead",
	["drift.thrust_back"] = "back",
	["drift.turn_left"] = "left",
	["drift.turn_right"] = "right",
	["drift.boost"] = "boost",
}

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
		end
	end
end

local function order(mode: string, target: string, range: number)
	return function(event, ctx)
		if not event.payload.down then
			return
		end
		for _, id in ctx.query("keys") do
			ctx.put(id, "engine.steer", { mode = mode, target = target, range = range })
		end
	end
end

-- The nearest rock to the ship.
local function mine(event, ctx)
	if not event.payload.down then
		return
	end
	for _, id in ctx.query("keys") do
		local at = ctx.get(id, "engine.position")
		local best, nearest = math.huge, ""
		for _, rock in { "drift.rock1", "drift.rock2", "drift.rock3", "drift.rock4" } do
			local p = ctx.get(rock, "engine.position")
			if at and p then
				local d = (p.x - at.x) ^ 2 + (p.y - at.y) ^ 2
				if d < best then
					best, nearest = d, rock
				end
			end
		end
		ctx.put(id, "engine.steer", { mode = "hold", target = nearest, range = 0 })
	end
end

return {
	on = {
		thrust_ahead = flight_key,
		thrust_back = flight_key,
		turn_left = flight_key,
		turn_right = flight_key,
		boost = flight_key,
		order_dock = order("dock", "drift.berth", 0),
		order_mine = mine,
		order_route = order("follow", "drift.w1", 0),
		order_none = order("", "", 0),
		["engine.arrived"] = function(event, ctx)
			print("arrived", event.payload.mode, event.payload.target)
		end,
	},
}
