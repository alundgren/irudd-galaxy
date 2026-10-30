--!strict
-- Ambient dock talk and "What changed?" (content/05-people.md, "How people speak").
--
-- On each of the player's dockings, at most one line from the people at that station:
-- the most specific eligible line whose fact is newer than the player's previous
-- docking there, ties to the newest fact. Any dockmaster can be asked "What changed?":
-- they give up to three eligible lines citing facts since the player's last docking
-- there, most specific first. If no line qualifies, nobody speaks.
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
			local why = ctx:cause({ summary = `{pick.npc} has a word at the dock`, cites = { pick.fact.id } })
			lines.say(ctx, why, pick, ev.station)
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
			-- Up to three, one per fact. The question is recorded even when nobody has a
			-- line: the dock screen waits on it, and fills in plain digest lines.
			local said: { lines.Pick } = {}
			local cited: { [string]: boolean } = {}
			for _, pick in ipairs(picks) do
				if #said >= 3 then
					break
				end
				if not cited[pick.fact.id] then
					cited[pick.fact.id] = true
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
