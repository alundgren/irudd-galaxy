-- Lantern Wharf as the lab plays it. Numbers are illustrative.
local data = {}

-- What the station sells and buys, the Wren and its parts. Places and people are in
-- places.luau.

data.wares = {
	{ id = "glassite", name = "Glassite", side = "sell", price = 26 },
	{ id = "ice", name = "Ice", side = "sell", price = 10 },
	{ id = "iron", name = "Iron ore", side = "sell", price = 15 },
	{ id = "rations", name = "Rations", side = "buy", price = 16 },
	{ id = "plate", name = "Refined plate", side = "buy", price = 34 },
}

data.hull = { name = "the Wren", power = 4, free_mass = 3, speed = 72, turn = 180, hold = 20 }

data.sockets = {
	{ id = "nose", name = "Nose", side = "right" },
	{ id = "utility", name = "Utility", side = "right" },
	{ id = "belly", name = "Hold", side = "right" },
	{ id = "tail", name = "Engine", side = "right" },
}

data.parts = {
	long_drill = {
		name = "Long-throw drill",
		socket = "nose",
		power = 3,
		mass = 2,
		effect = "Mine from further off",
		look = "long_drill",
	},
	lens = {
		name = "Mining lens",
		socket = "nose",
		power = 2,
		mass = 1,
		effect = "Mine half again as fast",
		look = "lens",
		price = 600,
	},
	pods = {
		name = "Hold pods",
		socket = "belly",
		power = 0,
		mass = 4,
		effect = "Hold +10, heavier",
		look = "pods",
		price = 450,
		hold = 10,
	},
	fins = {
		name = "Swift fins",
		socket = "tail",
		power = 1,
		mass = 1,
		effect = "Turns 40°/s faster",
		look = "fins",
		price = 380,
		turn = 40,
	},
	seam_lamp = {
		name = "Seam lamp",
		socket = "utility",
		power = 2,
		mass = 0,
		effect = "Shows what is left in nearby rocks",
		look = "seam_lamp",
	},
}

-- What Orrin has on the rack, in order.
data.rack = { "lens", "pods", "fins" }

-- Talk: one short line at a time. `who` is a person in places.luau.
data.talk = {
	welcome = {
		who = "ilse",
		lines = {
			"Welcome to Lantern Wharf. I'm Ilse; I keep the dock.",
			"The Wren's yours now: small, round-nosed and honest.",
			"Odo at Kettle Dock needs ice for his greenhouse.",
			"The pale rocks east of here are Rimewater ice.",
		},
		goal = { text = "Bring ice to Odo's greenhouse at Kettle Dock", from = "Ilse Marrow" },
		-- A story moment: Pim finishes eating and takes a parcel to the market.
		after = { person = "pim", place = "market", state = "work", spot = "crates" },
	},
	ilse = { who = "ilse", lines = { "Odo's still waiting on that ice. The pale rocks, east." } },
	juno_first = {
		who = "juno",
		lines = {
			"Glassite pays well here. The lamp-glass works can't get enough of it.",
			"Sell what you've mined. Plate and rations are on the counter.",
		},
	},
	juno = { who = "juno", lines = { "Ice is ten a block today. Bring me glassite and we'll talk." } },
	orrin_first = {
		who = "orrin",
		lines = { "So you're the Wren's new keeper. Set a part on a socket and I'll swing it in." },
	},
	orrin = { who = "orrin", lines = { "She's a good little hull. Mind the power when you fit things." } },
	bel_first = { who = "bel", lines = { "Sit anywhere. The broth's on, and it's always on." } },
	bel = { who = "bel", lines = { "Kelp and barley today. Same as yesterday, better than tomorrow." } },
	pim_eating = { who = "pim", lines = { "Can't talk, eating. Then parcels. Always parcels." } },
	pim_working = { who = "pim", lines = { "Parcel for Juno! Then back to the canteen. Then another parcel." } },
}

-- Who speaks first the first time you walk into a room.
data.greet = { office = "welcome", market = "juno_first", cradle = "orrin_first", canteen = "bel_first" }

-- What a person says when you go up to them, by what they are doing.
function data.chat(who, person)
	if who == "pim" then
		return if person.state == "eat" then "pim_eating" else "pim_working"
	end
	return who
end

-- The canteen's broth.
data.bowl = { name = "Kelp and barley broth", price = 6 }

data.notices = {
	"Lamp night at Kettle Dock once the greenhouse grows its next ring.",
	"The lantern current east of the Wharf runs to Cinder Drift.",
}

return data
