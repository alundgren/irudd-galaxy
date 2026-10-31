-- Lantern Wharf as the lab plays it. Numbers are illustrative.
local data = {}

data.station = { name = "Lantern Wharf", faction = "Wharf Cooperative" }

-- Each module on the station: its room, its sign and any news on its key icon.
data.rooms = {
	{ id = "office", name = "Office", icon = "office", keeper = "Ilse Marrow", line = "Ilse keeps the berths" },
	{
		id = "market",
		name = "Market",
		icon = "market",
		keeper = "Ilse Marrow",
		line = "Ice and ore bought, plate sold",
		news = { "new", "New", "Glassite is bought here now" },
	},
	{
		id = "wharf",
		name = "Cradle",
		icon = "wharf",
		keeper = "Orrin Vale",
		line = "Orrin fits parts to your ship",
		news = { "changed", "Changed", "Orrin has hold pods on the rack" },
	},
	{ id = "notices", name = "Notices", icon = "notices", keeper = "Ilse Marrow", line = "Two notes pinned" },
	{
		id = "canteen",
		name = "Canteen",
		icon = "canteen",
		keeper = "Bel Ashby",
		line = "Opens when Kettle Dock ships rations",
		news = { "closed", "Closed", "The canteen frame is down for re-sealing" },
	},
}

data.wares = {
	{ id = "glassite", name = "Glassite", side = "sell", price = 26 },
	{ id = "ice", name = "Ice", side = "sell", price = 10 },
	{ id = "iron", name = "Iron ore", side = "sell", price = 15 },
	{ id = "rations", name = "Rations", side = "buy", price = 16 },
	{ id = "plate", name = "Refined plate", side = "buy", price = 34 },
}

data.hull = { name = "the Wren", power = 4, free_mass = 3, speed = 72, turn = 180, hold = 20 }

data.sockets = {
	{ id = "nose", name = "Drill mount", side = "left" },
	{ id = "utility", name = "Utility", side = "right" },
	{ id = "belly", name = "Hold", side = "left" },
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

-- Talk: one short line at a time.
data.talk = {
	welcome = {
		who = "Ilse Marrow",
		place = "Dockmaster",
		at = "office",
		lines = {
			"Welcome to Lantern Wharf. I'm Ilse; I keep the dock.",
			"The Wren's yours now: small, round-nosed and honest.",
			"Odo at Kettle Dock needs ice for his greenhouse.",
			"The pale rocks east of here are Rimewater ice.",
		},
		goal = { text = "Bring ice to Odo's greenhouse at Kettle Dock", from = "Ilse Marrow" },
	},
	market = {
		who = "Ilse Marrow",
		place = "Market",
		at = "market",
		lines = { "Glassite pays well here. The lamp-glass works can't get enough of it." },
	},
	wharf = {
		who = "Orrin Vale",
		place = "Hullwright",
		at = "keeper",
		lines = { "Set a part on a socket and I'll swing it in." },
	},
	canteen = {
		who = "Bel Ashby",
		place = "Canteen",
		at = "canteen",
		lines = { "We've taken the frame down to re-seal it. Kettle Dock's rations will open us." },
	},
}

data.notices = {
	"Lamp night at Kettle Dock once the greenhouse grows its next ring.",
	"The lantern current east of the Wharf runs to Cinder Drift.",
}

return data
