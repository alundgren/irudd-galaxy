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

return G.script({
	id = "act1",
	beats = {
		G.beat({
			id = ids.B01,
			act = 1,
			giver = ids.ILSE,
			goal = "Ilse needs ice for the greenhouse at Kettle Dock. Fly off with W A S D and look for the pale rocks.",
			-- A new save: docked at the Wharf in the Wren.
			requires = { G.docked_at(ids.WHARF), G.hull(ids.WREN) },
			done = G.fact({ kind = "undock", ship = ids.PLAYER, station = ids.WHARF }, { since = "start" }),
		}),
		G.beat({
			id = ids.B02,
			act = 1,
			giver = ids.ILSE,
			goal = "Hold Space beside an ice rock until your hold is full.",
			requires = G.done(ids.B01),
			done = G.cargo(ids.ICE, G.tunable("b02.ice")),
		}),
		G.beat({
			id = ids.B03,
			act = 1,
			giver = ids.ILSE,
			goal = "Fly into Kettle Dock's ring, press Space, and give Odo the ice.",
			requires = G.done(ids.B02),
			done = G.sum({ kind = "deliver", other = ids.GREENHOUSE }, G.tunable("b03.ice"), { since = "start" }),
			-- The mining lens joins Orrin's list through its own unlock (b03 done).
			rewards = { credits = 120, standing = { [ids.COOP] = 5 } },
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
		-- a pause before b04 doesn't stop a later one counting.
		G.beat({
			id = ids.B05,
			act = 1,
			giver = ids.ILSE,
			goal = "Press P or Esc whenever you like. The whole galaxy waits for you.",
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
		-- The handover: docking at Kettle while b03 is under way gives Odo the ice the
		-- greenhouse still needs (all of it, or what the player carries).
		dock = function(ev: G.DockEvent, ctx: G.Ctx)
			if ev.ship ~= ids.PLAYER or ev.station ~= ids.KETTLE or not ctx.beats:is_active(ids.B03) then
				return
			end
			-- A handover that lost to an undock runs again next tick: it hands over only
			-- if the player is still at Kettle.
			local pilot = ctx.beats:pilot()
			if pilot.docked_at ~= ids.KETTLE then
				return
			end
			local still = util.num(ctx, "b03.ice") - delivered(ctx)
			local carried = pilot.cargo[ids.ICE] or 0
			local qty = math.min(still, carried)
			if qty < 1 then
				return
			end
			ctx.beats:deliver(
				{ station = ids.KETTLE, ware = ids.ICE, quantity = qty, ledger = ids.GREENHOUSE },
				ctx:cause({ summary = "the player hands Odo ice for the greenhouse", cites = { ev.fact } })
			)
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
