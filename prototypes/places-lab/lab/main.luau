-- The lab's game side: the state, the events that change it, and the view that turns it
-- into a screen. The engine calls `view` only after an event or a reload.
--
-- Where the player and the people are is the engine's (the place model): `world.here`,
-- `world.people`. Handlers move them with `world.go(place)` and `world.move(person,
-- place, state, spot)`; the engine hears of an arrival through `on.arrive`.
local data = use("data")
local fit = use("fit")

local lab = { on = {} }

function lab.init()
	return {
		credits = 2000,
		cargo = { ice = 14, iron = 6 },
		talk = nil, -- { id, line }: the talk going on
		heard = {}, -- lines already said, for the log
		said = {}, -- talks finished
		greeted = {}, -- rooms whose person has spoken first already
		goal = nil,
		receipt = nil,
		log_open = false,
		fitted = { nose = "long_drill" },
		locker = { "seam_lamp" },
		drag = nil, -- the part in hand: { part, from }
	}
end

local function say(state, id)
	state.talk = { id = id, line = 1 }
end

-- Going places -------------------------------------------------------------------------

function lab.on.go(state, args, world)
	state.talk = nil
	state.receipt = nil
	state.log_open = false
	world.go(args.place)
end

function lab.on.back(state, _, world)
	if state.drag then
		state.drag = nil
	elseif world.place.kind ~= "station" then
		state.talk = nil
		state.receipt = nil
		world.go(world.station.id)
	elseif state.log_open then
		state.log_open = false
	end
end

-- The view has landed in a room: its person speaks first, once.
function lab.on.arrive(state, args)
	local greet = data.greet[args.place]
	if greet and not state.greeted[args.place] then
		state.greeted[args.place] = true
		say(state, greet)
	end
end

function lab.on.toggle_log(state)
	state.log_open = not state.log_open
end

-- Talk ---------------------------------------------------------------------------------

function lab.on.talk_to(state, args, world)
	local person = world.people[args.who]
	if person and person.here then
		say(state, data.chat(args.who, person))
	end
end

function lab.on.talk_next(state, _, world)
	local t = state.talk
	if not t then
		return
	end
	local talk = data.talk[t.id]
	table.insert(state.heard, { who = world.people[talk.who].name, text = talk.lines[t.line] })
	if t.line < #talk.lines then
		t.line += 1
		return
	end
	state.talk = nil
	state.said[t.id] = true
	if talk.goal then
		state.goal = talk.goal
	end
	local m = talk.after
	if m then
		world.move(m.person, m.place, m.state, m.spot)
	end
end

-- Market -------------------------------------------------------------------------------

local function ware(id)
	for _, w in data.wares do
		if w.id == id then
			return w
		end
	end
	error(`no ware {id}`)
end

function lab.on.sell(state, args)
	local w = ware(args.ware)
	local have = state.cargo[w.id] or 0
	local n = if args.all then have else math.min(1, have)
	if n <= 0 then
		return
	end
	state.cargo[w.id] = have - n
	state.credits += n * w.price
	state.receipt = { text = `Sold {n} {string.lower(w.name)}`, amount = `+{n * w.price} cr`, state = "gain" }
end

function lab.on.buy(state, args)
	local w = ware(args.ware)
	local room = fit.stats(state.fitted).hold - fit.cargo_total(state)
	local afford = state.credits // w.price
	local n = math.min(if args.max then room else 1, room, afford)
	if n <= 0 then
		return
	end
	state.cargo[w.id] = (state.cargo[w.id] or 0) + n
	state.credits -= n * w.price
	state.receipt = { text = `Bought {n} {string.lower(w.name)}`, amount = `-{n * w.price} cr`, state = "loss" }
end

-- Canteen ------------------------------------------------------------------------------

function lab.on.buy_bowl(state)
	if state.credits < data.bowl.price then
		return
	end
	state.credits -= data.bowl.price
	state.receipt = { text = "A bowl of broth. Warm hands.", amount = `-{data.bowl.price} cr`, state = "loss" }
end

-- Cradle -------------------------------------------------------------------------------

function lab.on.pick(state, args)
	state.drag = { part = args.part, from = args.from }
	state.receipt = nil
end

function lab.on.drop_cancel(state)
	state.drag = nil
end

local function remove(list, item)
	for i, v in list do
		if v == item then
			table.remove(list, i)
			return
		end
	end
end

function lab.on.fit(state, args)
	state.drag = nil
	local p = data.parts[args.part]
	local ok = fit.socket_verdict(state, { part = args.part, from = args.from }, args.socket)
	if not ok then
		return -- the engine only drops on targets that said yes; the rules still hold
	end
	local replaced = state.fitted[args.socket]
	if args.from == "rack" then
		state.credits -= p.price
	elseif args.from == "locker" then
		remove(state.locker, args.part)
	else
		state.fitted[args.from] = nil
	end
	state.fitted[args.socket] = args.part
	local note = nil
	if replaced then
		table.insert(state.locker, replaced)
		note = `{data.parts[replaced].name} went to the locker`
	end
	state.receipt = {
		text = `Fitted {string.lower(p.name)}` .. (if note then `. {note}` else ""),
		amount = if args.from == "rack" then `-{p.price} cr` else nil,
		state = "loss",
	}
end

function lab.on.unfit(state, args)
	state.drag = nil
	if not fit.locker_verdict(state, { part = args.part, from = args.from }) then
		return
	end
	state.fitted[args.from] = nil
	table.insert(state.locker, args.part)
	state.receipt = { text = `{data.parts[args.part].name} is in the locker` }
end

-- View ---------------------------------------------------------------------------------

local rooms = {
	market = use("market"),
	cradle = use("cradle"),
}

function lab.view(state, ui, world)
	if world.place.kind == "interior" then
		return (rooms[world.here] or use("room"))(state, ui, world)
	end
	return use("station")(state, ui, world)
end

return lab
