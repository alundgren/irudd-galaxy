-- Lantern Wharf's places and people. The engine keeps where the player and everyone
-- else is (the place model); this says what there is to start with.
--
-- A place: `id`, `kind` (station, module, interior), `name`, `parent`, `enter` (going
-- to a module lands you in this room), `connects` (other places reached from here),
-- `line` (for signs). A module has its look on the station: `art` (the kit's module
-- kind) and `bearing` round the core. An interior has its `set`, its `floor` size in
-- metres, where you `arrive`, and named floor `spots` ({x, y}: x to the right, y away
-- from the open front of the set).
--
-- A person: `id`, `name`, `role`, `place`, `spot`, `state` (work, eat, away...), `look`
-- (the figure) and `home` (the room that is theirs, if not where they start).
return {
	start = "wharf",
	places = {
		{ id = "wharf", kind = "station", name = "Lantern Wharf", line = "Wharf Cooperative" },

		{
			id = "office_block",
			kind = "module",
			parent = "wharf",
			name = "Office block",
			art = "block",
			bearing = 180,
			enter = "office",
			line = "Ilse keeps the berths",
		},
		{
			id = "market_block",
			kind = "module",
			parent = "wharf",
			name = "Market hall",
			art = "banner",
			bearing = 70,
			enter = "market",
			line = "Ice and ore bought, plate sold",
		},
		{
			id = "wharf_arm",
			kind = "module",
			parent = "wharf",
			name = "Wharf arm",
			art = "docking_arm",
			bearing = 290,
			enter = "cradle",
			line = "Orrin fits parts to your ship",
		},
		{
			id = "canteen_block",
			kind = "module",
			parent = "wharf",
			name = "Canteen dome",
			art = "dome",
			bearing = 235,
			enter = "canteen",
			line = "Bel's broth, all hours",
		},

		{
			id = "office",
			kind = "interior",
			parent = "office_block",
			name = "Dock office",
			set = "office",
			floor = { 8, 5.5 },
			arrive = { 2.8, 0.4 },
			spots = {
				desk = { -0.8, 1.85 },
				chair = { 0.9, -0.3 },
				door = { 3.6, 0.4 },
				-- Ilse's break: looking out of the porthole.
				window = { -2.9, 1.7 },
				-- A visitor at the desk (Pim with a parcel), at its front left corner.
				parcel = { -2.0, 0.15 },
				-- Where something new for the player stands (the Wren's pennant).
				flag = { 1.3, 1.9 },
			},
		},
		{
			id = "market",
			kind = "interior",
			parent = "market_block",
			name = "Market",
			set = "market",
			floor = { 9, 6 },
			arrive = { 3.4, -0.6 },
			spots = {
				stall = { -1.6, 1.9 },
				crates = { -2.7, -0.3 },
				door = { 4.1, -0.6 },
				-- Juno counting stock at the end of the shelves.
				shelf = { 0.2, 2.4 },
				-- What the player sold, stacked.
				pile = { -0.9, -1.3 },
			},
		},
		{
			id = "cradle",
			kind = "interior",
			parent = "wharf_arm",
			name = "Orrin's cradle",
			set = "cradle",
			floor = { 14, 9 },
			arrive = { -6.2, -1.8 },
			spots = {
				bench = { -4.9, -0.6 },
				door = { -6.6, -1.8 },
				-- The crate the last part came in.
				boxes = { -3.6, -3.4 },
			},
		},
		{
			id = "canteen",
			kind = "interior",
			parent = "canteen_block",
			name = "Canteen",
			set = "canteen",
			floor = { 9, 6 },
			arrive = { 3.6, 1.2 },
			spots = {
				counter = { -2.0, 2.45 },
				table = { 1.75, 0.6 },
				door = { 4.1, 1.2 },
				-- Orrin's lunch: the near stool of the window table.
				stool = { 1.65, -0.9 },
				-- Bel eats last, standing at her end of the counter.
				nook = { -2.9, 2.45 },
				-- A mug for the player, at the end of the counter.
				mug = { -0.7, 1.7 },
			},
		},
	},
	people = {
		{ id = "ilse", name = "Ilse Marrow", role = "Dockmaster", place = "office", spot = "desk" },
		{ id = "juno", name = "Juno Pell", role = "Trader", place = "market", spot = "stall" },
		{ id = "orrin", name = "Orrin Vale", role = "Hullwright", place = "cradle", spot = "bench" },
		{ id = "bel", name = "Bel Ashby", role = "Cook", place = "canteen", spot = "counter" },
		{
			id = "pim",
			name = "Pim",
			role = "Courier",
			place = "canteen",
			spot = "table",
			state = "eat",
			home = "market",
		},
	},
}
