--!strict
-- b13, the first tender (content/04-journey.md, "The tender lesson"). Tamsin's own
-- tender, Little Pail, shows a patient sell with twelve of her ice at Kettle Dock; then
-- she builds the player Thimble, and Thimble runs one of the player's loops with a
-- floored sell.
--
-- The demonstration never fakes the market: it starts only at a Kettle docking where
-- Kettle's ice saturation is low enough for the price to dip (Tamsin says so
-- otherwise), never while the Shallows are short of ice, and if the market moves
-- under Little Pail mid-demonstration, Tamsin sets her a new floor at the player's
-- next Kettle docking (her own tender only: a floor the player sets is never lowered
-- for them).
local G = require("@galaxy")
local ids = require("../lib/ids")
local util = require("../lib/util")
local lines = require("../lib/lines")

local function demo(ctx: G.Ctx): string?
	local v = ctx.store:get("demo", "since")
	return if type(v) == "string" and v ~= "" then v else nil
end

-- Little Pail's `patient_sold` since the demonstration began, if any.
local function demo_sold(ctx: G.Ctx): G.Fact?
	local since = demo(ctx)
	if since == nil then
		return nil
	end
	return ctx.history:last({ kind = "patient_sold", ship = ids.LITTLE_PAIL, after = since })
end

-- A floor at `tender_demo.floor_share` of what Kettle pays for ice now.
local function demo_floor(ctx: G.Ctx): number?
	local line = ctx.market:line(ids.KETTLE, ids.ICE)
	if line == nil then
		return nil
	end
	return math.floor(line.price * util.num(ctx, "tender_demo.floor_share"))
end

local function sell_order(floor: number): { G.Order }
	return { { verb = "sell", ware = ids.ICE, quantity = { kind = "all" }, floor = floor } }
end

-- The newest market change at Kettle's ice since `after`: the shortage starting or
-- ending, or any lever that moved the ice base there (a `market_base_changed` fact,
-- whoever pulled it: the shortage, a game-master pack).
local function market_moved(ctx: G.Ctx, after: string): G.Fact?
	local best: G.Fact? = nil
	local queries: { G.HistoryQuery } = {
		{ kind = "core.shortage_started", after = after },
		{ kind = "core.shortage_resolved", after = after },
		{ kind = "market_base_changed", station = ids.KETTLE, ware = ids.ICE, after = after },
	}
	for _, q in ipairs(queries) do
		local f = ctx.history:last(q)
		if f ~= nil and (best == nil or util.seq(f.id) > util.seq(best.id)) then
			best = f
		end
	end
	return best
end

-- A lap of Thimble's that included a floored sell that finished its stack, since the
-- current step began.
local function thimble_lapped(ctx: G.Ctx): boolean
	local s = ctx.beats:state(ids.B13)
	if s == nil then
		return false
	end
	local sold = ctx.history:find({ kind = "patient_sold", ship = ids.THIMBLE, after = s.step_since, limit = 1 })
	if #sold == 0 then
		return false
	end
	return ctx.history:count({ kind = "loop_lap", ship = ids.THIMBLE, after = sold[1].id }) > 0
end

