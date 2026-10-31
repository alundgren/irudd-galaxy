--!strict
-- Ambient dock talk and "What changed?" (content/05-people.md, "How people speak").
--
-- On each of the player's dockings, at most one line from the people at that station:
-- the mind picks among eligible authored lines, defaulting to the most specific,
-- then the newest fact. Any dockmaster can be asked "What changed?":
-- they give up to three eligible lines citing facts since the player's last docking
-- there, at most one per place, most specific first. If no line qualifies, nobody
-- speaks.
local G = require("@galaxy")
local ids = require("../lib/ids")
local util = require("../lib/util")
local lines = require("../lib/lines")

-- The player's docking at `station` before the one at `tick`.
local function previous(ctx: G.Ctx, station: string, tick: number): G.Fact?
	return util.before(ctx, { kind = "dock", ship = ids.PLAYER, station = station }, tick)
end

return G.script({
	id = "talk",
	on = {
		dock = function(ev: G.DockEvent, ctx: G.Ctx)
			if ev.ship ~= ids.PLAYER then
				return
			end
			local prev = previous(ctx, ev.station, ev.tick)
			local picks = lines.candidates(ctx, ev.station, if prev then prev.id else nil, false)
			local pick = picks[1]
			if pick == nil then
				return
			end
			local candidates: { G.DockTalkCandidate } = {}
			local cites = { ev.fact }
			for _, p in ipairs(picks) do
				if #candidates >= 32 then break end
				table.insert(cites, p.fact.id)
				table.insert(candidates, {
					line = p.line.id, npc = p.npc, text = p.line.text(p.fact, ctx),
					cite = p.fact.id, specificity = p.line.specific,
				})
			end
			local why = ctx:cause({ summary = "Authored lines offered at the dock", cites = cites })
			ctx.talk:request(ev.fact, candidates, why)
		end,
		talk_picked = function(ev: G.TalkPickedEvent, ctx: G.Ctx)
			local pick = ctx.talk:selected(ev.request, ev.line)
			local place = ctx.beats:pilot().docked_at
			if pick == nil or place == nil or util.said(ctx, pick.line, pick.cite) then return end
			local why = ctx:cause({ summary = `{pick.npc} has a word at the dock`, cites = { ev.fact, pick.cite } })
			ctx.beats:expect_docked(place, why)
			util.say(ctx, why, pick.line, pick.npc, place, pick.text, pick.cite)
		end,
		reply = function(ev: G.ReplyEvent, ctx: G.Ctx)
			if ev.choice ~= ids.WHAT_CHANGED then
				return
			end
			-- Ilse's homecoming answer is b17's own conversation.
			if ev.npc == ids.ILSE and ctx.beats:is_active(ids.B17) then
				return
			end
			local npc = ctx.npcs:get(ev.npc)
			if npc == nil or npc.role ~= "dockmaster" then
				return
			end
			-- Facts since the docking before this one.
			local here = ctx.history:last({ kind = "dock", ship = ids.PLAYER, station = ev.station })
			local prev = if here then previous(ctx, ev.station, here.tick) else nil
			local picks = lines.candidates(ctx, ev.station, if prev then prev.id else nil, false)
			-- Up to three, one per cited place and fact. Record the question even when
			-- nobody has a line: the dock screen waits on it and fills in digest lines.
			local station = ctx.stations:get(ev.station)
			local region = if station then station.region else ""
			local said: { lines.Pick } = {}
			local cited: { [string]: boolean } = {}
			local places: { [string]: boolean } = {}
			for _, pick in ipairs(picks) do
				if #said >= 3 then
					break
				end
				-- Facts without a station share one place, as in the plain digest. Only
				-- what changed here or nearby: this station, or one in its region.
				local place = pick.fact.subjects.station or ""
				local at = if place ~= "" then ctx.stations:get(place) else nil
				local near = place == "" or place == ev.station or (at ~= nil and at.region == region)
				if near and not cited[pick.fact.id] and not places[place] then
					cited[pick.fact.id] = true
					places[place] = true
					table.insert(said, pick)
				end
			end
			local why = ctx:cause({ summary = `{npc.name} says what changed`, cites = { ev.fact } })
			util.record(
				ctx,
				why,
				"asked_what_changed",
				{ npc = ev.npc, station = ev.station },
				#said,
				`The player asked {npc.name} what changed`
			)
			for _, pick in ipairs(said) do
				lines.say(ctx, why, pick, ev.station)
			end
		end,
	},
})
