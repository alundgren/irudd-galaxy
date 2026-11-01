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

	-- What people say while they're about their day (the `states` and `return` life
	-- variants move them through it; `data.chat` picks by what they're doing).
	ilse_window = { who = "ilse", lines = { "I watch every ship in. Thirty years and I still do." } },
	juno_count = { who = "juno", lines = { "Forty-one jars of lamp oil. It was forty-two this morning." } },
	juno_snack = { who = "juno", lines = { "Mouth full, hands free. Sell me something." } },
	orrin_lunch = { who = "orrin", lines = { "Bel's broth and ten minutes off my feet. The Wren'll keep." } },
	bel_eat = { who = "bel", lines = { "Cook eats last. That's the rule, and I made it." } },
	pim_visit = { who = "pim", lines = { "Berth papers for Ilse. Sign here. No, there." } },

	-- Something to tell the player when they come back, because of what they did (the
	-- `return` variant says it as they come in).
	ilse_board = { who = "ilse", lines = { "You're on the board now: berth three, the Wren. Pim ran the card over." } },
	juno_sold_ice = { who = "juno", lines = { "Your ice went out on the Kettle run. Odo's greenhouse drinks it by the block." } },
	juno_sold_iron = { who = "juno", lines = { "The foundry took your ore. They grumbled at the grade, then paid." } },
	juno_sold_glassite = { who = "juno", lines = { "Your glassite's already lamp glass. Look up in the market hall tonight." } },
	orrin_lens = { who = "orrin", lines = { "How's the lens? It runs warm the first few rocks, then settles." } },
	orrin_pods = { who = "orrin", lines = { "Pods ride low. You'll feel her lean in the turns." } },
	orrin_fins = { who = "orrin", lines = { "Fins bedding in? She'll want to spin now. Let her." } },
	bel_bowl = { who = "bel", lines = { "There's a mug with your name on it. Well, your ship's. Same thing here." } },

	-- What the people in charge of a room say about something new in it.
	mug_look = { who = "bel", lines = { "Every regular gets a mug on my counter. You came back, so you're a regular." } },
	pennant_look = { who = "ilse", lines = { "Every ship with a berth flies its pennant here. The Wren's is new." } },
	crate_look = { who = "orrin", lines = { "Your part came in that. I keep the crates. Never know." } },
	pile_look = { who = "juno", lines = { "That's yours, what you sold me. Goes out on the next run." } },
}

-- Who speaks first the first time you walk into a room.
data.greet = { office = "welcome", market = "juno_first", cradle = "orrin_first", canteen = "bel_first" }

-- What a person says when you go up to them, by what they are doing.
local by_state = {
	ilse = { ["break"] = "ilse_window" },
	juno = { count = "juno_count", snack = "juno_snack" },
	orrin = { lunch = "orrin_lunch" },
	bel = { eat = "bel_eat" },
	pim = { eat = "pim_eating", visit = "pim_visit" },
}

function data.chat(who, person)
	local s = by_state[who] and by_state[who][person.state]
	if s then
		return s
	end
	if who == "pim" then
		return "pim_working"
	end
	return who
end

-- What someone is doing, said on their sign: by state.
data.doing = {
	["break"] = "On a break",
	visit = "Visiting",
	count = "Counting stock",
	snack = "Eating",
	lunch = "At lunch",
	eat = "Eating",
}

-- Things that appear in a room because of what the player did (the `return` variant):
-- what to call them, and who in the room says what about them.
data.details = {
	pennant = { name = "The Wren's pennant", talk = "pennant_look" },
	mug = { name = "A mug for you", talk = "mug_look" },
	sold_ice = { name = "Your ice", talk = "pile_look" },
	sold_iron = { name = "Your ore", talk = "pile_look" },
	sold_glassite = { name = "Your glassite", talk = "pile_look" },
	crate_lens = { name = "The lens's crate", talk = "crate_look" },
	crate_pods = { name = "The pods' crate", talk = "crate_look" },
	crate_fins = { name = "The fins' crate", talk = "crate_look" },
}

-- The canteen's broth.
data.bowl = { name = "Kelp and barley broth", price = 6 }

data.notices = {
	"Lamp night at Kettle Dock once the greenhouse grows its next ring.",
	"The lantern current east of the Wharf runs to Cinder Drift.",
}

return data
