--!strict
-- Act 1, beats b01 to b05 (content/04-journey.md). b01 to b03 are the worked examples
-- of the beat API; the rest of the journey is authored the same way, one script per
-- act or situation.
--
-- A beat is data: preconditions, steps and an end state built from G.* requirements,
-- plus rewards. The script host checks it every tick; nothing here counts time, and
-- every threshold is a tunable from data/pacing.toml. Callbacks (`on_complete`) make
-- world changes atomically with the transition; lines that cite the beat's own facts
-- go in ordinary event handlers, where the fact exists.
local G = require("@galaxy")
local ids = require("../lib/ids")
local util = require("../lib/util")
local lines = require("../lib/lines")

-- Ice handed to the greenhouse since b03 started.
local function delivered(ctx: G.Ctx): number
	local b = ctx.beats:state(ids.B03)
	if b == nil then
		return 0
	end
	return ctx.history:sum({ kind = "deliver", other = ids.GREENHOUSE, after = b.since })
end

-- Ice the greenhouse still needs.
local function still(ctx: G.Ctx): number
	return math.max(0, util.num(ctx, "b03.ice") - delivered(ctx))
end

-- What a Deliver would hand over now: what the greenhouse needs, or what the player
-- carries if that's less.
local function handover(ctx: G.Ctx): number
	return math.min(still(ctx), ctx.beats:pilot().cargo[ids.ICE] or 0)
end

