--!strict
-- Act 4's closing beats (content/04-journey.md): b16, the hullwright's question
-- (Hearthbound prestige), and b17, coming home. Prestige itself is the progress
-- system's service (pacing.toml `[prestige]`); the beat asks the question and notices
-- the answer.
local G = require("@galaxy")
local ids = require("../lib/ids")
local util = require("../lib/util")
local lines = require("../lib/lines")

local WHAT_CHANGED: { G.TalkChoice } = { { id = ids.WHAT_CHANGED, text = "What changed while I was out?" } }

-- Ilse's candidates for b17, in the spec's order of preference (content/05-people.md,
-- "Beat conversations"). Lines on one row are alternatives: at most one of them.
local HOMECOMING: { { string } } = {
	{ "line.ilse_marrow.03", "line.ilse_marrow.04" },
	{ "line.ilse_marrow.06", "line.ilse_marrow.14" },
	{ "line.ilse_marrow.15" },
	{ "line.ilse_marrow.16" },
	{ "line.ilse_marrow.12" },
	{ "line.ilse_marrow.13" },
	{ "line.ilse_marrow.08", "line.ilse_marrow.09" },
}

-- Counted dockings since the prestige at stations other than the Wharf, and whether
-- one was outside the Shallows.
local function away(ctx: G.Ctx): (number, boolean)
	local p = ctx.history:last({ kind = "prestige" })
	if p == nil then
		return 0, false
	end
	local n, outside = 0, false
	for _, d in ipairs(util.find_all(ctx, { kind = "dock", ship = ids.PLAYER }, p.id, 400)) do
		local station = d.subjects.station
		-- A re-dock that didn't count (amount 0) isn't another docking.
		if station ~= nil and station ~= ids.WHARF and d.amount ~= 0 then
			n += 1
			local s = ctx.stations:get(station)
			if s ~= nil and s.region ~= ids.SHALLOWS then
				outside = true
			end
		end
	end
	return n, outside
end

-- Ilse invites the question at the Wharf, citing b17's start.
local function invite(ctx: G.Ctx, cite: string)
	local why = ctx:cause({ summary = "Ilse wants to hear about the new hull", cites = { cite } })
	ctx.talk:offer({
		speaker = ids.ILSE,
		place = ids.WHARF,
		text = "Tell me how the new hull flies. And ask me what changed while you were out.",
		cites = { cite },
		choices = WHAT_CHANGED,
	}, why)
end

type Said = { line: lines.Line, fact: G.Fact }

-- Ilse's answer: up to three changes from the whole save, most specific first,
-- preferring facts she hasn't told the player before; then her closing line, which
-- cites the prestige.
local function homecoming(ctx: G.Ctx): { Said }
	local picks: { Said } = {}
	for _, row in ipairs(HOMECOMING) do
		local best: Said? = nil
		for _, id in ipairs(row) do
			local line = lines.get(id)
			local f = lines.newest(ctx, line, nil)
			if f ~= nil and (best == nil or util.seq(f.id) > util.seq(best.fact.id)) then
				best = { line = line, fact = f }
			end
		end
		if best ~= nil then
			table.insert(picks, best)
		end
	end
	-- Most specific first; facts she hasn't cited before ahead of ones she has.
	local fresh: { [number]: boolean } = {}
	for i, p in ipairs(picks) do
		fresh[i] = not util.said(ctx, p.line.id, p.fact.id)
	end
	local order: { number } = {}
	for i = 1, #picks do
		table.insert(order, i)
	end
	table.sort(order, function(a: number, b: number): boolean
		if fresh[a] ~= fresh[b] then
			return fresh[a]
		end
		if picks[a].line.specific ~= picks[b].line.specific then
			return picks[a].line.specific > picks[b].line.specific
		end
		return a < b
	end)
	local out: { Said } = {}
	for k = 1, math.min(3, #order) do
		table.insert(out, picks[order[k]])
	end
	return out
end

return G.script({
	id = "act4",
	beats = {
		G.beat({
			id = ids.B16,
			act = 4,
			giver = ids.ORRIN,
			goal = "Orrin asks whether the Wren is still the ship for you.",
			requires = {
				G.done(ids.B13),
				G.done(ids.B14),
				G.counter("dockings", G.tunable("b16.dockings")),
				G.docked_at(ids.WHARF),
			},
			done = G.fact({ kind = "prestige" }, { since = "start" }),
		}),
		G.beat({
			id = ids.B17,
			act = 4,
			giver = ids.ILSE,
			goal = "Ilse wants to hear how the new hull flies. Ask her what changed while you were out.",
			requires = {
				G.done(ids.B16),
				-- Dockings only change at a docking: check while docked.
				G.docked(),
				G.test(function(ctx: G.Ctx): boolean
					local n, outside = away(ctx)
					return n >= util.num(ctx, "b17.dockings_elsewhere") and outside
				end),
			},
			done = G.fact({ kind = "core.asked_what_changed", npc = ids.ILSE }, { since = "start" }),
		}),
	},
	on = {
		beat_started = function(ev: G.BeatStartedEvent, ctx: G.Ctx)
			if ev.beat == ids.B16 then
				-- Orrin asks (line.orrin_vale.11, citing b14's completion).
				local done14 = ctx.history:last({ kind = "beat_completed", other = ids.B14 })
				if done14 ~= nil then
					local why = ctx:cause({ summary = "Orrin asks the hullwright's question", cites = { done14.id } })
					lines.say(
						ctx,
						why,
						{ line = lines.get("line.orrin_vale.11"), fact = done14, npc = ids.ORRIN },
						ids.WHARF
					)
				end
			elseif ev.beat == ids.B17 then
				invite(ctx, ev.fact)
			end
		end,
		dock = function(ev: G.DockEvent, ctx: G.Ctx)
			if ev.ship ~= ids.PLAYER or ev.station ~= ids.WHARF or not ctx.beats:is_active(ids.B17) then
				return
			end
			local started = ctx.history:last({ kind = "beat_started", other = ids.B17 })
			if started ~= nil then
				invite(ctx, started.id)
			end
		end,
		-- b17: Ilse tells what changed, and always closes with line.ilse_marrow.17.
		reply = function(ev: G.ReplyEvent, ctx: G.Ctx)
			if ev.npc ~= ids.ILSE or ev.choice ~= ids.WHAT_CHANGED or not ctx.beats:is_active(ids.B17) then
				return
			end
			local prestige = ctx.history:last({ kind = "prestige" })
			if prestige == nil then
				return
			end
			local why = ctx:cause({ summary = "Ilse tells what changed while the player was out", cites = { ev.fact } })
			util.record(
				ctx,
				why,
				"asked_what_changed",
				{ npc = ids.ILSE, station = ids.WHARF },
				nil,
				"The player asked Ilse what changed"
			)
			for _, s in ipairs(homecoming(ctx)) do
				lines.say(ctx, why, { line = s.line, fact = s.fact, npc = ids.ILSE }, ids.WHARF)
			end
			lines.say(
				ctx,
				why,
				{ line = lines.get("line.ilse_marrow.17"), fact = prestige, npc = ids.ILSE },
				ids.WHARF
			)
		end,
	},
})
