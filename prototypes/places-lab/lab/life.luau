-- What the life variant marks on the screens, from `world.life` (nil for the baseline).
-- The screens stay as they are: this adds a state to the signs they already have (a
-- module's, a person's) and, coming back to a room, a line under its name.
--
-- `states`: the station's signs name who is there who doesn't belong there; in a room,
-- people's signs say what they're doing.
-- `return`: rooms changed since you last left them are marked on the station; in the
-- room, people here now or doing something else are marked, the room's panel says who
-- has gone where, and what is new can be clicked (its person says what it is).
local data = use("data")
local life = {}

local function first(name)
	return string.split(name, " ")[1]
end

local function find(s, id)
	for _, c in s do
		if type(c) == "table" and c.id == id then
			return c
		end
	end
	return nil
end

-- A place's name in a sentence: "the canteen", "the dock office", "Orrin's cradle".
local function where(world, place)
	local p = world.places[place]
	if not p then
		return place
	end
	return if string.find(p.name, "'") then p.name else `the {string.lower(p.name)}`
end

local function doing(person)
	return data.doing[person.state]
end

-- States -------------------------------------------------------------------------------

local function states(s, world)
	-- Someone on their way somewhere has no sign: they're busy going, and a sign moving
	-- across the room would pass over the face of whoever is talking.
	for i = #s, 1, -1 do
		local c = s[i]
		local who = type(c) == "table" and c.kind == "hotspot" and c.id and string.match(c.id, "^person_(.+)$")
		if who and world.people[who] and world.people[who].state == "walk" then
			table.remove(s, i)
		end
	end
	if world.place.kind == "station" then
		for _, id in world.modules do
			local room = world.places[id].enter
			local names = {}
			for _, p in world.people do
				if p.place == room and p.home ~= room then
					table.insert(names, first(p.name))
				end
			end
			local spot = find(s, "spot_" .. id)
			if spot and #names > 0 then
				table.sort(names)
				spot.sign.state = { "info", table.concat(names, ", ") }
			end
		end
		return
	end
	for _, id in world.people_here do
		local p = world.people[id]
		local spot = find(s, "person_" .. id)
		local word = doing(p)
		if spot and word then
			spot.sign.state = { "info", word }
		end
	end
end

-- Return -------------------------------------------------------------------------------

-- What changed in `room` while the player was elsewhere, said short. Nothing for a
-- room never seen, or one they have only just left (`away`: rooms entered since).
local function summary(state, world, room)
	local c = world.life.rooms[room]
	local bits = {}
	if not c or c.away < 1 then
		return bits
	end
	for _, who in c.came do
		table.insert(bits, `{first(world.people[who].name)} is here`)
	end
	for _, g in c.gone do
		table.insert(bits, `{first(world.people[g.who].name)} went to {where(world, g.to)}`)
	end
	for _, d in c.doing do
		local word = doing(world.people[d.who])
		if word then
			table.insert(bits, `{first(world.people[d.who].name)} is {string.lower(word)}`)
		end
	end
	if #c.details > 0 then
		table.insert(bits, "something new")
	end
	for who, n in state.news do
		local p = world.people[who]
		if n.ready and p and p.place == room then
			table.insert(bits, `{first(p.name)} has a word for you`)
		end
	end
	return bits
end

local function returning(s, state, ui, world)
	if world.place.kind == "station" then
		for _, id in world.modules do
			local room = world.places[id].enter
			local bits = summary(state, world, room)
			local spot = find(s, "spot_" .. id)
			if spot and #bits > 0 then
				spot.sign.state = { "changed", "Changed" }
				spot.sign.line = table.concat(bits, " · ")
			end
		end
		return
	end
	local c = world.life.rooms[world.here]
	if not c then
		return
	end
	for _, who in c.came do
		local spot = find(s, "person_" .. who)
		if spot then
			spot.sign.state = { "new", "Here now" }
		end
	end
	for _, d in c.doing do
		local spot = find(s, "person_" .. d.who)
		local word = doing(world.people[d.who])
		if spot and word then
			spot.sign.state = { "changed", word }
		end
	end
	-- Who has gone where, under the room's name.
	local gone = {}
	for _, g in c.gone do
		table.insert(gone, `{first(world.people[g.who].name)} went to {where(world, g.to)}`)
	end
	local panel = find(s, "room")
	local head = panel and panel[1] and panel[1][2]
	if head and #gone > 0 then
		table.insert(head, ui.small({ `Since you were here: {table.concat(gone, "; ")}.`, id = "since" }))
	end
	-- What is new: its own sign, to click.
	for _, d in world.life.details do
		local what = data.details[d.id]
		if d.new and d.room == world.here and what then
			table.insert(
				s,
				ui.hotspot({
					id = "new_" .. d.id,
					anchor = "spot." .. d.spot,
					on = "look",
					args = { detail = d.id },
					sign = { title = what.name, state = { "new", "New" } },
				})
			)
		end
	end
end

function life.mark(s, state, ui, world)
	local l = world.life
	if not l then
		return s
	end
	if l.name == "states" then
		states(s, world)
	elseif l.name == "return" then
		returning(s, state, ui, world)
	end
	return s
end

return life
