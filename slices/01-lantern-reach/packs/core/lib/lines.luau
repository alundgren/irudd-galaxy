--!strict
-- The core pack's NPC line pools (content/05-people.md). Text is important or absent:
-- every line cites one history fact of its `kind` that passes `when`, and fills its
-- placeholders from that fact only. Lines marked (gm) in the spec live in the
-- gm-examples pack, never here.
--
-- `specific` is how many conditions the line's citation has: dock talk says the most
-- specific eligible line, ties to the newest fact. `beat` lines are said only by a
-- beat conversation (the scripts that run that beat), never as ambient dock talk.
--
-- How core facts carry the spec's fields (content/SPEC.md "History facts"):
--   core.stage_built        station, amount = stage
--   core.shortage_started   station = Kettle, other = the shortage, amount = way A's quota
--   core.shortage_helped    other = the shortage, amount = way (1 import, 2 rest)
--   core.shortage_resolved  other = the shortage, faction = who (the Compact), or
--                           ship = the player; amount = way (0 ignored)
--   core.dispute_started    station = the Hook, other = the dispute
--   core.side_chosen        station, other = the dispute, faction = the side (none: no faction)
--   core.territory_changed  station, other = the dispute, faction = the new owner
--   core.rumor_heard        station, npc = the teller, other = the rumor,
--                           ship = the craft that brought it; amount = truth (1 or 0);
--                           note = who it came from, in words
--   core.rumor_spread       station, other = the rumor
--   core.rumor_followed     ship = the player, other = the rumor, amount = truth
--   core.rumor_resolved     other = the rumor, amount = truth, ship = the player if followed
--   core.fragment_found     ship, rock, ware = the fragment
--   core.shown              npc, station, ware = the item
--   core.canon_opened       other = the canon node
--   core.mechanic_taught    npc = the teacher, other = the mechanic
--   core.asked_what_changed npc, station
--   core.npc_line           npc, station, other = the line, amount = the cited fact's number
local G = require("@galaxy")
local ids = require("./ids")
local util = require("./util")

export type Line = {
	id: string,
	-- The speaker, or "any_dockmaster": whoever keeps the dock where it is said.
	npc: string,
	kind: string,
	specific: number,
	beat: boolean?,
	when: ((f: G.Fact, ctx: G.Ctx) -> boolean)?,
	text: (f: G.Fact, ctx: G.Ctx) -> string,
}

local function station_name(ctx: G.Ctx, id: string?): string
	local s = if id then ctx.stations:get(id) else nil
	return if s then s.name else "the dock"
end

local function craft_name(ctx: G.Ctx, id: string?): string
	local c = if id then ctx.space:craft(id) else nil
	return if c then c.name else "the tender"
end

local function trait_name(ctx: G.Ctx, f: G.Fact): string
	local t = ctx.traits:get(f.subjects.other or "")
	return if t then t.name else "a habit"
end

-- The player's own tender (not the flagship, not someone else's craft).
local function players_tender(ctx: G.Ctx, f: G.Fact): boolean
	local ship = f.subjects.ship
	return ship ~= nil and ship ~= ids.PLAYER and util.players(ctx, ship)
end

-- Fields the ship crate writes into a patient sell's note.
local function pulses(f: G.Fact): string
	return string.match(util.note(f), "(%d+) pulses waited") or "0"
end
local function avg(f: G.Fact): string
	local a = string.match(util.note(f), ": ([%d%.]+) cr a unit")
	return if a then `{a} cr` else "a fair price"
end
local function floor_of(f: G.Fact): string
	local a = string.match(util.note(f), "floor ([%d%.]+) cr")
	return if a then `{a} cr` else "its floor"
end
local function blocked_ware(ctx: G.Ctx, f: G.Fact): string
	local w = string.match(util.note(f), "can't sell (%S+) at") or ""
	local names: { [string]: string } = {
		[ids.ICE] = "ice",
		[ids.ORE] = "iron ore",
		[ids.PLATE] = "plate",
		[ids.GLASSITE] = "glassite",
		[ids.RATIONS] = "rations",
	}
	return names[w] or "its cargo"
end

