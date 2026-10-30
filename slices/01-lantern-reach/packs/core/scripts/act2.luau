--!strict
-- Act 2's directed beats, b06 to b08 (content/04-journey.md): the nav chip, Ilse's
-- consignment to Cinder Hook (and the lane chart it earns), and chasing the glassite
-- gap at Brassmouth. The dispute (b09) and the rumor (b10) are their own scripts.
local G = require("@galaxy")
local ids = require("../lib/ids")
local util = require("../lib/util")
local lines = require("../lib/lines")

-- Consigned plate handed to Rook since b07 started.
local function delivered(ctx: G.Ctx): number
	return ctx.history:sum({ kind = "deliver", other = ids.CONSIGNMENT, after = util.since(ctx, ids.B07) })
end

-- Ilse loads the consignment at the Wharf once the hold has room for it. A consignment
-- is loaded once: `core.consigned` remembers it.
local function try_load(ctx: G.Ctx, cite: string): boolean
	if util.step(ctx, ids.B07) ~= 0 then
		return false
	end
	if ctx.history:count({ kind = "core.consigned", after = util.since(ctx, ids.B07) }) > 0 then
		return false
	end
	local pilot = ctx.beats:pilot()
	local plate = util.num(ctx, "b07.plate")
	if pilot.docked_at ~= ids.WHARF or pilot.hold_free < plate then
		return false
	end
	local why = ctx:cause({ summary = "Ilse loads the plate consigned to Rook", cites = { cite } })
	ctx.beats:load({ ware = ids.PLATE, quantity = plate }, why)
	util.record(
		ctx,
		why,
		"consigned",
		{ ship = ids.PLAYER, station = ids.WHARF, ware = ids.PLATE, other = ids.CONSIGNMENT },
		plate,
		`Ilse loaded {plate} plate for Rook at Cinder Hook`
	)
	return true
end

-- How many separate Brassmouth dockings since b08 started had a glassite sale: counted
-- one sale at a time into the `b08` store (`brass_sale`), so a long b08 never loses
-- later visits to a history window.
local function brass_glassite_dockings(ctx: G.Ctx): number
	local n = ctx.store:get("b08", "dockings")
	return if type(n) == "number" then n else 0
end

-- A glassite sale at Brassmouth during b08: if it is the first in this docking, one
-- more docking counts. Stops once b08 has the dockings it needs.
local function brass_sale(ev: G.SellEvent, ctx: G.Ctx)
	if ev.ship ~= ids.PLAYER or ev.station ~= ids.BRASS or ev.ware ~= ids.GLASSITE then
		return
	end
	if not ctx.beats:is_active(ids.B08) then
		return
	end
	local n = brass_glassite_dockings(ctx)
	if n >= util.num(ctx, "b08.dockings") then
		return
	end
	-- The docking the sale happened in: the player's newest at Brassmouth before it.
	local dock = util.before(ctx, { kind = "dock", ship = ids.PLAYER, station = ids.BRASS }, ev.tick + 1)
	local since = util.since(ctx, ids.B08)
	-- A visit that began before b08 started doesn't count (the old rule).
	if dock == nil or (since ~= nil and util.seq(dock.id) < util.seq(since)) then
		return
	end
	if ctx.store:get("b08", "dock") == dock.id then
		return
	end
	local why = ctx:cause({ summary = "a Brassmouth docking with a glassite sale", cites = { ev.fact } })
	ctx.store:set("b08", "dock", dock.id, why)
	ctx.store:set("b08", "dockings", n + 1, why)
end

