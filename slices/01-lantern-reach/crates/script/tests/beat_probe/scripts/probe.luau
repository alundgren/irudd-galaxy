--!strict
local G = require("@galaxy")

local PLAYER = "ship.player"
local CRAFT = "craft.probe"
local KETTLE = "station.kettle_dock"
local WHARF = "station.lantern_wharf"
local COMPACT = "faction.tollway_compact"

return G.script({
	id = "probe",
	beats = {
		G.beat({
			id = "beat.t01_probe",
			act = 2,
			kind = "situation",
			goal = "Dock a few times, then carry some ice.",
			requires = G.counter("dockings", 1, { station = KETTLE }),
			steps = {
				{ id = "more", goal = "Dock at Kettle again.", done = G.counter("dockings", G.tunable("probe.dockings"), { station = KETTLE }) },
				{
					id = "ice",
					goal = "Carry 5 ice.",
					done = G.test(function(ctx: G.Ctx): boolean
						return (ctx.beats:pilot().cargo["ware.ice"] or 0) >= 5
					end),
				},
			},
			-- And one more docking anywhere once the last step is under way.
			done = G.fact({ kind = "dock", ship = PLAYER }, { since = "step" }),
			rewards = { flags = { "cap.click_fly" }, standing = { [COMPACT] = 2 } },
			on_step = function(ctx: G.Ctx, beat: G.BeatInfo)
				ctx.history:record({ kind = "probe.stepped", note = beat.step_id or "?" }, beat.why)
			end,
		}),
	},
	on = {
		dock = function(ev: G.DockEvent, ctx: G.Ctx)
			if ev.ship ~= PLAYER then
				return
			end
			local why = ctx:cause({ summary = "the probe reacts to a docking", cites = { ev.fact } })
			if ev.station == WHARF then
				-- Scripts never order the player's ship: this call aborts the handler.
				ctx.orders:give(PLAYER, { { verb = "undock" } }, why)
			elseif ev.station == KETTLE and ctx.orders:can_order(CRAFT) and #ctx.orders:get(CRAFT).orders == 0 then
				ctx.orders:give(CRAFT, { { verb = "dock", station = WHARF } }, why)
				ctx.orders:append(CRAFT, { verb = "undock" }, why)
				ctx.mechanics:teach({ mechanic = "mech.rock_tagging", teacher = "npc.juno_brask", slots = { "slot.key_t" } }, why)
				ctx.standing:deed({ faction = COMPACT, deed = "deed.probe.kindness", delta = 12 }, why)
				ctx.market:scale_consumers(KETTLE, "ware.ice", 2, why)
				ctx.pacing:set("probe.dockings", 3, why)
			end
		end,
		beat_completed = function(ev: G.BeatCompletedEvent, ctx: G.Ctx)
			ctx.history:record(
				{ kind = "probe.noticed", note = ev.beat },
				ctx:cause({ summary = "the probe saw a beat finish", cites = { ev.fact } })
			)
		end,
	},
})
