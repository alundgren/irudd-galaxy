--!strict
-- Act 3's directed beats (content/04-journey.md): b11, orders and loops, and b14,
-- where Orrin names the pilot's new habit. The shortage (b12) and the tender (b13) are
-- their own scripts.
local G = require("@galaxy")
local ids = require("../lib/ids")
local util = require("../lib/util")
local lines = require("../lib/lines")

local TRAIT_LINES: { [string]: string } = {
	["trait.rimesight"] = "line.orrin_vale.02",
	["trait.ringwise"] = "line.orrin_vale.03",
	["trait.market_ear"] = "line.orrin_vale.04",
	["trait.lanerunner"] = "line.orrin_vale.05",
	["trait.good_neighbour"] = "line.orrin_vale.06",
}

local ASK: { G.TalkChoice } = { { id = ids.ASK_ORRIN, text = "What do you mean?" } }

-- Orrin offers the question at the Wharf, citing b14's start.
local function offer_question(ctx: G.Ctx, cite: string)
	local why = ctx:cause({ summary = "Orrin says the pilot flies differently", cites = { cite } })
	ctx.talk:offer({
		speaker = ids.ORRIN,
		place = ids.WHARF,
		text = "You fly differently these days. Ask me what I mean.",
		cites = { cite },
		choices = ASK,
	}, why)
end

return G.script({
	id = "act3",
	beats = {
		G.beat({
			id = ids.B11,
			act = 3,
			giver = ids.ORRIN,
			goal = "Orrin can fit an order slate at the Wharf. Shift-click to line up orders.",
			requires = {
				G.done(ids.B08),
				G.flag("cap.map_travel"),
				G.counter("dockings", G.tunable("b11.dockings")),
			},
			steps = {
				{
					id = "slate",
					goal = "Orrin can fit an order slate at the Wharf. Shift-click to line up orders.",
					done = G.fitted("upgrade.order_slate"),
				},
				-- Orrin fits the loop log free once its own unlock holds (four queues).
				{
					id = "log",
					goal = "Finish four queues of three orders or more, and Orrin will fit the loop log free.",
					done = G.fitted("upgrade.loop_log"),
				},
				{
					id = "loop",
					goal = "Orrin says: save a loop between two stations you trust, and let it run twice.",
					done = G.fact({ kind = "loop_lap", ship = ids.PLAYER }, { at_least = G.tunable("b11.laps") }),
				},
			},
		}),
		G.beat({
			id = ids.B14,
			act = 3,
			giver = ids.ORRIN,
			goal = "Orrin says you fly differently these days. Ask him what he means.",
			requires = {
				G.done(ids.B13),
				G.any({
					G.trait("trait.rimesight"),
					G.trait("trait.ringwise"),
					G.trait("trait.market_ear"),
					G.trait("trait.lanerunner"),
					G.trait("trait.good_neighbour"),
				}),
			},
			done = G.fact({ kind = "core.habit_named", npc = ids.ORRIN }, { since = "start" }),
		}),
	},
	on = {
		beat_started = function(ev: G.BeatStartedEvent, ctx: G.Ctx)
			if ev.beat == ids.B14 then
				offer_question(ctx, ev.fact)
			end
		end,
		-- While b14 waits, Orrin asks again at each Wharf docking.
		dock = function(ev: G.DockEvent, ctx: G.Ctx)
			if ev.ship ~= ids.PLAYER or ev.station ~= ids.WHARF or not ctx.beats:is_active(ids.B14) then
				return
			end
			local started = ctx.history:last({ kind = "beat_started", other = ids.B14 })
			if started ~= nil then
				offer_question(ctx, started.id)
			end
		end,
		-- The beat conversation: Orrin names the newest habit and what it does. His line
		-- for that trait if it has one, else his fallback (line.orrin_vale.10), which the
		-- newest `trait_earned` (b14's precondition) always lets him say.
		reply = function(ev: G.ReplyEvent, ctx: G.Ctx)
			if ev.npc ~= ids.ORRIN or ev.choice ~= ids.ASK_ORRIN or not ctx.beats:is_active(ids.B14) then
				return
			end
			local newest = ctx.history:last({ kind = "trait_earned" })
			if newest == nil then
				return
			end
			local line = lines.get(TRAIT_LINES[newest.subjects.other or ""] or "line.orrin_vale.10")
			if not lines.fits(line, newest, ctx) then
				line = lines.get("line.orrin_vale.10")
			end
			local why = ctx:cause({ summary = "Orrin names the pilot's habit", cites = { newest.id } })
			lines.say(ctx, why, { line = line, fact = newest, npc = ids.ORRIN }, ids.WHARF)
			local t = ctx.traits:get(newest.subjects.other or "")
			util.record(
				ctx,
				why,
				"habit_named",
				{ npc = ids.ORRIN, station = ids.WHARF, other = newest.subjects.other },
				nil,
				"Orrin named the pilot's newest habit"
			)
			if t ~= nil then
				ctx.talk:offer({
					speaker = ids.ORRIN,
					place = ids.WHARF,
					text = `{t.name}: {t.effect}`,
					cites = { newest.id },
				}, why)
			end
		end,
	},
})
