--!strict
-- b10, the Ember Shoals rumor (content/04-journey.md, "The rumor"): a state-driven
-- beat. Pip tells it at Brassmouth. Its source, and so its truth, is fixed from history
-- when he does: the NPC craft whose docking at Brassmouth is the newest before the
-- telling (Ninefold or Quiet Lamp: true; Even Keel, or none yet: a stale tale). A
-- game-master pack may set `rumor.ember_shoals.override` first; core only lays out
-- the rocks for the truth it is given.
--
-- It resolves when the player flies within reach of the Shoals (followed) or once NPC
-- craft have carried it to every station (not followed).
local G = require("@galaxy")
local ids = require("../lib/ids")
local util = require("../lib/util")
local lines = require("../lib/lines")

-- The craft that can bring the story to Brassmouth, and how Pip names them.
local SOURCES: { [string]: string } = {
	[ids.NINEFOLD] = "A surveyor off the Ninefold",
	[ids.QUIET_LAMP] = "A Lamplighter chart-keeper",
	[ids.EVEN_KEEL] = "A deckhand off the Even Keel",
	[ids.LITTLE_MERCY] = "A skiff hand off the Little Mercy",
}
local SOURCE_ORDER = { ids.NINEFOLD, ids.QUIET_LAMP, ids.EVEN_KEEL, ids.LITTLE_MERCY }

local SHOALS_POSITIONS = {
	{ 0, 0 },
	{ -100, 90 },
	{ 90, -70 },
	{ -70, -120 },
	{ 110, 80 },
	{ -110, -50 },
}

local function site(ctx: G.Ctx): G.Spot
	return {
		x = util.num(ctx, "rumor.ember_shoals.x"),
		y = util.num(ctx, "rumor.ember_shoals.y"),
		region = ids.CINDER,
	}
end

-- Who told it and whether it's true, from the override or from history.
type Telling = { truth: boolean, planted_by: string?, source: string, craft: string?, dock: string? }
local function telling(ctx: G.Ctx): Telling
	local o = ctx.pacing:get("rumor.ember_shoals.override")
	if type(o) == "table" and #o >= 1 then
		local planted = o[2]
		return {
			truth = o[1] == "true",
			planted_by = if planted ~= nil and planted ~= "" then planted else nil,
			source = o[3] or "A surveyor",
		}
	end
	local best: G.Fact? = nil
	for _, craft in ipairs(SOURCE_ORDER) do
		local d = ctx.history:last({ kind = "dock", ship = craft, station = ids.BRASS })
		if d ~= nil and (best == nil or util.seq(d.id) > util.seq(best.id)) then
			best = d
		end
	end
	if best == nil then
		return { truth = false, source = "An old captain" }
	end
	local craft = best.subjects.ship or ""
	return {
		truth = util.has(util.list(ctx, "rumor.ember_shoals.true_sources"), craft),
		source = SOURCES[craft] or "A captain",
		craft = craft,
		dock = best.id,
	}
end