return G.script({
	id = "act2",
	beats = {
		G.beat({
			id = ids.B06,
			act = 2,
			giver = ids.ORRIN,
			goal = "Orrin can fit a nav chip: then click a point near the Wren and she flies there.",
			requires = { G.done(ids.B05), G.counter("dockings", G.tunable("b06.dockings")) },
			-- The rock cradle and the docking clamp follow on their own unlocks.
			done = G.fitted("upgrade.nav_chip"),
		}),
		G.beat({
			id = ids.B07,
			act = 2,
			giver = ids.ILSE,
			goal = "Ilse has 20 plate for Rook at Cinder Hook. The lane to Cinder Drift starts east of the Wharf.",
			requires = G.done(ids.B06),
			steps = {
				{
					id = "load",
					goal = "Make room for 20 plate and Ilse will load it at the Wharf.",
					done = G.fact({ kind = "core.consigned" }, { since = "start" }),
				},
				{
					id = "deliver",
					goal = "Ilse has 20 plate for Rook at Cinder Hook. The lane to Cinder Drift starts east of the Wharf.",
					done = G.sum({ kind = "deliver", other = ids.CONSIGNMENT }, G.tunable("b07.plate"), { since = "start" }),
				},
			},
			-- The lane chart is Rook's gift; Cinder Hook's first snapshot comes with the
			-- docking itself.
			rewards = { credits = 250, standing = { [ids.COOP] = 4 }, fit = { "upgrade.lane_chart" } },
		}),
		G.beat({
			id = ids.B08,
			act = 2,
			giver = ids.ROOK,
			goal = "Rook says Brassmouth Exchange pays well for glassite. Go and see how well, twice.",
			requires = G.done(ids.B07),
			-- The Compact's +3 for the first glassite sale is a faction deed
			-- (factions.toml), so it counts however the sale happens.
			done = {
				G.sum(
					{ kind = "sell", ship = ids.PLAYER, station = ids.BRASS, ware = ids.GLASSITE },
					G.tunable("b08.glassite"),
					{ since = "start" }
				),
				G.test(function(ctx: G.Ctx): boolean
					return brass_glassite_dockings(ctx) >= util.num(ctx, "b08.dockings")
				end),
			},
		}),
	},
	on = {
		-- b07 opens while the player is often still at the Wharf (the nav chip is fitted
		-- there): load right away if there's room, else at a later docking or sale.
		beat_started = function(ev: G.BeatStartedEvent, ctx: G.Ctx)
			if ev.beat == ids.B07 then
				try_load(ctx, ev.fact)
			end
		end,
		sell = function(ev: G.SellEvent, ctx: G.Ctx)
			if ev.ship == ids.PLAYER and ev.station == ids.WHARF then
				try_load(ctx, ev.fact)
			end
			brass_sale(ev, ctx)
		end,
		dock = function(ev: G.DockEvent, ctx: G.Ctx)
			if ev.ship ~= ids.PLAYER then
				return
			end
			if ev.station == ids.WHARF then
				try_load(ctx, ev.fact)
				return
			end
			-- The handover at Cinder Hook.
			if ev.station ~= ids.HOOK or util.step(ctx, ids.B07) ~= 1 then
				return
			end
			local pilot = ctx.beats:pilot()
			if pilot.docked_at ~= ids.HOOK then
				return
			end
			local qty = math.min(util.num(ctx, "b07.plate") - delivered(ctx), pilot.cargo[ids.PLATE] or 0)
			if qty < 1 then
				return
			end
			ctx.beats:deliver(
				{ station = ids.HOOK, ware = ids.PLATE, quantity = qty, ledger = ids.CONSIGNMENT },
				ctx:cause({ summary = "the player hands Rook Ilse's plate", cites = { ev.fact } })
			)
		end,
		-- Rook says so, citing the handover (line.rook_adeyemi.01).
		deliver = function(ev: G.DeliverEvent, ctx: G.Ctx)
			if ev.ledger ~= ids.CONSIGNMENT then
				return
			end
			local f = ctx.history:fact(ev.fact)
			if f == nil then
				return
			end
			local why = ctx:cause({ summary = "Rook takes Ilse's plate", cites = { ev.fact } })
			lines.say(ctx, why, { line = lines.get("line.rook_adeyemi.01"), fact = f, npc = ids.ROOK }, ids.HOOK)
		end,
	},
})
