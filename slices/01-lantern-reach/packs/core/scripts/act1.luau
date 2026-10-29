--!strict
-- Act 1, beats b01 to b03 (content/04-journey.md): the worked examples of the beat API.
-- The rest of the journey is authored the same way, one script per act or situation.
--
-- A beat is data: preconditions, steps and an end state built from G.* requirements,
-- plus rewards. The script host checks it every tick; nothing here counts time, and
-- every threshold is a tunable from data/pacing.toml. Callbacks (`on_complete`) make
-- world changes atomically with the transition; lines that cite the beat's own facts
-- go in ordinary event handlers, where the fact exists.
local G = require("@galaxy")

local WHARF = "station.lantern_wharf"
local KETTLE = "station.kettle_dock"
local COOP = "faction.wharf_cooperative"
local ILSE = "npc.ilse_marrow"
local ODO = "npc.odo_fenn"
local PLAYER = "ship.player"
local ICE = "ware.ice"
local GREENHOUSE = "ledger.kettle_greenhouse"

local B01 = "beat.b01_undock"
local B02 = "beat.b02_mine_by_hand"
local B03 = "beat.b03_first_delivery"

-- A number from pacing.toml. Missing data is an error, never a silent default.
local function tunable(ctx: G.Ctx, id: string): number
	local v = ctx.pacing:get(id)
	if type(v) ~= "number" then
		error(`pacing.toml has no number {id}`)
	end
	return v
end

-- Ice handed to the greenhouse since b03 started.
local function delivered(ctx: G.Ctx): number
	local b = ctx.beats:state(B03)
	if b == nil then
		return 0
	end
	return ctx.history:sum({ kind = "deliver", other = GREENHOUSE, after = b.since })
end

return G.script({
	id = "act1",
	beats = {
		G.beat({
			id = B01,
			act = 1,
			giver = ILSE,
			goal = "Ilse needs ice for the Kettle Dock greenhouse. Undock and look for the pale rocks.",
			-- A new save: docked at the Wharf in the Wren.
			requires = { G.docked_at(WHARF), G.hull("hull.wren") },
			done = G.fact({ kind = "undock", ship = PLAYER, station = WHARF }, { since = "start" }),
		}),
		G.beat({
			id = B02,
			act = 1,
			giver = ILSE,
			goal = "Hold Space beside an ice rock until your hold is full.",
			requires = G.done(B01),
			done = G.cargo(ICE, G.tunable("b02.ice")),
		}),
		G.beat({
			id = B03,
			act = 1,
			giver = ILSE,
			goal = "Fly into Kettle Dock's ring, press Space, and give Odo the ice.",
			requires = G.done(B02),
			done = G.sum({ kind = "deliver", other = GREENHOUSE }, G.tunable("b03.ice"), { since = "start" }),
			-- The mining lens joins Orrin's list through its own unlock (b03 done).
			rewards = { credits = 120, standing = { [COOP] = 5 } },
			on_complete = function(ctx: G.Ctx, beat: G.BeatInfo)
				-- Kettle Dock stage 1, for good: a fogged glass dome.
				if ctx.stations:stage(KETTLE) < 1 then
					ctx.stations:set_stage(KETTLE, 1, beat.why)
				end
				ctx.stations:attach(KETTLE, "part.kettle.pane_dome_a", beat.why)
				ctx.history:record({
					kind = "core.stage_built",
					subjects = { station = KETTLE },
					amount = 1,
					note = "Kettle Dock stage 1: a fogged glass dome",
				}, beat.why)
			end,
		}),
	},
	on = {
		-- The handover: docking at Kettle while b03 is under way gives Odo the ice the
		-- greenhouse still needs (all of it, or what the player carries).
		dock = function(ev: G.DockEvent, ctx: G.Ctx)
			if ev.ship ~= PLAYER or ev.station ~= KETTLE or not ctx.beats:is_active(B03) then
				return
			end
			local still = tunable(ctx, "b03.ice") - delivered(ctx)
			local carried = ctx.beats:pilot().cargo[ICE] or 0
			local qty = math.min(still, carried)
			if qty < 1 then
				return
			end
			ctx.beats:deliver(
				{ station = KETTLE, ware = ICE, quantity = qty, ledger = GREENHOUSE },
				ctx:cause({ summary = "the player hands Odo ice for the greenhouse", cites = { ev.fact } })
			)
		end,
		-- Odo names exactly what was brought (line.odo_fenn.01), citing the delivery.
		deliver = function(ev: G.DeliverEvent, ctx: G.Ctx)
			if ev.ledger ~= GREENHOUSE then
				return
			end
			ctx.talk:offer({
				speaker = ODO,
				place = KETTLE,
				text = `{ev.quantity} ice. The beans will hear about this.`,
				cites = { ev.fact },
			}, ctx:cause({ summary = "Odo thanks the player for the ice", cites = { ev.fact } }))
		end,
	},
})