return G.script({
	id = "tender",
	beats = {
		G.beat({
			id = ids.B13,
			act = 3,
			giver = ids.TAMSIN,
			goal = "Tamsin Oddway at Kettle Dock wants to show you her tender.",
			requires = {
				G.done(ids.B11),
				G.fact({ kind = "loop_lap", ship = ids.PLAYER }, { at_least = G.tunable("b13.loop_laps") }),
			},
			steps = {
				{
					id = "show",
					goal = "Tamsin Oddway at Kettle Dock wants to show you her tender.",
					done = G.fact({ kind = "core.mechanic_taught", npc = ids.TAMSIN }, { since = "start" }),
				},
				{
					id = "build",
					goal = "Tamsin will build you a tender for 2400 credits.",
					done = G.owns(ids.THIMBLE),
				},
				{
					id = "assign",
					goal = "Give Thimble one of your loops and a floor price.",
					-- Data first: the predicate runs only once Thimble has sold patiently
					-- and lapped.
					done = {
						G.fact({ kind = "patient_sold", ship = ids.THIMBLE }, { since = "step" }),
						G.fact({ kind = "loop_lap", ship = ids.THIMBLE }, { since = "step" }),
						G.test(thimble_lapped),
					},
				},
			},
			-- Building Thimble already set `cap.automation` (upgrades.toml), so step 3
			-- can assign it; the Cooperative's +3 is the tender deed (factions.toml).
			rewards = { flags = { "cap.automation" } },
			on_start = function(ctx: G.Ctx, beat: G.BeatInfo)
				-- Little Pail waits at Kettle with twelve of Tamsin's ice.
				if ctx.space:craft(ids.LITTLE_PAIL) == nil then
					ctx.space:put_craft({
						id = ids.LITTLE_PAIL,
						name = "Little Pail",
						hull = ids.TENDER,
						npc = ids.TAMSIN,
						station = ids.KETTLE,
						cargo = { [ids.ICE] = util.num(ctx, "tender_demo.ice") },
					}, beat.why)
				end
			end,
		}),
	},
	on = {
		dock = function(ev: G.DockEvent, ctx: G.Ctx)
			if ev.ship ~= ids.PLAYER or ev.station ~= ids.KETTLE or util.step(ctx, ids.B13) ~= 0 then
				return
			end
			if ctx.space:craft(ids.LITTLE_PAIL) == nil then
				return
			end
			local since = demo(ctx)
			-- The demonstration finished: Tamsin says how it went (line.tamsin_oddway.02,
			-- citing Little Pail's `patient_sold`) and the lesson is taught.
			local sold = demo_sold(ctx)
			if sold ~= nil then
				local why = ctx:cause({ summary = "Tamsin shows what Little Pail did", cites = { sold.id } })
				lines.say(
					ctx,
					why,
					{ line = lines.get("line.tamsin_oddway.02"), fact = sold, npc = ids.TAMSIN },
					ids.KETTLE
				)
				ctx.talk:offer({
					speaker = ids.TAMSIN,
					place = ids.KETTLE,
					text = "Rings only let crewed ships trade and go. Keeps the ring clear. A tender's got no crew to get bored, so we give it a standing berth. It can sit there and listen to the dock for as long as it likes.",
					cites = { sold.id },
				}, why)
				-- Teaching is a checked change: only `ctx.mechanics:teach` records
				-- `core.mechanic_taught` (npc = Tamsin, other = the mechanic).
				ctx.mechanics:teach({ mechanic = ids.TENDER_MECH, teacher = ids.TAMSIN }, why)
				return
			end
			-- No demonstration while the Shallows are short of ice
			-- (line.tamsin_oddway.12, citing `shortage_started`).
			local short = ctx.beats:is_active(ids.B12)
			if since == nil and short then
				local f = ctx.history:last({ kind = "core.shortage_started", other = ids.SHORTAGE })
				if f ~= nil then
					local why = ctx:cause({ summary = "Tamsin waits for the ice to settle", cites = { f.id } })
					lines.say(
						ctx,
						why,
						{ line = lines.get("line.tamsin_oddway.12"), fact = f, npc = ids.TAMSIN },
						ids.KETTLE
					)
				end
				return
			end
			local line = ctx.market:line(ids.KETTLE, ids.ICE)
			local floor = demo_floor(ctx)
			if line == nil or floor == nil then
				return
			end
			if since == nil then
				-- Kettle has had its fill of ice: a sale wouldn't dip enough to show
				-- anything (line.tamsin_oddway.08, citing the newest ice sale there).
				if (line.saturation or 0) > util.num(ctx, "tender_demo.max_sat") then
					local f = ctx.history:last({ kind = "sell", station = ids.KETTLE, ware = ids.ICE })
					if f ~= nil then
						local why = ctx:cause({ summary = "Tamsin waits for Kettle to be thirsty", cites = { f.id } })
						lines.say(
							ctx,
							why,
							{ line = lines.get("line.tamsin_oddway.08"), fact = f, npc = ids.TAMSIN },
							ids.KETTLE
						)
					end
					return
				end
				-- The demonstration starts: Little Pail sells her ice with a floor at 90%
				-- of what Kettle pays now.
				local why = ctx:cause({ summary = "Little Pail shows a patient sell", cites = { ev.fact } })
				ctx.orders:give(ids.LITTLE_PAIL, sell_order(floor), why)
				ctx.store:set("demo", "since", ev.fact, why)
				util.record(
					ctx,
					why,
					"demo_started",
					{ ship = ids.LITTLE_PAIL, station = ids.KETTLE, ware = ids.ICE },
					floor,
					`Little Pail sells Tamsin's ice at Kettle Dock, floor {util.cr(floor)} cr`
				)
				return
			end
			-- The market moved under Little Pail while she waited: Tamsin revises her own
			-- floor to 90% of the price now and keeps the cargo (line.tamsin_oddway.11,
			-- citing the market change).
			local moved = market_moved(ctx, since)
			local pail = ctx.space:craft(ids.LITTLE_PAIL)
			if moved ~= nil and pail ~= nil and (pail.cargo[ids.ICE] or 0) > 0 then
				local why = ctx:cause({ summary = "Tamsin sets Little Pail a new mark", cites = { moved.id } })
				ctx.orders:give(ids.LITTLE_PAIL, sell_order(floor), why)
				ctx.store:set("demo", "since", ev.fact, why)
				util.record(
					ctx,
					why,
					"demo_floor_revised",
					{ ship = ids.LITTLE_PAIL, station = ids.KETTLE, ware = ids.ICE },
					floor,
					`Little Pail's floor is now {util.cr(floor)} cr`
				)
				lines.say(
					ctx,
					why,
					{ line = lines.get("line.tamsin_oddway.11"), fact = moved, npc = ids.TAMSIN },
					ids.KETTLE
				)
			end
		end,
	},
})
