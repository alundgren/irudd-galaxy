--!strict
-- b09, the Cinder Hook dispute (content/04-journey.md, "The Cinder Hook dispute"): a
-- state-driven beat. The Cooperative and the Compact both want the Hook's ore depot;
-- Rook takes plate in whichever name the player chooses (and the player can change it
-- at any time). Plate and rations the player sells there in a side's name, and NPC
-- craft docking there, fill two tallies; when one reaches `win` with a lead of
-- `lead`, the depot changes hands at the next calm moment (the player docked or
-- paused). Every number is a `dispute.hook.*` tunable.
local G = require("@galaxy")
local ids = require("../lib/ids")
local util = require("../lib/util")
local lines = require("../lib/lines")

local SIDES: { [string]: string } = {
	[ids.SIDE_COOP] = ids.COOP,
	[ids.SIDE_COMPACT] = ids.COMPACT,
}

local CHOICES: { G.TalkChoice } = {
	{ id = ids.SIDE_COOP, text = "Supply in the Cooperative's name" },
	{ id = ids.SIDE_COMPACT, text = "Supply in the Compact's name" },
	{ id = ids.SIDE_NONE, text = "In nobody's name" },
}

-- The side the player supplies in ("" for nobody).
local function side(ctx: G.Ctx): string
	local v = ctx.store:get("dispute", "side")
	return if type(v) == "string" then v else ""
end

-- Each side's tally (units) since the dispute started.
local function tallies(ctx: G.Ctx): (number, number)
	local since = util.since(ctx, ids.B09)
	local function of(faction: string): number
		return ctx.history:sum({ kind = "core.dispute_tally", faction = faction, after = since }) / 1000
	end
	return of(ids.COOP), of(ids.COMPACT)
end

-- Who has won, given `extra` units about to go to a side's tally in this call.
local function winner(ctx: G.Ctx, extra_side: string?, extra: number): string?
	local coop, compact = tallies(ctx)
	if extra_side == ids.COOP then
		coop += extra
	elseif extra_side == ids.COMPACT then
		compact += extra
	end
	local win, lead = util.num(ctx, "dispute.hook.win"), util.num(ctx, "dispute.hook.lead")
	if coop >= win and coop - compact >= lead then
		return ids.COOP
	elseif compact >= win and compact - coop >= lead then
		return ids.COMPACT
	end
	return nil
end

local function set_base(station: string, ware: string, credits: number): G.Lever
	return util.set_base(station, ware, credits * 100)
end

-- The handover: the depot's owner, banner and markets, standing for whoever the player
-- backed, and Rook's 300 cr "from the new owners" if that side won.
local function hand_over(ctx: G.Ctx, why: G.Cause, to: string)
	local other = if to == ids.COOP then ids.COMPACT else ids.COOP
	ctx.stations:set_owner(ids.HOOK, to, why)
	ctx.stations:detach(ids.HOOK, if to == ids.COOP then "part.banner.compact" else "part.banner.coop", why)
	ctx.stations:attach(ids.HOOK, if to == ids.COOP then "part.banner.coop" else "part.banner.compact", why)
	if to == ids.COOP then
		-- Miners paid fairly; Cinder ore reaches the Wharf smelter.
		ctx.market:levers({
			set_base(ids.HOOK, ids.ORE, util.num(ctx, "dispute.hook.coop.hook_ore")),
			set_base(ids.WHARF, ids.ORE, util.num(ctx, "dispute.hook.coop.wharf_ore")),
			set_base(ids.WHARF, ids.PLATE, util.num(ctx, "dispute.hook.coop.wharf_plate")),
		}, why)
	else
		-- The Compact's ore trade; for the Cooperative's side, the Hookside claim.
		ctx.market:levers({
			set_base(ids.HOOK, ids.ORE, util.num(ctx, "dispute.hook.compact.hook_ore")),
			set_base(ids.BRASS, ids.ORE, util.num(ctx, "dispute.hook.compact.brass_ore")),
			set_base(ids.HOOK, ids.ICE, util.num(ctx, "dispute.hook.compact.hook_ice")),
		}, why)
		local hookside = ctx.beats:field(ids.HOOKSIDE)
		if hookside ~= nil then
			ctx.space:set_regrow(
				ids.HOOKSIDE,
				hookside.regrow * util.num(ctx, "dispute.hook.compact.hookside_regrow"),
				why
			)
		end
	end
	local backed = SIDES[side(ctx)]
	if backed ~= nil then
		local won = backed == to
		local against = if backed == ids.COOP then ids.COMPACT else ids.COOP
		ctx.standing:deed({
			faction = backed,
			deed = if won then "deed.dispute.backed_winner" else "deed.dispute.backed_loser",
			delta = util.num(ctx, if won then "dispute.hook.won_backed" else "dispute.hook.lost_backed"),
		}, why)
		ctx.standing:deed({
			faction = against,
			deed = if won then "deed.dispute.beaten" else "deed.dispute.held",
			delta = util.num(ctx, if won then "dispute.hook.won_other" else "dispute.hook.lost_other"),
		}, why)
		if won then
			ctx.beats:pay(util.num(ctx, "dispute.hook.reward"), why)
		end
	end
	util.record(
		ctx,
		why,
		"territory_changed",
		{ station = ids.HOOK, faction = to, other = ids.DISPUTE },
		nil,
		`The Cinder Hook depot changed hands: {other} to {to}`
	)
