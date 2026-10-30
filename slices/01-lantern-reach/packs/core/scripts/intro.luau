--!strict
-- Names before they're used (the first playtest, PR #11): as a beat starts, the person
-- at the player's dock introduces whoever and whatever its goal line names next, in
-- one light line. Ilse greets the pilot and names the Wren, Odo and Rimewater (b01);
-- Odo names Orrin (b04); Ilse names Rook, Cinder Hook and the lane (b07); Rook names
-- glassite and Brassmouth (b08). Nobody speaks if the player isn't docked where the
-- speaker is: the goal line still carries the name.
local G = require("@galaxy")
local ids = require("../lib/ids")
local lines = require("../lib/lines")

-- Each beat's introduction: the line, who says it, and where.
local INTROS: { [string]: { line: string, npc: string, station: string } } = {
	[ids.B01] = { line = "line.ilse_marrow.18", npc = ids.ILSE, station = ids.WHARF },
	[ids.B04] = { line = "line.odo_fenn.10", npc = ids.ODO, station = ids.KETTLE },
	[ids.B07] = { line = "line.ilse_marrow.19", npc = ids.ILSE, station = ids.WHARF },
	[ids.B08] = { line = "line.rook_adeyemi.09", npc = ids.ROOK, station = ids.HOOK },
}

return G.script({
	id = "intro",
	on = {
		beat_started = function(ev: G.BeatStartedEvent, ctx: G.Ctx)
			local intro = INTROS[ev.beat]
			if intro == nil or ctx.beats:pilot().docked_at ~= intro.station then
				return
			end
			local f = ctx.history:fact(ev.fact)
			if f == nil then
				return
			end
			local why = ctx:cause({ summary = "someone at the dock names who's next", cites = { ev.fact } })
			lines.say(ctx, why, { line = lines.get(intro.line), fact = f, npc = intro.npc }, intro.station)
		end,
	},
})