local function is_rock(f: G.Fact, prefix: string): boolean
	local r = f.subjects.rock
	return r ~= nil and string.sub(r, 1, #prefix) == prefix
end

local function n_of(ctx: G.Ctx, q: G.HistoryQuery): number
	return ctx.history:count(q)
end

local lines: { Line } = {
	-- Ilse Marrow, dockmaster at Lantern Wharf
	{
		id = "line.ilse_marrow.01",
		npc = ids.ILSE,
		kind = "deliver",
		specific = 2,
		when = function(f, _)
			return f.subjects.other == ids.GREENHOUSE
		end,
		text = function(_, _)
			return "Odo's been counting your ice. He counts slower than I do, but he gets there."
		end,
	},
	{
		id = "line.ilse_marrow.03",
		npc = ids.ILSE,
		kind = "core.territory_changed",
		specific = 3,
		when = function(f, _)
			return f.subjects.station == ids.HOOK and f.subjects.faction == ids.COOP
		end,
		text = function(_, _)
			return "Cooperative banner over the Hook depot. Rook pretends not to care. He cares."
		end,
	},
	{
		id = "line.ilse_marrow.04",
		npc = ids.ILSE,
		kind = "core.territory_changed",
		specific = 3,
		when = function(f, _)
			return f.subjects.station == ids.HOOK and f.subjects.faction == ids.COMPACT
		end,
		text = function(_, _)
			return "The Compact has the Hook depot now. We'll buy our ore like anyone else."
		end,
	},
	{
		id = "line.ilse_marrow.05",
		npc = ids.ILSE,
		kind = "core.shortage_started",
		specific = 1,
		text = function(_, _)
			return "Rimewater's thin. A lot of your ice is in our tanks, and not much is left out there."
		end,
	},
	{
		id = "line.ilse_marrow.06",
		npc = ids.ILSE,
		kind = "core.shortage_resolved",
		specific = 2,
		when = function(f, _)
			return f.subjects.faction == ids.COMPACT
		end,
		text = function(_, _)
			return "The Compact brought ice in. Kind of them. They'll want thanking."
		end,
	},
	{
		id = "line.ilse_marrow.07",
		npc = ids.ILSE,
		kind = "market_flooded",
		specific = 2,
		when = function(f, _)
			return f.subjects.station == ids.BRASS and f.subjects.ship == ids.PLAYER
		end,
		text = function(_, _)
			return "Saffron says you sold Brassmouth so much glassite they're using it as doorstops."
		end,
	},
	{
		id = "line.ilse_marrow.08",
		npc = ids.ILSE,
		kind = "prestige",
		specific = 2,
		when = function(f, _)
			return f.subjects.other == "hull.tandem"
		end,
		text = function(_, _)
			return "A Tandem. Wide in the hips. She'll carry what you can't."
		end,
	},
	{
		id = "line.ilse_marrow.09",
		npc = ids.ILSE,
		kind = "prestige",
		specific = 2,
		when = function(f, _)
			return f.subjects.other == "hull.heron"
		end,
		text = function(_, _)
			return "A Heron. You'll see the prices before I do, then."
		end,
	},
	{
		id = "line.ilse_marrow.11",
		npc = ids.ILSE,
		kind = "core.stage_built",
		specific = 3,
		when = function(f, _)
			return f.subjects.station == ids.KETTLE and f.amount == 1
		end,
		text = function(_, _)
			return "Odo's got a pane up at Kettle. One dome, and it fogs. He's never been prouder of anything."
		end,
	},
	{
		id = "line.ilse_marrow.12",
		npc = ids.ILSE,
		kind = "tender_acquired",
		specific = 1,
		text = function(f, ctx)
			return `Tamsin says {craft_name(ctx, f.subjects.ship)} is yours now. Feed it good loops and it won't complain.`
		end,
	},
	{
		id = "line.ilse_marrow.13",
		npc = ids.ILSE,
		kind = "trait_earned",
		specific = 1,
		text = function(f, ctx)
			return `Orrin tells me you've a knack now: {trait_name(ctx, f)}. He doesn't say that about many.`
		end,
	},
	{
		id = "line.ilse_marrow.14",
		npc = ids.ILSE,
		kind = "core.shortage_resolved",
		specific = 2,
		when = function(f, _)
			return f.subjects.ship == ids.PLAYER
		end,
		text = function(_, _)
			return "Rimewater came through, and so did you. Odo's lamps are bright again."
		end,
	},
	{
		id = "line.ilse_marrow.15",
		npc = ids.ILSE,
		kind = "core.rumor_resolved",
		specific = 1,
		text = function(f, _)
			local word = if f.amount == 1 then "true" else "empty"
			return `The Shoals turned out {word}. Somebody always comes back with a story. This time it was you.`
		end,
	},
	{
		id = "line.ilse_marrow.16",
		npc = ids.ILSE,
		kind = "core.canon_opened",
		specific = 2,
		when = function(f, _)
			return f.subjects.other == "canon.fragments_sing"
		end,
		text = function(_, _)
			return "The Lamplighters are humming at their lamps. Something about your rock singing."
		end,
	},
	{
		id = "line.ilse_marrow.17",
		npc = ids.ILSE,
		kind = "prestige",
		specific = 1,
		beat = true,
		text = function(_, ctx)
			return `Welcome home, {ctx.beats:pilot().name}. New hull, same ring. Mind the paint.`
		end,
	},

	-- Orrin Vale, hullwright at Lantern Wharf
	{
		id = "line.orrin_vale.01",
		npc = ids.ORRIN,
		kind = "upgrade_fitted",
		specific = 2,
		when = function(f, _)
			return f.subjects.other == "upgrade.nav_chip"
		end,
		text = function(_, _)
			return "The chip's settling in. Ships take a few dockings to trust a new brain."
		end,
	},
	{
		id = "line.orrin_vale.02",
		npc = ids.ORRIN,
		kind = "trait_earned",
		specific = 2,
		when = function(f, _)
			return f.subjects.other == "trait.rimesight"
		end,
		text = function(f, _)
			return `{util.amount(f)} rocks worked to dust. You find the good face first now. I've watched you.`
		end,
	},
	{
		id = "line.orrin_vale.03",
		npc = ids.ORRIN,
		kind = "trait_earned",
		specific = 2,
		when = function(f, _)
			return f.subjects.other == "trait.ringwise"
		end,
		text = function(_, _)
			return "You don't dock any more, you arrive. The ring knows you."
		end,
	},
	{
		id = "line.orrin_vale.04",
		npc = ids.ORRIN,
		kind = "trait_earned",
		specific = 2,
		when = function(f, _)
			return f.subjects.other == "trait.market_ear"
		end,
		text = function(_, _)
			return "You stop selling before the price does. That's an ear, that is."
		end,
	},
	{
		id = "line.orrin_vale.05",
		npc = ids.ORRIN,
		kind = "trait_earned",
		specific = 2,
		when = function(f, _)
			return f.subjects.other == "trait.lanerunner"
		end,
		text = function(f, _)
			return `{util.amount(f)} times through the lanes. The lamps lean toward you a bit.`
		end,
	},
	{
		id = "line.orrin_vale.06",
		npc = ids.ORRIN,
		kind = "trait_earned",
		specific = 2,
		when = function(f, _)
			return f.subjects.other == "trait.good_neighbour"
		end,
		text = function(_, _)
			return "Half the Shallows has your name in their ledgers. Nicely."
		end,
	},
	{
		id = "line.orrin_vale.07",
		npc = ids.ORRIN,
		kind = "hull_rank",
		specific = 3,
		when = function(f, _)
			return f.subjects.other == ids.WREN and f.amount == 3
		end,
		text = function(_, _)
			return "Three stripes on the Wren. She's properly yours now."
		end,
	},
	{
		id = "line.orrin_vale.08",
		npc = ids.ORRIN,
		kind = "tender_acquired",
		specific = 1,
		text = function(_, _)
			return "Tamsin's tender? Sound little hull. Don't let it get ideas."
		end,
	},
	{
		id = "line.orrin_vale.09",
		npc = ids.ORRIN,
		kind = "prestige",
		specific = 1,
		text = function(_, _)
			return "The Wren's on berth six. She'll be fine. I talk to her."
		end,
	},
	{
		id = "line.orrin_vale.10",
		npc = ids.ORRIN,
		kind = "trait_earned",
		specific = 1,
		text = function(f, ctx)
			return `You've picked up a habit, {ctx.beats:pilot().name}: {trait_name(ctx, f)}. Nobody taught you that. It suits you.`
		end,
	},
	{
		id = "line.orrin_vale.11",
		npc = ids.ORRIN,
		kind = "beat_completed",
		specific = 2,
		beat = true,
		when = function(f, _)
			return f.subjects.other == ids.B14
		end,
		text = function(_, _)
			return "You fly her like you know her now. So: is the Wren still the ship for you?"
		end,
	},

	-- Odo Fenn, head grower and dockmaster at Kettle Dock
	{
		id = "line.odo_fenn.01",
		npc = ids.ODO,
		kind = "deliver",
		specific = 2,
		when = function(f, _)
			return f.subjects.other == ids.GREENHOUSE
		end,
		text = function(f, _)
			return `{util.amount(f)} ice. The beans will hear about this.`
		end,
	},
	{
		id = "line.odo_fenn.02",
		npc = ids.ODO,
		kind = "core.stage_built",
		specific = 3,
		when = function(f, _)
			return f.subjects.station == ids.KETTLE and f.amount == 1
		end,
		text = function(_, _)
			return "First pane's up. It fogs in the mornings. Well. When we call it morning."
		end,
	},
	{
		id = "line.odo_fenn.05",
		npc = ids.ODO,
		kind = "core.shortage_helped",
		specific = 2,
		when = function(f, _)
			return f.amount == 2
		end,
		text = function(_, _)
			return "Rimewater's resting. So am I, a little."
		end,
	},
	{
		id = "line.odo_fenn.06",
		npc = ids.ODO,
		kind = "core.shortage_helped",
		specific = 2,
		when = function(f, _)
			return f.amount == 1
		end,
		text = function(_, _)
			return "Ice from outside the Shallows. Tastes different. The tomatoes don't mind."
		end,
	},
	{
		id = "line.odo_fenn.07",
		npc = ids.ODO,
		kind = "patient_sold",
		specific = 3,
		when = function(f, ctx)
			return f.subjects.station == ids.KETTLE and players_tender(ctx, f)
		end,
		text = function(f, ctx)
			return `{craft_name(ctx, f.subjects.ship)} sat with us a while and sold when it should. Polite little thing.`
		end,
	},
	{
		id = "line.odo_fenn.08",
		npc = ids.ODO,
		kind = "core.shortage_started",
		specific = 1,
		beat = true,
		text = function(_, _)
			return "Rimewater's thin and our tanks are thinner. I've two ideas, if you've a minute."
		end,
	},

	-- Tamsin Oddway, tender keeper at Kettle Dock
	{
		id = "line.tamsin_oddway.01",
		npc = ids.TAMSIN,
		kind = "patient_wait",
		specific = 2,
		when = function(f, _)
			return f.subjects.ship == ids.LITTLE_PAIL
		end,
		text = function(_, _)
			return "See Little Pail sitting there? She's not stuck. She's listening."
		end,
	},
	{
		id = "line.tamsin_oddway.02",
		npc = ids.TAMSIN,
		kind = "patient_sold",
		specific = 2,
		beat = true,
		when = function(f, _)
			return f.subjects.ship == ids.LITTLE_PAIL
		end,
		text = function(f, _)
			return `{pulses(f)} ships came and went, and she sold at {avg(f)}. No clock in her at all.`
		end,
	},
	{
		id = "line.tamsin_oddway.03",
		npc = ids.TAMSIN,
		kind = "loop_lap",
		specific = 3,
		when = function(f, _)
			return f.subjects.ship == ids.PLAYER and util.amount(f) >= 4
		end,
		text = function(f, _)
			return `You've run that loop {util.amount(f)} times. Ready for something to run it for you?`
		end,
	},
	{
		id = "line.tamsin_oddway.04",
		npc = ids.TAMSIN,
		kind = "market_flooded",
		specific = 2,
		when = function(f, _)
			return f.subjects.ship == ids.PLAYER
		end,
		text = function(f, ctx)
			return `You emptied a whole hold into {station_name(ctx, f.subjects.station)}. A tender would've waited.`
		end,
	},
	{
		id = "line.tamsin_oddway.06",
		npc = ids.TAMSIN,
		kind = "patient_sold",
		specific = 2,
		when = function(f, ctx)
			return players_tender(ctx, f)
		end,
		text = function(f, ctx)
			return `{craft_name(ctx, f.subjects.ship)} waited {pulses(f)} dockings at {station_name(ctx, f.subjects.station)} and got {avg(f)} a unit. Told you.`
		end,
	},
	{
		id = "line.tamsin_oddway.07",
		npc = ids.TAMSIN,
		kind = "loop_stalled",
		specific = 2,
		when = function(f, ctx)
			return players_tender(ctx, f)
		end,
		text = function(f, ctx)
			return `{craft_name(ctx, f.subjects.ship)}'s sitting at {station_name(ctx, f.subjects.station)} with nothing to do. Its loop's run dry.`
		end,
	},
	{
		id = "line.tamsin_oddway.08",
		npc = ids.TAMSIN,
		kind = "sell",
		specific = 2,
		beat = true,
		when = function(f, _)
			return f.subjects.ware == ids.ICE and f.subjects.station == ids.KETTLE
		end,
		text = function(_, _)
			return "Kettle's had its fill of ice just now. Little Pail will show you when it's thirsty again."
		end,
	},
	{
		id = "line.tamsin_oddway.09",
		npc = ids.TAMSIN,
		kind = "patient_blocked",
		specific = 2,
		when = function(f, ctx)
			return players_tender(ctx, f)
		end,
		text = function(f, ctx)
			return `{craft_name(ctx, f.subjects.ship)} won't sell {blocked_ware(ctx, f)} below {floor_of(f)}, and {station_name(
				ctx,
				f.subjects.station
			)} can't pay more than {util.cr(util.amount(f))} cr now. Your call: a new floor, a new loop, or home.`
		end,
	},
	{
		id = "line.tamsin_oddway.10",
		npc = ids.TAMSIN,
		kind = "patient_unblocked",
		specific = 2,
		when = function(f, ctx)
			return players_tender(ctx, f)
		end,
		text = function(f, ctx)
			return `{station_name(ctx, f.subjects.station)}'s paying enough again. {craft_name(ctx, f.subjects.ship)} has gone back to selling.`
		end,
	},
	{
		id = "line.tamsin_oddway.11",
		npc = ids.TAMSIN,
		kind = "core.demo_floor_revised",
		specific = 2,
		beat = true,
		text = function(_, _)
			return "The market moved under Little Pail. I've set her a new mark; watch her again."
		end,
	},
	{
		id = "line.tamsin_oddway.12",
		npc = ids.TAMSIN,
		kind = "core.shortage_started",
		specific = 1,
		beat = true,
		text = function(_, _)
			return "Not while the Shallows are short of ice. I'll show you once it settles."
		end,
	},

	-- Rook Adeyemi, depot master at Cinder Hook
	{
		id = "line.rook_adeyemi.01",
		npc = ids.ROOK,
		kind = "deliver",
		specific = 2,
		when = function(f, _)
			return f.subjects.other == ids.CONSIGNMENT
		end,
		text = function(_, _)
			return "Ilse's plate. Tell her it's already holding up a wall."
		end,
	},
	{
		id = "line.rook_adeyemi.02",
		npc = ids.ROOK,
		kind = "core.side_chosen",
		specific = 2,
		when = function(f, _)
			return f.subjects.faction == ids.COOP
		end,
		text = function(_, _)
			return "Cooperative plate, is it. Noted. Everyone's noting."
		end,
	},
	{
		id = "line.rook_adeyemi.03",
		npc = ids.ROOK,
		kind = "core.side_chosen",
		specific = 2,
		when = function(f, _)
			return f.subjects.faction == ids.COMPACT
		end,
		text = function(_, _)
			return "Compact name on your plate. Saffron will like that. I like the plate."
		end,
	},
	{
		id = "line.rook_adeyemi.04",
		npc = ids.ROOK,
		kind = "core.territory_changed",
		specific = 1,
		text = function(_, _)
			return "New banner. Same ore. Same me."
		end,
	},
	{
		id = "line.rook_adeyemi.05",
		npc = ids.ROOK,
		kind = "core.rumor_resolved",
		specific = 2,
		when = function(f, _)
			return f.amount == 0
		end,
		text = function(_, _)
			return "Empty Shoals? Old maps. Somebody's always selling old maps."
		end,
	},
	{
		id = "line.rook_adeyemi.07",
		npc = ids.ROOK,
		kind = "core.dispute_started",
		specific = 1,
		beat = true,
		text = function(_, _)
			return "Two banners want my depot. I want plate. Sell me plate in whose name you like, or nobody's."
		end,
	},
	{
		id = "line.rook_adeyemi.06",
		npc = ids.ROOK,
		kind = "rock_depleted",
		specific = 3,
		when = function(f, ctx)
			return f.subjects.ship == ids.PLAYER
				and is_rock(f, "rock.cinder.iron.")
				and n_of(ctx, { kind = "rock_depleted", ship = ids.PLAYER }) >= 10
		end,
		text = function(_, _)
			return "You're eating my belt one rock at a time. Leave some for the Hook."
		end,
	},

	-- Juno Brask, prospector at Cinder Hook
	{
		id = "line.juno_brask.03",
		npc = ids.JUNO,
		kind = "rock_depleted",
		specific = 2,
		when = function(f, _)
			return f.subjects.ship == ids.PLAYER
				and (is_rock(f, ids.GLASS_DRIFT_ROCKS) or is_rock(f, ids.EMBER_SHOALS_ROCKS))
		end,
		text = function(_, _)
			return "Glassite rocks glint when they go. Did you see it?"
		end,
	},
	{
		id = "line.juno_brask.04",
		npc = ids.JUNO,
		kind = "core.fragment_found",
		specific = 1,
		text = function(_, _)
			return "A rock with a voice in it. Didn't I say? I definitely said."
		end,
	},
	{
		id = "line.juno_brask.05",
		npc = ids.JUNO,
		kind = "core.rumor_heard",
		specific = 1,
		text = function(_, _)
			return "Ember Shoals, is it. I've heard that one before. Sometimes it's even true."
		end,
	},

	-- Saffron Vey, Compact factor at Brassmouth Exchange
	{
		id = "line.saffron_vey.01",
		npc = ids.SAFFRON,
		kind = "sell",
		specific = 4,
		when = function(f, ctx)
			return f.subjects.ship == ids.PLAYER
				and f.subjects.ware == ids.GLASSITE
				and f.subjects.station == ids.BRASS
				and n_of(ctx, { kind = "sell", ship = ids.PLAYER, ware = ids.GLASSITE, station = ids.BRASS, ["until"] = f.tick - 1 })
					== 0
		end,
		text = function(_, _)
			return "Glassite. Lovely. The Exchange will remember your name."
		end,
	},
	{
		id = "line.saffron_vey.02",
		npc = ids.SAFFRON,
		kind = "market_flooded",
		specific = 3,
		when = function(f, _)
			return f.subjects.ware == ids.GLASSITE and f.subjects.station == ids.BRASS and f.subjects.ship == ids.PLAYER
		end,
		text = function(f, _)
			return `{util.amount(f)} glassite in one go. Generous. Our buyers needed a lie-down.`
		end,
	},
	{
		id = "line.saffron_vey.03",
		npc = ids.SAFFRON,
		kind = "core.side_chosen",
		specific = 2,
		when = function(f, _)
			return f.subjects.faction == ids.COMPACT
		end,
		text = function(_, _)
			return "The Compact appreciates plate with its name on it."
		end,
	},
	{
		id = "line.saffron_vey.04",
		npc = ids.SAFFRON,
		kind = "core.side_chosen",
		specific = 2,
		when = function(f, _)
			return f.subjects.faction == ids.COOP
		end,
		text = function(_, _)
			return "Cooperative plate at the Hook. Well. Ledgers are long."
		end,
	},
	{
		id = "line.saffron_vey.05",
		npc = ids.SAFFRON,
		kind = "core.shortage_resolved",
		specific = 2,
		when = function(f, _)
			return f.subjects.faction == ids.COMPACT
		end,
		text = function(_, _)
			return "Our barges kept Kettle Dock's taps running. Do mention it to people."
		end,
	},
	{
		id = "line.saffron_vey.06",
		npc = ids.SAFFRON,
		kind = "prestige",
		specific = 2,
		when = function(f, _)
			return f.subjects.other == "hull.heron"
		end,
		text = function(_, _)
			return "A Heron. You'll read my prices from across the Narrows. Do be kind."
		end,
	},

	-- Pip Calloway, dockhand at Brassmouth Exchange
	{
		id = "line.pip_calloway.01",
		npc = ids.PIP,
		kind = "core.rumor_heard",
		specific = 2,
		beat = true,
		text = function(f, _)
			local source = util.note(f)
			if string.find(source, "Ninefold", 1, true) then
				return "A surveyor off the Ninefold says the Ember Shoals are thick with glassite. Thick!"
			end
			return `{source} says the Ember Shoals are thick with glassite. Thick!`
		end,
	},
	{
		id = "line.pip_calloway.02",
		npc = ids.PIP,
		kind = "core.rumor_resolved",
		specific = 2,
		when = function(f, _)
			return f.amount == 1
		end,
		text = function(_, _)
			return "It was true? I knew it. I said so, didn't I?"
		end,
	},
	{
		id = "line.pip_calloway.03",
		npc = ids.PIP,
		kind = "core.rumor_resolved",
		specific = 2,
		when = function(f, _)
			return f.amount == 0
		end,
		text = function(_, ctx)
			local heard = ctx.history:last({ kind = "core.rumor_heard", other = ids.RUMOR })
			local source = string.lower(if heard then util.note(heard) else "a captain")
			return `Empty? Oh. I had it off {source}. Old news, then. I'm sorry. Truly.`
		end,
	},
	{
		id = "line.pip_calloway.04",
		npc = ids.PIP,
		kind = "jump",
		specific = 3,
		when = function(f, ctx)
			return f.subjects.ship == ids.PLAYER and n_of(ctx, { kind = "jump", ship = ids.PLAYER }) >= 20
		end,
		text = function(_, _)
			return "You're through that lane more often than the barges are."
		end,
	},
	{
		id = "line.pip_calloway.05",
		npc = ids.PIP,
		kind = "sell",
		specific = 3,
		when = function(f, ctx)
			return f.subjects.ship == ids.PLAYER
				and f.subjects.station == ids.BRASS
				and n_of(ctx, { kind = "sell", ship = ids.PLAYER, station = ids.BRASS }) >= 5
		end,
		text = function(_, _)
			return "Saffron says you're good for the books. From her that's practically a hug."
		end,
	},

	-- Nell Quillon, archivist at the Glass Drift Observatory
	{
		id = "line.nell_quillon.01",
		npc = ids.NELL,
		kind = "dock",
		specific = 4,
		when = function(f, ctx)
			return f.subjects.ship == ids.PLAYER
				and f.subjects.station == ids.GDO
				and n_of(ctx, { kind = "dock", ship = ids.PLAYER, station = ids.GDO, ["until"] = f.tick - 1 }) == 0
		end,
		text = function(_, _)
			return "A visitor! Did you bring ice or questions? Both are welcome."
		end,
	},
	{
		id = "line.nell_quillon.02",
		npc = ids.NELL,
		kind = "core.fragment_found",
		specific = 1,
		beat = true,
		text = function(_, _)
			return "You found it in a rock you mined yourself? Not bought? Not given? Good."
		end,
	},
	{
		id = "line.nell_quillon.03",
		npc = ids.NELL,
		kind = "core.shown",
		specific = 2,
		when = function(f, ctx)
			local npc = if f.subjects.npc then ctx.npcs:get(f.subjects.npc) else nil
			return npc ~= nil and npc.faction ~= ids.LAMP
		end,
		text = function(_, _)
			return "I hear you've found something. I'd have liked to hear it from you."
		end,
	},
	{
		id = "line.nell_quillon.04",
		npc = ids.NELL,
		kind = "core.canon_opened",
		specific = 2,
		beat = true,
		when = function(f, _)
			return f.subjects.other == "canon.fragments_sing"
		end,
		text = function(_, _)
			return "It sang in the lane. We thought they all would. Now we know one did."
		end,
	},
	{
		id = "line.nell_quillon.05",
		npc = ids.NELL,
		kind = "rock_depleted",
		specific = 3,
		when = function(f, ctx)
			if f.subjects.ship ~= ids.PLAYER or not is_rock(f, ids.GLASS_DRIFT_ROCKS) then
				return false
			end
			local n = 0
			for _, d in ipairs(util.find_all(ctx, { kind = "rock_depleted", ship = ids.PLAYER }, nil, 200)) do
				if is_rock(d, ids.GLASS_DRIFT_ROCKS) then
					n += 1
				end
			end
			return n >= 6
		end,
		text = function(_, _)
			return "Every rock in the Drift is a crumb of something bigger. Mind how you crumble them."
		end,
	},
	{
		id = "line.nell_quillon.06",
		npc = ids.NELL,
		kind = "prestige",
		specific = 2,
		when = function(f, _)
			return f.subjects.other == "hull.heron"
		end,
		text = function(_, _)
			return "A Heron! Survey notes, please. As many as you can bear."
		end,
	},
	{
		id = "line.nell_quillon.07",
		npc = ids.NELL,
		kind = "core.shown",
		specific = 2,
		beat = true,
		when = function(f, _)
			return f.subjects.npc == ids.NELL
		end,
		text = function(_, _)
			return "It's warm. Will you carry it through a lane for me, and listen? Any lane will do."
		end,
	},

	-- Hesper Lune, keeper and dockmaster at the Glass Drift Observatory
	{
		id = "line.hesper_lune.01",
		npc = ids.HESPER,
		kind = "jump",
		specific = 3,
		when = function(f, ctx)
			return f.subjects.ship == ids.PLAYER and n_of(ctx, { kind = "jump", ship = ids.PLAYER }) >= 10
		end,
		text = function(_, _)
			return "The lane lamps know your hull now. They brighten a little when you pass."
		end,
	},
	{
		id = "line.hesper_lune.02",
		npc = ids.HESPER,
		kind = "mind.rumor_spread",
		specific = 2,
		when = function(f, _)
			return f.subjects.station == ids.GDO
		end,
		text = function(_, _)
			return "Rumours come here to be weighed. Most of them are light."
		end,
	},
	{
		id = "line.hesper_lune.03",
		npc = ids.HESPER,
		kind = "regard_changed",
		specific = 3,
		when = function(f, _)
			return f.subjects.faction == ids.LAMP and util.event(f).to == "trusted"
		end,
		text = function(_, _)
			return "The Society asked me to say it: you are welcome at any lamp."
		end,
	},
	{
		id = "line.hesper_lune.04",
		npc = ids.HESPER,
		kind = "core.territory_changed",
		specific = 1,
		text = function(_, _)
			return "The Hook changed banners. The lanes don't mind who owns the ore."
		end,
	},
	{
		id = "line.hesper_lune.06",
		npc = ids.HESPER,
		kind = "core.rumor_resolved",
		specific = 3,
		when = function(f, _)
			return f.amount == 0 and f.subjects.ship == nil
		end,
		text = function(_, _)
			return "The Shoals were empty, they say. Most rumours are."
		end,
	},

	-- Any dockmaster
	{
		id = "line.any_dockmaster.01",
		npc = "any_dockmaster",
		kind = "patient_blocked",
		specific = 2,
		when = function(f, ctx)
			return players_tender(ctx, f)
		end,
		text = function(f, ctx)
			return `Your {craft_name(ctx, f.subjects.ship)} is sitting at {station_name(ctx, f.subjects.station)} on {blocked_ware(
				ctx,
				f
			)} at a floor of {floor_of(f)}. They can't pay that now. Tell it what you'd like.`
		end,
	},
}

local M = {}

M.lines = lines

-- A line by ID. Unknown IDs are an authoring error.
function M.get(id: string): Line
	for _, l in ipairs(lines) do
		if l.id == id then
			return l
		end
	end
	error(`no line {id}`)
end

-- Whether `line` can cite `f`.
function M.fits(line: Line, f: G.Fact, ctx: G.Ctx): boolean
	if f.kind ~= line.kind then
		return false
	end
	local when = line.when
	return when == nil or when(f, ctx)
end

-- The newest fact `line` can cite, among facts after `after` (nil: all history).
function M.newest(ctx: G.Ctx, line: Line, after: string?): G.Fact?
	local facts = util.find_all(ctx, { kind = line.kind }, after, 400)
	for i = #facts, 1, -1 do
		if M.fits(line, facts[i], ctx) then
			return facts[i]
		end
	end
	return nil
end

-- Who says a line at `station`: its NPC if they're there, or the station's dockmaster
-- for "any_dockmaster" lines. nil if nobody there can say it.
function M.speaker(ctx: G.Ctx, line: Line, station: string): string?
	for _, npc in ipairs(ctx.npcs:at(station)) do
		if npc.id == line.npc or (line.npc == "any_dockmaster" and npc.role == "dockmaster") then
			return npc.id
		end
	end
	return nil
end

-- Candidate lines for dock talk at `station`: ambient lines whose speaker is there,
-- each with the newest fact after `after` it can cite and hasn't already cited. Most
-- specific first, then the newest fact.
export type Pick = { line: Line, fact: G.Fact, npc: string }
function M.candidates(ctx: G.Ctx, station: string, after: string?, include_beat: boolean?): { Pick }
	local out: { Pick } = {}
	-- One history scan per kind, shared by every line that cites it.
	local by_kind: { [string]: { G.Fact } } = {}
	for _, line in ipairs(lines) do
		local npc = M.speaker(ctx, line, station)
		if npc ~= nil and (include_beat or not line.beat) then
			local facts = by_kind[line.kind]
			if facts == nil then
				facts = util.find_all(ctx, { kind = line.kind }, after, 100)
				by_kind[line.kind] = facts
			end
			for i = #facts, 1, -1 do
				local f = facts[i]
				if M.fits(line, f, ctx) then
					if not util.said(ctx, line.id, f.id) and not ctx.talk:cited(f.id) then
						table.insert(out, { line = line, fact = f, npc = npc })
					end
					break
				end
			end
		end
	end
	table.sort(out, function(a: Pick, b: Pick): boolean
		if a.line.specific ~= b.line.specific then
			return a.line.specific > b.line.specific
		end
		if a.fact.id ~= b.fact.id then
			return util.seq(a.fact.id) > util.seq(b.fact.id)
		end
		return a.line.id < b.line.id
	end)
	return out
end

-- Says a picked line, citing its fact.
function M.say(ctx: G.Ctx, why: G.Cause, pick: Pick, place: string, choices: { G.TalkChoice }?)
	util.say(ctx, why, pick.line.id, pick.npc, place, pick.line.text(pick.fact, ctx), pick.fact.id, choices)
end

return M