return G.script({
	id = "act1",
	beats = {
		G.beat({
			id = ids.B01,
			act = 1,
			giver = ids.ILSE,
			goal = "Ilse needs ice for Odo's greenhouse at Kettle Dock. Fly off with W, turn with A and D, and look for the pale rocks.",
			-- A new save: docked at the Wharf in the Wren.
			requires = { G.docked_at(ids.WHARF), G.hull(ids.WREN) },
			done = G.fact({ kind = "undock", ship = ids.PLAYER, station = ids.WHARF }, { since = "start" }),
		}),
		G.beat({
			id = ids.B02,
			act = 1,
			giver = ids.ILSE,
			goal = "Ilse wants a full hold of ice: hold Space beside a pale rock until your hold is full.",
			requires = G.done(ids.B01),
			done = G.cargo(ids.ICE, G.tunable("b02.ice")),
		}),
		G.beat({
			id = ids.B03,
			act = 1,
			giver = ids.ILSE,
			goal = "Ilse asks you to take the ice to Odo at Kettle Dock. Dock in its ring and pick Deliver.",
			requires = G.done(ids.B02),
			done = G.sum({ kind = "deliver", other = ids.GREENHOUSE }, G.tunable("b03.ice"), { since = "start" }),
			-- The mining lens joins Orrin's list through its own unlock (b03 done). The
			-- fee (`b03.pay`) is paid by the Deliver that fills the greenhouse.
			rewards = { standing = { [ids.COOP] = 5 } },
			on_complete = function(ctx: G.Ctx, beat: G.BeatInfo)
				-- Kettle Dock stage 1, for good: a fogged glass dome and its grow lamps.
				if ctx.stations:stage(ids.KETTLE) < 1 then
					ctx.stations:set_stage(ids.KETTLE, 1, beat.why)
				end
				ctx.stations:attach(ids.KETTLE, "part.kettle.pane_dome_a", beat.why)
				ctx.stations:attach(ids.KETTLE, "part.light.warm_green", beat.why)
				ctx.history:record({
					kind = "core.stage_built",
					subjects = { station = ids.KETTLE },
					amount = 1,
					note = "Kettle Dock stage 1: a fogged glass dome",
				}, beat.why)
			end,
		}),
		-- b04: the first sale. The lens is on Orrin's list once b03 is done
		-- (upgrades.toml); selling what the player mines pays for it, and the sell
		-- screen shows the price dipping unit by unit.
		G.beat({
			id = ids.B04,
			act = 1,
			giver = ids.ORRIN,
			goal = "Orrin, the Wharf's hullwright, can fit a mining lens for 600 credits. Sell what you mine to pay for it.",
			requires = G.done(ids.B03),
			done = G.fitted("upgrade.mining_lens"),
		}),
		-- b05: the pause. Every opening of the PAUSED card writes a `paused` fact; one
		-- newer than b04's start counts, so a pause during b04 completes b05 at once and
		-- a pause before b04 doesn't stop a later one counting. Its goal is a literal
		-- hint: the key and what it does.
		G.beat({
			id = ids.B05,
			act = 1,
			giver = ids.ILSE,
			goal = "P or Esc · pause",
			requires = G.done(ids.B04),
			done = G.test(function(ctx: G.Ctx): boolean
				local b04 = ctx.beats:state(ids.B04)
				if b04 == nil or b04.since == nil then
					return false
				end
				return ctx.history:count({ kind = "paused", after = b04.since }) > 0
			end),
		}),
	},
	on = {
		-- Docking at Kettle while b03 is under way with ice aboard: Odo asks, and the
		-- player picks Deliver, Sell or Keep. Nothing leaves the hold until they do.
		dock = function(ev: G.DockEvent, ctx: G.Ctx)
			if ev.ship ~= ids.PLAYER or ev.station ~= ids.KETTLE or not ctx.beats:is_active(ids.B03) then
				return
			end
			if ctx.beats:pilot().docked_at ~= ids.KETTLE then
				return
			end
			local qty = handover(ctx)
			if qty < 1 then
				return
			end
			local why = ctx:cause({ summary = "Odo asks for the greenhouse ice", cites = { ev.fact } })
			ctx.talk:offer({
				speaker = ids.ODO,
				place = ids.KETTLE,
				text = `Is that ice for my greenhouse? I still need {still(ctx)}. It's yours to deliver, sell or keep.`,
				cites = { ev.fact },
				choices = {
					{ id = ids.DELIVER_ICE, text = `Deliver {qty} ice to Odo` },
					{ id = ids.SELL_ICE, text = "Sell it at the market" },
					{ id = ids.KEEP_ICE, text = "Keep it" },
				},
			}, why)
		end,
		-- The player's answer. Deliver hands over what the greenhouse still needs (or
		-- what they carry) and, once that fills it, pays Ilse's fee in the same
		-- transaction; a line at the dock says what went and what came back.
		reply = function(ev: G.ReplyEvent, ctx: G.Ctx)
			if ev.npc ~= ids.ODO or ev.station ~= ids.KETTLE or not ctx.beats:is_active(ids.B03) then
				return
			end
			if ev.choice == ids.DELIVER_ICE then
				-- A handover that lost to an undock runs again next tick: it hands over
				-- only if the player is still at Kettle.
				if ctx.beats:pilot().docked_at ~= ids.KETTLE then
					return
				end
				local qty = handover(ctx)
				if qty < 1 then
					return
				end
				local why = ctx:cause({ summary = "the player hands Odo ice for the greenhouse", cites = { ev.fact } })
				ctx.beats:deliver({ station = ids.KETTLE, ware = ids.ICE, quantity = qty, ledger = ids.GREENHOUSE }, why)
				local left = still(ctx) - qty
				local back = `{left} more to go`
				if left <= 0 then
					local fee = util.num(ctx, "b03.pay")
					ctx.beats:pay(fee, why)
					back = `+{fee} cr`
				end
				util.confirm(ctx, why, ids.KETTLE, `Delivered {qty} ice to Odo · {back}`, ev.fact)
			elseif ev.choice == ids.SELL_ICE or ev.choice == ids.KEEP_ICE then
				local why = ctx:cause({ summary = "the player keeps their ice for now", cites = { ev.fact } })
				local text = if ev.choice == ids.SELL_ICE
					then `Sell it, then: the market's open. I'll still need {still(ctx)} ice when you've some to spare.`
					else "Keep it, then. I'll ask again next time you dock."
				ctx.talk:offer({ speaker = ids.ODO, place = ids.KETTLE, text = text, cites = { ev.fact } }, why)
			end
		end,
		-- Odo names exactly what was brought (line.odo_fenn.01), citing the delivery.
		deliver = function(ev: G.DeliverEvent, ctx: G.Ctx)
			if ev.ledger ~= ids.GREENHOUSE then
				return
			end
			local f = ctx.history:fact(ev.fact)
			if f == nil then
				return
			end
			local why = ctx:cause({ summary = "Odo thanks the player for the ice", cites = { ev.fact } })
			lines.say(ctx, why, { line = lines.get("line.odo_fenn.01"), fact = f, npc = ids.ODO }, ids.KETTLE)
		end,
	},
})
