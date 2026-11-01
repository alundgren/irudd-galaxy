--!strict
-- Keys set the helm; the helm sets the ship's velocity and facing. A stand-in for the
-- engine's flight model: it runs only when a key goes down or up, never per frame.

local DIRECTIONS = {
	["drift.steer_up"] = "up",
	["drift.steer_down"] = "down",
	["drift.steer_left"] = "left",
	["drift.steer_right"] = "right",
}

local function steer(event, ctx)
	local which = DIRECTIONS[event.kind]
	for _, id in ctx.query("helm") do
		local helm = ctx.get(id, "helm")
		if helm then
			helm[which] = event.payload.down
			ctx.patch(id, "helm", { [which] = event.payload.down })
			local x = (helm.right and 1 or 0) - (helm.left and 1 or 0)
			local y = (helm.up and 1 or 0) - (helm.down and 1 or 0)
			local length = math.sqrt(x * x + y * y)
			if length > 0 then
				ctx.patch(id, "engine.motion", { vx = helm.speed * x / length, vy = helm.speed * y / length })
				ctx.patch(id, "engine.position", { angle = math.atan2(y, x) })
			else
				ctx.patch(id, "engine.motion", { vx = 0, vy = 0 })
			end
		end
	end
end

return {
	on = {
		steer_up = steer,
		steer_down = steer,
		steer_left = steer,
		steer_right = steer,
	},
}