-- Lays out what's really at the Shoals: a rich glassite vein, or three small iron
-- rocks by an old claim marker.
local function lay_out(ctx: G.Ctx, why: G.Cause, truth: boolean, rocks: number?)
	if ctx.beats:field(ids.EMBER_SHOALS) ~= nil then
		return
	end
	local at = site(ctx)
	local n = rocks
		or util.num(ctx, if truth then "rumor.ember_shoals.rich_rocks" else "rumor.ember_shoals.stale_rocks")
	local spots: { G.Vec2 } = {}
	for i = 1, math.min(n, #SHOALS_POSITIONS) do
		local p = SHOALS_POSITIONS[i]
		table.insert(spots, { x = at.x + p[1], y = at.y + p[2] })
	end
	ctx.space:put_field({
		id = ids.EMBER_SHOALS,
		name = "Ember Shoals",
		region = ids.CINDER,
		rock_type = if truth then "rock_type.rich_glassite" else "rock_type.iron",
		center = { x = at.x, y = at.y },
		radius = 160,
		amount = util.num(ctx, if truth then "rumor.ember_shoals.rich_units" else "rumor.ember_shoals.stale_units"),
		regrow = 0,
		rocks = spots,
	}, why)
end

-- Records the rumor's resolution.
local function resolve(ctx: G.Ctx, why: G.Cause, followed: boolean)
	local r = ctx.rumors:get(ids.RUMOR)
	local truth = r ~= nil and r.is_true
	if followed then
		util.record(
			ctx,
			why,
			"rumor_followed",
			{ ship = ids.PLAYER, other = ids.RUMOR },
			if truth then 1 else 0,
			"The player flew out to the Ember Shoals"
		)
	end
	util.record(
		ctx,
		why,
		"rumor_resolved",
		{ other = ids.RUMOR, ship = if followed then ids.PLAYER else nil },
		if truth then 1 else 0,
		if truth then "The Ember Shoals vein was real" else "The Ember Shoals were worked out years ago"
	)
end

return G.script({
	id = "rumor",
	beats = {
		G.beat({
			id = ids.B10,
			act = 2,
			kind = "situation",
			giver = ids.PIP,
			goal = "Pip heard of a fat glassite vein at the Ember Shoals, out on Cinder Drift's far edge.",
			requires = {
				G.done(ids.B08),
				G.started(ids.B09),
				G.counter("dockings", G.tunable("b10.brass_dockings"), { station = ids.BRASS }),
				-- Pip tells it when the player docks at Brassmouth.
				G.docked_at(ids.BRASS),
			},
			done = G.any({
				G.fact({ kind = "core.rumor_resolved", other = ids.RUMOR }),
				G.test(function(ctx: G.Ctx): boolean
					local r = ctx.rumors:get(ids.RUMOR)
					return r ~= nil and #r.heard_at >= #ctx.stations:list()
				end),
				-- Followed: the player flew within reach of the Shoals.
				G.near({
					x = G.tunable("rumor.ember_shoals.x"),
					y = G.tunable("rumor.ember_shoals.y"),
					region = ids.CINDER,
				}, G.tunable("rumor.ember_shoals.follow_radius")),
			}),
			on_start = function(ctx: G.Ctx, beat: G.BeatInfo)
				local t = telling(ctx)
				ctx.rumors:put({
					id = ids.RUMOR,
					claim = "A fat glassite vein at the Ember Shoals, out on Cinder Drift's far edge.",
					is_true = t.truth,
					planted_by = t.planted_by,
					heard_at = { ids.BRASS },
					heard_by_player = true,
					source = t.dock,
				}, beat.why)
				util.record(
					ctx,
					beat.why,
					"rumor_heard",
					{ station = ids.BRASS, npc = ids.PIP, other = ids.RUMOR, ship = t.craft },
					if t.truth then 1 else 0,
					t.source
				)
			end,
			-- Followed: the player flew within reach. Lay out what's there.
			on_complete = function(ctx: G.Ctx, beat: G.BeatInfo)
				if ctx.history:count({ kind = "core.rumor_resolved", other = ids.RUMOR }) > 0 then
					return
				end
				local r = ctx.rumors:get(ids.RUMOR)
				local truth = r ~= nil and r.is_true
				local followed = r == nil or #r.heard_at < #ctx.stations:list()
				lay_out(ctx, beat.why, truth, if truth and not followed then math.floor(util.num(ctx, "rumor.ember_shoals.rich_rocks") / 2) else nil)
				resolve(ctx, beat.why, followed)
			end,
		}),
	},
	on = {
		-- Pip tells it (line.pip_calloway.01), citing `rumor_heard`; the map marks the
		-- rumored Shoals by the Observatory, the nearest lamp to them.
		beat_started = function(ev: G.BeatStartedEvent, ctx: G.Ctx)
			if ev.beat ~= ids.B10 then
				return
			end
			local f = ctx.history:last({ kind = "core.rumor_heard", other = ids.RUMOR })
			if f == nil then
				return
			end
			local why = ctx:cause({ summary = "Pip tells the Ember Shoals rumor", cites = { f.id } })
			lines.say(ctx, why, { line = lines.get("line.pip_calloway.01"), fact = f, npc = ids.PIP }, ids.BRASS)
			ctx.map:note({
				place = ids.GDO,
				text = "Ember Shoals (rumored): out on Cinder Drift's far edge, past the Observatory.",
				cites = { f.id },
			}, why)
		end,
		beat_completed = function(ev: G.BeatCompletedEvent, ctx: G.Ctx)
			if ev.beat ~= ids.B10 then
				return
			end
			local f = ctx.history:last({ kind = "core.rumor_resolved", other = ids.RUMOR })
			if f == nil then
				return
			end
			local why = ctx:cause({ summary = "the map shows what the Ember Shoals hold", cites = { f.id } })
			ctx.map:note({
				place = ids.GDO,
				text = if f.amount == 1
					then "Ember Shoals: a rich glassite vein."
					else "Ember Shoals: three small iron rocks and an old claim marker.",
				cites = { f.id },
			}, why)
		end,
		-- MindHost carries rumors only from a craft's previous dock, preserving truth.
		-- The beat above resolves once those committed spreads reach every station.
	},
})
