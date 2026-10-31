--!strict
-- Kettle Dock's first harvest (issue #47, 4.1): the payoff of Odo's greenhouse. While
-- the `happening.kettle.first_harvest` happening runs (data/happenings.toml), Odo
-- offers the player a crate of fresh rations. The happening's own start line asks
-- (Accept / Not now); he asks again at each later docking while it runs, until the
-- player takes it. Only Accept loads it, as the pilot's own, with a receipt saying
-- what came aboard. One crate per save; its size is `harvest.rations` in pacing.toml.
local G = require("@galaxy")
local ids = require("../lib/ids")
local util = require("../lib/util")

local HARVEST = "happening.kettle.first_harvest"
local TAKE = "choice.harvest_take"
local LATER = "choice.harvest_later"

local function running(ctx: G.Ctx): boolean
	return ctx.happenings:active(ids.KETTLE) == HARVEST
end

local function taken(ctx: G.Ctx): boolean
	return ctx.history:count({ kind = "core.harvest_taken" }) > 0
end

-- Odo says `text` at Kettle, citing `cite`, with the crate's answers if `ask`.
local function odo(ctx: G.Ctx, summary: string, text: string, cite: string, ask: boolean)
	local why = ctx:cause({ summary = summary, cites = { cite } })
	ctx.talk:offer({
		speaker = ids.ODO,
		place = ids.KETTLE,
		text = text,
		cites = { cite },
		choices = if ask
			then {
				{ id = TAKE, text = "Accept: a crate of rations" },
				{ id = LATER, text = "Not now" },
			}
			else nil,
	}, why)
end

-- Accept: the crate goes aboard if there's room, and the dock says what came aboard.
local function take(ctx: G.Ctx, ev: G.ReplyEvent)
	local n = util.num(ctx, "harvest.rations")
	local free = ctx.beats:pilot().hold_free
	if free < n then
		odo(
			ctx,
			"the harvest crate needs room in the hold",
			`You've room for {free}, and the crate's {n} rations. Make some room and I'll ask again next time you dock.`,
			ev.fact,
			false
		)
		return
	end
	local why = ctx:cause({ summary = "Odo gives the player a crate from the first harvest", cites = { ev.fact } })
	ctx.beats:load({ ware = ids.RATIONS, quantity = n }, why)
	util.record(
		ctx,
		why,
		"harvest_taken",
		{ ship = ids.PLAYER, station = ids.KETTLE, ware = ids.RATIONS, npc = ids.ODO },
		n,
		`Odo gave the player {n} rations from the first harvest`
	)
	util.confirm(ctx, why, ids.KETTLE, `Odo gave you {n} rations from the first harvest · a gift`, ev.fact)
end

return G.script({
	id = "harvest",
	on = {
		dock = function(ev: G.DockEvent, ctx: G.Ctx)
			-- The docking that starts the harvest asks in its own line; this is a later
			-- one (the harvest isn't running yet in the world this docking reads).
			if ev.station ~= ids.KETTLE or not util.players(ctx, ev.ship) then
				return
			end
			if not running(ctx) or taken(ctx) then
				return
			end
			odo(
				ctx,
				"Odo asks again about the harvest crate",
				"The harvest crate's still put by for you, if you've room for it.",
				ev.fact,
				true
			)
		end,
		reply = function(ev: G.ReplyEvent, ctx: G.Ctx)
			if ev.npc ~= ids.ODO or ev.station ~= ids.KETTLE then
				return
			end
			if ev.choice ~= TAKE and ev.choice ~= LATER then
				return
			end
			if not running(ctx) or taken(ctx) then
				return
			end
			if ev.choice == TAKE then
				take(ctx, ev)
			else
				odo(ctx, "the player leaves the harvest crate for now", "It'll keep while the lamps are up.", ev.fact, false)
			end
		end,
	},
})
