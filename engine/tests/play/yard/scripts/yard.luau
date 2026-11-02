--!strict
-- Flight keys set the ship's helm; M mines both rocks a step, running their morph from
-- whole (0) to mined (1) and back to whole. Runs only when a key goes down or up.

local AXES = {
	["yard.thrust_ahead"] = "ahead",
	["yard.thrust_back"] = "back",
	["yard.turn_left"] = "left",
	["yard.turn_right"] = "right",
	["yard.boost"] = "boost",
}

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

local function mine(event, ctx)
	if not event.payload.down then
		return
	end
	for _, rock in { "yard.rock", "yard.rock_pale" } do
		local model = ctx.get(rock, "engine.model")
		if model then
			local next = model.morph + 0.25
			ctx.patch(rock, "engine.model", { morph = if next > 1.01 then 0 else next })
		end
	end
end

return {
	on = {
		thrust_ahead = flight_key,
		thrust_back = flight_key,
		turn_left = flight_key,
		turn_right = flight_key,
		boost = flight_key,
		mine = mine,
	},
}
