--!strict
-- Act 2's directed beats, b06 to b08 (content/04-journey.md): the nav chip, Ilse's
-- consignment to Cinder Hook (and the lane chart Rook offers for it), and chasing the
-- glassite gap at Brassmouth. The dispute (b09) and the rumor (b10) are their own
-- scripts.
--
-- The consignment is someone else's cargo, so every step is the player's: Ilse asks
-- (Accept / Not now), the plate rides in the hold as Ilse's (no market buys it), and
-- Rook asks for it at Cinder Hook (Deliver / Not yet). b07 completes only when that
-- consignment reaches him (`core.consignment_delivered`), never on any 20 plate.
local G = require("@galaxy")
local ids = require("../lib/ids")
local util = require("../lib/util")
local lines = require("../lib/lines")

-- Ilse's plate, if the player carries it.
local function carried(ctx: G.Ctx): G.Consigned?
	return ctx.beats:pilot().consigned[ids.CONSIGNMENT]
end

-- Whether b07 waits on the player to take Ilse's plate: its first step, or (a save
-- from before consignments, whose plate went aboard as cargo) its second with nothing
-- of hers aboard.
local function waiting(ctx: G.Ctx): boolean
	local step = util.step(ctx, ids.B07)
	return (step == 0 or step == 1) and carried(ctx) == nil
end

-- Ilse asks the player to carry the plate, while b07 waits for an answer and the
-- player is at the Wharf. `cite` is the fact that brought it up (the beat starting, a
-- docking, a sale that made room).
local function ask_to_carry(ctx: G.Ctx, cite: string)
	if not waiting(ctx) then
		return
	end
	if ctx.beats:pilot().docked_at ~= ids.WHARF then
		return
	end
	local plate = util.num(ctx, "b07.plate")
	local why = ctx:cause({ summary = "Ilse asks the player to carry plate to Rook", cites = { cite } })
	ctx.store:set("b07", "room", false, why)
	ctx.talk:offer({
		speaker = ids.ILSE,
		place = ids.WHARF,
		text = `Will you carry {plate} plate to Rook at Cinder Hook for me? It stays mine until he has it, and he'll pay you when it's there.`,
		cites = { cite },
		choices = {
			{ id = ids.ACCEPT_CONSIGNMENT, text = `Accept: carry {plate} plate to Rook` },
			{ id = ids.DECLINE_CONSIGNMENT, text = "Not now" },
		},
	}, why)
end

-- Accept: Ilse's plate goes aboard as hers, and a line says so.
local function accept(ctx: G.Ctx, ev: G.ReplyEvent)
	if carried(ctx) ~= nil or ctx.beats:pilot().docked_at ~= ids.WHARF then
		return
	end
	local plate = util.num(ctx, "b07.plate")
	local pilot = ctx.beats:pilot()
	if pilot.hold_free < plate then
		local why = ctx:cause({ summary = "Ilse's plate needs room in the hold", cites = { ev.fact } })
		ctx.store:set("b07", "room", true, why)
		ctx.talk:offer({
			speaker = ids.ILSE,
			place = ids.WHARF,
			text = `You've room for {pilot.hold_free}, and it's {plate} plate. Sell or deliver what you carry, and I'll ask again.`,
			cites = { ev.fact },
		}, why)
		return
	end
	local why = ctx:cause({ summary = "Ilse loads the plate consigned to Rook", cites = { ev.fact } })
	ctx.beats:consign({
		ledger = ids.CONSIGNMENT,
		ware = ids.PLATE,
		quantity = plate,
		owner = ids.ILSE,
		to = ids.ROOK,
		station = ids.HOOK,
	}, why)
	util.record(
		ctx,
		why,
		"consigned",
		{ ship = ids.PLAYER, station = ids.WHARF, ware = ids.PLATE, other = ids.CONSIGNMENT, npc = ids.ILSE },
		plate,
		`Ilse loaded {plate} plate for Rook at Cinder Hook`
	)
	util.confirm(ctx, why, ids.WHARF, `Loaded Ilse's {plate} plate for Rook · it stays hers until he has it`, ev.fact)