end

-- At a calm moment: hand over if a side has won (with `extra` units of this call's own
-- tally counted). Returns whether it did.
local function settle(ctx: G.Ctx, why: G.Cause, extra_side: string?, extra: number): boolean
	if not ctx.beats:is_active(ids.B09) then
		return false
	end
	if ctx.history:count({ kind = "core.territory_changed", other = ids.DISPUTE }) > 0 then
		return false
	end
	local to = winner(ctx, extra_side, extra)
	if to == nil then
		return false
	end
	hand_over(ctx, why, to)
	return true
end

return G.script({
	id = "dispute",
	beats = {
		G.beat({
			id = ids.B09,
			act = 2,
			kind = "situation",
			giver = ids.ROOK,
			goal = "The Cooperative and the Compact both want the Hook's ore depot. Supply it in whose name you like, or not at all.",
			requires = {
				G.done(ids.B07),
				G.counter("dockings", G.tunable("b09.hook_dockings"), { station = ids.HOOK }),
			},
			done = G.fact({ kind = "core.territory_changed", station = ids.HOOK }, { since = "start" }),
			on_start = function(ctx: G.Ctx, beat: G.BeatInfo)
				util.record(
					ctx,
					beat.why,
					"dispute_started",
					{ station = ids.HOOK, other = ids.DISPUTE },
					nil,
					"Two banners want the Cinder Hook ore depot"
				)
			end,
		}),
	},
	on = {
		-- Rook opens the dispute (line.rook_adeyemi.07), citing `dispute_started`, and
		-- offers the three names to supply in.
		beat_started = function(ev: G.BeatStartedEvent, ctx: G.Ctx)
			if ev.beat ~= ids.B09 then
				return
			end
			local f = ctx.history:last({ kind = "core.dispute_started", other = ids.DISPUTE })
			if f == nil then
				return
			end
			local why = ctx:cause({ summary = "Rook tells of the two banners", cites = { f.id } })
			lines.say(
				ctx,
				why,
				{ line = lines.get("line.rook_adeyemi.07"), fact = f, npc = ids.ROOK },
				ids.HOOK,
				CHOICES
			)
		end,
		reply = function(ev: G.ReplyEvent, ctx: G.Ctx)
			if ev.npc ~= ids.ROOK or not ctx.beats:is_active(ids.B09) then
				return
			end
			if ev.choice ~= ids.SIDE_COOP and ev.choice ~= ids.SIDE_COMPACT and ev.choice ~= ids.SIDE_NONE then
				return
			end
			local now = if ev.choice == ids.SIDE_NONE then "" else ev.choice
			if now == side(ctx) and ctx.history:count({ kind = "core.side_chosen", after = util.since(ctx, ids.B09) }) > 0 then
				return
			end
			local why = ctx:cause({ summary = "the player picks a name to supply the Hook in", cites = { ev.fact } })
			ctx.store:set("dispute", "side", now, why)
			util.record(
				ctx,
				why,
				"side_chosen",
				{ station = ids.HOOK, faction = SIDES[now], other = ids.DISPUTE },
				nil,
				if now == "" then "Supplying the Hook in nobody's name" else `Supplying the Hook in the name of {SIDES[now]}`
			)
		end,
		-- The player's plate and rations sold at the Hook count for the chosen side.
		sell = function(ev: G.SellEvent, ctx: G.Ctx)
			if ev.station ~= ids.HOOK or not ctx.beats:is_active(ids.B09) or not util.players(ctx, ev.ship) then
				return
			end
			local per = if ev.ware == ids.PLATE
				then util.num(ctx, "dispute.hook.plate")
				elseif ev.ware == ids.RATIONS then util.num(ctx, "dispute.hook.rations")
				else 0
			local faction = SIDES[side(ctx)]
			if per <= 0 or faction == nil then
				return
			end
			local units = ev.quantity * per
			local why = ctx:cause({ summary = "plate for the Hook depot, in a side's name", cites = { ev.fact } })
			util.record(
				ctx,
				why,
				"dispute_tally",
				{ station = ids.HOOK, faction = faction, ship = ev.ship, other = ids.DISPUTE },
				math.floor(units * 1000 + 0.5),
				`{units} toward the depot for {faction}`
			)
			-- Docked at the Hook: a calm moment.
			settle(ctx, why, faction, units)
		end,
		dock = function(ev: G.DockEvent, ctx: G.Ctx)
			if not ctx.beats:is_active(ids.B09) then
				return
			end
			-- NPC traffic at the Hook tips the depot too.
			if ev.station == ids.HOOK and (ev.ship == ids.NINEFOLD or ev.ship == ids.LITTLE_MERCY) then
				local faction = if ev.ship == ids.NINEFOLD then ids.COMPACT else ids.COOP
				local units =
					util.num(ctx, if ev.ship == ids.NINEFOLD then "dispute.hook.ninefold" else "dispute.hook.little_mercy")
				local why = ctx:cause({ summary = "a trader's docking at the Hook", cites = { ev.fact } })
				util.record(
					ctx,
					why,
					"dispute_tally",
					{ station = ids.HOOK, faction = faction, ship = ev.ship, other = ids.DISPUTE },
					math.floor(units * 1000 + 0.5),
					`{units} toward the depot for {faction}`
				)
				return
			end
			if ev.ship ~= ids.PLAYER then
				return
			end
			if winner(ctx, nil, 0) ~= nil then
				-- The player docked: a calm moment for the handover.
				settle(ctx, ctx:cause({ summary = "the Hook depot changes hands", cites = { ev.fact } }), nil, 0)
				return
			end
			-- While the dispute runs, Rook keeps the choice open at every Hook docking.
			local f = ctx.history:last({ kind = "core.dispute_started", other = ids.DISPUTE })
			if ev.station == ids.HOOK and f ~= nil then
				local why = ctx:cause({ summary = "Rook asks whose name the plate is in", cites = { f.id } })
				ctx.talk:offer({
					speaker = ids.ROOK,
					place = ids.HOOK,
					text = lines.get("line.rook_adeyemi.07").text(f, ctx),
					cites = { f.id },
					choices = CHOICES,
				}, why)
			end
		end,
		paused = function(ev: G.PausedEvent, ctx: G.Ctx)
			if not ctx.beats:is_active(ids.B09) or winner(ctx, nil, 0) == nil then
				return
			end
			settle(ctx, ctx:cause({ summary = "the Hook depot changes hands", cites = { ev.fact } }), nil, 0)
		end,
	},
})
