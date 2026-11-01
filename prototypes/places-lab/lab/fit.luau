-- The fit rules: what the Wren's stats are, and whether a part can go somewhere. The
-- cradle screen asks these once, when a part is picked up, and hands the verdicts to the
-- engine's drop targets.
local data = use("data")
local fit = {}

-- Power and mass in use, and the stats, for a set of fitted parts.
function fit.stats(fitted)
	local h = data.hull
	local power, mass, hold, turn = 0, 0, h.hold, h.turn
	for _, id in pairs(fitted) do
		local p = data.parts[id]
		power += p.power
		mass += p.mass
		hold += p.hold or 0
		turn += p.turn or 0
	end
	local over = math.max(0, mass - h.free_mass)
	return {
		power = power,
		mass = mass,
		over = over,
		hold = hold,
		speed = h.speed - over * 3,
		turn = turn - over * 8,
	}
end

local function copy(t)
	local c = {}
	for k, v in pairs(t) do
		c[k] = v
	end
	return c
end

local function cargo_total(state)
	local n = 0
	for _, v in pairs(state.cargo) do
		n += v
	end
	return n
end
fit.cargo_total = cargo_total

-- The fit if `part` (coming `from` a socket, the rack or the locker) went on `socket`.
function fit.after(state, part, from, socket)
	local f = copy(state.fitted)
	if from and data.parts[part] and f[from] == part then
		f[from] = nil
	end
	f[socket] = part
	return f
end

-- Can the dragged part go on `socket`? Returns ok, reason.
function fit.socket_verdict(state, drag, socket)
	local p = data.parts[drag.part]
	if p.socket ~= socket then
		return nil, nil -- not a target: it goes on another socket
	end
	if drag.from == socket then
		return nil, nil
	end
	local after = fit.after(state, drag.part, drag.from, socket)
	local s = fit.stats(after)
	if s.power > data.hull.power then
		local free = data.hull.power - fit.stats(fit.after(state, nil, nil, socket)).power
		return false, `Not enough power: needs {p.power}, {math.max(free, 0)} free`
	end
	if drag.from == "rack" and p.price > state.credits then
		return false, `Costs {p.price} cr; you have {state.credits} cr`
	end
	if s.hold < cargo_total(state) then
		return false, `You carry {cargo_total(state)}; the hold would take {s.hold}`
	end
	return true, nil
end

-- Can a fitted part come off into the locker?
function fit.locker_verdict(state, drag)
	if drag.from == "rack" or drag.from == "locker" then
		return nil, nil
	end
	local f = copy(state.fitted)
	f[drag.from] = nil
	local s = fit.stats(f)
	if s.hold < cargo_total(state) then
		return false, `You carry {cargo_total(state)}; the hold would take {s.hold}. Sell or deliver some first`
	end
	return true, nil
end

-- A change where more is better, as a word and a state: "+6" gain, "-6" loss.
function fit.delta(before, after)
	local d = after - before
	if d == 0 then
		return nil
	end
	return (if d > 0 then "+" else "") .. tostring(d), (if d > 0 then "gain" else "loss")
end

return fit