end

-- Deliver: the consignment goes to Rook, and the fee comes back in the same
-- transaction.
local function hand_to_rook(ctx: G.Ctx, ev: G.ReplyEvent)
	local c = carried(ctx)
	if c == nil or ctx.beats:pilot().docked_at ~= ids.HOOK then
		return
	end
	local fee = util.num(ctx, "b07.pay")
	local why = ctx:cause({ summary = "the player hands Rook Ilse's plate", cites = { ev.fact } })
	ctx.beats:deliver_consignment(ids.CONSIGNMENT, why)
	util.record(
		ctx,
		why,
		"consignment_delivered",
		{ ship = ids.PLAYER, station = ids.HOOK, ware = c.ware, other = ids.CONSIGNMENT, npc = ids.ROOK },
		c.quantity,
		`Delivered Ilse's {c.quantity} plate to Rook`
	)
	ctx.beats:pay(fee, why)
	util.confirm(ctx, why, ids.HOOK, `Delivered Ilse's {c.quantity} plate to Rook · +{fee} cr`, ev.fact)
end

-- Rook's gift: the lane chart on his list, offered (never fitted) once b07 is done and
-- until the player fits it.
local function offer_chart(ctx: G.Ctx, cite: string)
	local chart = ctx.beats:service("upgrade.lane_chart")
	if chart == nil or chart.owned or ctx.beats:pilot().docked_at ~= ids.HOOK then
		return
	end
	local why = ctx:cause({ summary = "Rook offers the lane chart", cites = { cite } })
	ctx.talk:offer({
		speaker = ids.ROOK,
		place = ids.HOOK,
		text = `And this is from me: a lane chart. {chart.effect}. It's on my list at no charge; fit it whenever you like.`,
		cites = { cite },
	}, why)
end

-- How many separate Brassmouth dockings since b08 started had a glassite sale: counted
-- one sale at a time into the `b08` store (`brass_sale`), so a long b08 never loses
-- later visits to a history window.
local function brass_glassite_dockings(ctx: G.Ctx): number
	local n = ctx.store:get("b08", "dockings")
	return if type(n) == "number" then n else 0
end

-- A glassite sale at Brassmouth during b08: if it is the first in this visit, one
-- more docking counts. A visit starts at a counted docking: tapping out of the ring
-- and back in is the same visit. Stops once b08 has the dockings it needs.
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
	-- The visit the sale happened in: the player's newest counted Brassmouth docking.
	local dock = util.counted_dock(ctx, { kind = "dock", ship = ids.PLAYER, station = ids.BRASS }, ev.tick + 1)
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
			goal = "Orrin can fit a nav chip at the Wharf: click a point near your ship, the Wren, and she flies there.",
			requires = { G.done(ids.B05), G.counter("dockings", G.tunable("b06.dockings")) },
			-- The rock cradle and the docking clamp follow on their own unlocks.
			done = G.fitted("upgrade.nav_chip"),
		}),
		G.beat({
			id = ids.B07,
			act = 2,
			giver = ids.ILSE,
			goal = "Ilse asks you to carry 20 plate to Rook at Cinder Hook. Dock at the Wharf and pick Accept.",
			requires = G.done(ids.B06),
			steps = {
				{
					id = "accept",
					goal = "Ilse asks you to carry 20 plate to Rook at Cinder Hook. Dock at the Wharf and pick Accept.",
					done = G.fact({ kind = "core.consigned" }, { since = "start" }),
				},
				{
					id = "deliver",
					goal = "Carry Ilse's 20 plate to Rook at Cinder Hook and pick Deliver. The lane to Cinder Drift starts east of the Wharf.",
					done = G.fact({ kind = "core.consignment_delivered" }, { since = "start" }),
				},
			},
			-- Rook pays the fee (`b07.pay`) when the plate is delivered, and offers the
			-- lane chart, a gift on his list. Cinder Hook's first snapshot comes with
			-- the docking itself.
			rewards = { standing = { [ids.COOP] = 4 } },
		}),
		G.beat({
			id = ids.B08,
			act = 2,
			giver = ids.ROOK,
			goal = "Rook says Brassmouth Exchange pays well for glassite. Sell 30 there, over 2 separate visits.",
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
		-- there): Ilse asks right away, and again at each Wharf docking until they agree.
		beat_started = function(ev: G.BeatStartedEvent, ctx: G.Ctx)
			if ev.beat == ids.B07 then
				ask_to_carry(ctx, ev.fact)
			end
		end,
		sell = function(ev: G.SellEvent, ctx: G.Ctx)
			-- Room made after "sell what you carry": Ilse asks again.
			if ev.ship == ids.PLAYER and ev.station == ids.WHARF and ctx.store:get("b07", "room") == true then
				ask_to_carry(ctx, ev.fact)
			end
			brass_sale(ev, ctx)
		end,
		dock = function(ev: G.DockEvent, ctx: G.Ctx)
			if ev.ship ~= ids.PLAYER then
				return
			end
			if ev.station == ids.WHARF then
				ask_to_carry(ctx, ev.fact)
				return
			end
			if ev.station == ids.HOOK and ctx.beats:is_done(ids.B07) then
				offer_chart(ctx, ev.fact)
				return
			end
			-- At Cinder Hook with Ilse's plate aboard, Rook asks for it.
			local c = carried(ctx)
			if ev.station ~= ids.HOOK or util.step(ctx, ids.B07) ~= 1 or c == nil then
				return
			end
			if ctx.beats:pilot().docked_at ~= ids.HOOK then
				return
			end
			local why = ctx:cause({ summary = "Rook asks for Ilse's plate", cites = { ev.fact } })
			ctx.talk:offer({
				speaker = ids.ROOK,
				place = ids.HOOK,
				text = "Is that Ilse's plate in your hold? I've been waiting on it.",
				cites = { ev.fact },
				choices = {
					{ id = ids.DELIVER_PLATE, text = `Deliver Ilse's {c.quantity} plate to Rook` },
					{ id = ids.KEEP_PLATE, text = "Not yet" },
				},
			}, why)
		end,
		reply = function(ev: G.ReplyEvent, ctx: G.Ctx)
			if not ctx.beats:is_active(ids.B07) then
				return
			end
			local step = util.step(ctx, ids.B07)
			if ev.npc == ids.ILSE and ev.station == ids.WHARF and waiting(ctx) then
				if ev.choice == ids.ACCEPT_CONSIGNMENT then
					accept(ctx, ev)
				elseif ev.choice == ids.DECLINE_CONSIGNMENT then
					local why = ctx:cause({ summary = "the player isn't carrying Ilse's plate yet", cites = { ev.fact } })
					ctx.talk:offer({
						speaker = ids.ILSE,
						place = ids.WHARF,
						text = "Whenever you're ready. I'll ask again when you dock here.",
						cites = { ev.fact },
					}, why)
				end
			elseif ev.npc == ids.ROOK and ev.station == ids.HOOK and step == 1 then
				if ev.choice == ids.DELIVER_PLATE then
					hand_to_rook(ctx, ev)
				elseif ev.choice == ids.KEEP_PLATE then
					local why = ctx:cause({ summary = "the player keeps Ilse's plate aboard", cites = { ev.fact } })
					ctx.talk:offer({
						speaker = ids.ROOK,
						place = ids.HOOK,
						text = "It's Ilse's, not mine to take. I'll be here when you're ready.",
						cites = { ev.fact },
					}, why)
				end
			end
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
		-- b07 done: Rook offers the lane chart, a gift on his list. Nothing is fitted
		-- until the player fits it there.
		beat_completed = function(ev: G.BeatCompletedEvent, ctx: G.Ctx)
			if ev.beat == ids.B07 then
				offer_chart(ctx, ev.fact)
			end
		end,
	},
})
