--!strict
-- The Ancients fragment and b15, Ancients chapter 1 (content/01-galaxy.md "The
-- Ancients fragment", content/04-journey.md, content/06-game-master.md "canon graph").
--
-- Found by activity: once b13 is done and the player has mined enough glassite, the
-- next glassite rock their own ship spends drops the one fragment into the hold (a
-- tender never finds it). With the hold full it waits, glinting, until the player
-- mines glassite again with room. The first dockmaster the player then meets notices it
-- (`shown`): the Lamplighters remember being told first. Nell sees it (the shard node
-- opens), the player carries it through a lane and it sings (the song node opens),
-- and the player tells Nell and leaves it with the Observatory or keeps it.
local G = require("@galaxy")
local ids = require("../lib/ids")
local util = require("../lib/util")
local lines = require("../lib/lines")
local canon = require("../lib/canon")

local TELL: { G.TalkChoice } = {
	{ id = ids.FRAGMENT_LEAVE, text = "Leave it with the Observatory" },
	{ id = ids.FRAGMENT_KEEP, text = "Keep it" },
}

local function found(ctx: G.Ctx): G.Fact?
	return ctx.history:last({ kind = "core.fragment_found" })
end

local function carrying(ctx: G.Ctx): boolean
	return (ctx.beats:pilot().cargo[ids.FRAGMENT] or 0) > 0
end

local function glassite_rock(rock: string): boolean
	return string.sub(rock, 1, #ids.GLASS_DRIFT_ROCKS) == ids.GLASS_DRIFT_ROCKS
		or string.sub(rock, 1, #ids.EMBER_SHOALS_ROCKS) == ids.EMBER_SHOALS_ROCKS
end

-- Puts the fragment in the hold if there's room, and says where it came from.
-- `rock` is the spent rock it glinted in.
local function pick_up(ctx: G.Ctx, why: G.Cause, rock: string)
	ctx.beats:load({ ware = ids.FRAGMENT, quantity = 1, found = true }, why)
	util.record(
		ctx,
		why,
		"fragment_found",
		{ ship = ids.PLAYER, rock = rock, ware = ids.FRAGMENT },
		nil,
		"A warm shard of glass, unscratched, in the dust of a spent glassite rock"
	)
	ctx.store:set("fragment", "glint", "", why)
end

-- A fragment still waiting as a glint (the hold was full when its rock was spent).
local function glint(ctx: G.Ctx): string?
	local v = ctx.store:get("fragment", "glint")
	return if type(v) == "string" and v ~= "" then v else nil
end

-- A waiting glint is picked up the next time the player mines glassite with room in
-- the hold: their own mining is what finds it (`cite`, the `mine` fact).
local function try_glint(ctx: G.Ctx, cite: string)
	local rock = glint(ctx)
	if rock ~= nil and ctx.beats:pilot().hold_free >= 1 then
		pick_up(ctx, ctx:cause({ summary = "the player picks up the glinting fragment", cites = { cite } }), rock)
	end
end

-- The survey fee for telling Nell (`b15.pay`), paid with the player's answer, and a
-- line saying what went and what came back.
local function pay_fee(ctx: G.Ctx, why: G.Cause, what: string, cite: string)
	local fee = util.num(ctx, "b15.pay")
	ctx.beats:pay(fee, why)
	util.confirm(ctx, why, ids.GDO, `{what} · +{fee} cr survey fee`, cite)
end

return G.script({
	id = "fragment",
	beats = {
		G.beat({
			id = ids.B15,
			act = 4,
			giver = ids.NELL,
			goal = "Nell Quillon at Glass Drift would love to see what you found in that rock.",
			requires = G.fact({ kind = "core.fragment_found" }),
			steps = {
				{
					id = "show",
					goal = "Nell Quillon at Glass Drift would love to see what you found in that rock.",
					done = G.fact({ kind = "core.shown", npc = ids.NELL }),
				},
				{
					id = "carry",
					goal = "Nell asks you to carry the fragment through a lane, and listen.",
					done = G.fact({ kind = "core.canon_opened", other = "canon.fragments_sing" }),
				},
				{
					id = "tell",
					goal = "Tell Nell what you heard.",
					done = G.any({
						G.fact({ kind = "core.fragment_left" }, { since = "step" }),
						G.fact({ kind = "reply", npc = ids.NELL, other = ids.FRAGMENT_KEEP }, { since = "step" }),
					}),
				},
			},
			-- The Society's thanks; +5 more if left, said when left. The survey fee
			-- (`b15.pay`) is paid with the player's answer.
			rewards = { standing = { [ids.LAMP] = 15 } },
		}),
	},
	on = {
		-- The fragment glints in the next glassite rock the player's own ship spends.
		rock_depleted = function(ev: G.RockDepletedEvent, ctx: G.Ctx)
			if ev.ship ~= ids.PLAYER or not glassite_rock(ev.rock) or not ctx.beats:is_done(ids.B13) then
				return
			end
			if found(ctx) ~= nil or glint(ctx) ~= nil then
				return
			end
			local mined = ctx.history:sum({ kind = "mine", ship = ids.PLAYER, ware = ids.GLASSITE })
			if mined < util.num(ctx, "fragment.glassite_mined") then
				return
			end
			local why = ctx:cause({ summary = "something glints in a spent glassite rock", cites = { ev.fact } })
			if ctx.beats:pilot().hold_free >= 1 then
				pick_up(ctx, why, ev.rock)
			else
				-- The hold is full: it waits, glinting, until there's room.
				ctx.store:set("fragment", "glint", ev.rock, why)
			end
		end,
		mine = function(ev: G.MineEvent, ctx: G.Ctx)
			if ev.ship == ids.PLAYER and glassite_rock(ev.rock) then
				try_glint(ctx, ev.fact)
			end
		end,
		dock = function(ev: G.DockEvent, ctx: G.Ctx)
			if ev.ship ~= ids.PLAYER then
				return
			end
			local f = found(ctx)
			if f == nil then
				return
			end
			local step = util.step(ctx, ids.B15)
			local first = ctx.history:count({ kind = "core.shown" }) == 0
			if ev.station == ids.GDO and step == 0 and carrying(ctx) then
				-- b15 step 1: Nell sees it (line.nell_quillon.02, citing
				-- `fragment_found`), and the shard node opens.
				local why = ctx:cause({ summary = "Nell sees the fragment", cites = { f.id } })
				lines.say(ctx, why, { line = lines.get("line.nell_quillon.02"), fact = f, npc = ids.NELL }, ids.GDO)
				util.record(
					ctx,
					why,
					"shown",
					{ npc = ids.NELL, station = ids.GDO, ware = ids.FRAGMENT },
					nil,
					"Nell Quillon was shown the fragment"
				)
				canon.open(ctx, why, "canon.fragment_is_lens_shard", "core:b15.1")
				if first then
					ctx.standing:deed({ faction = ids.LAMP, deed = "deed.lamplighters.told_first", delta = 5 }, why)
				end
				return
			end
			if ev.station == ids.GDO and step == 2 then
				-- b15 step 3: Nell asks what the player heard (line.nell_quillon.04,
				-- citing the song).
				local sang = ctx.history:last({ kind = "core.canon_opened", other = "canon.fragments_sing" })
				if sang ~= nil then
					local why = ctx:cause({ summary = "Nell asks what the player heard", cites = { sang.id } })
					lines.say(
						ctx,
						why,
						{ line = lines.get("line.nell_quillon.04"), fact = sang, npc = ids.NELL },
						ids.GDO,
						TELL
					)
				end
				return
			end
			-- Whom the player told first: the first dockmaster they meet with it aboard
			-- notices the glint. Not a Lamplighter? The Society hears it secondhand.
			if first and carrying(ctx) then
				for _, npc in ipairs(ctx.npcs:at(ev.station)) do
					if npc.role == "dockmaster" then
						local why = ctx:cause({ summary = "a dockmaster notices the fragment", cites = { f.id, ev.fact } })
						util.record(
							ctx,
							why,
							"shown",
							{ npc = npc.id, station = ev.station, ware = ids.FRAGMENT },
							nil,
							`{npc.name} noticed the fragment in the hold`
						)
						if npc.faction == ids.LAMP then
							ctx.standing:deed({ faction = ids.LAMP, deed = "deed.lamplighters.told_first", delta = 5 }, why)
						end
						return
					end
				end
			end
		end,
		-- Nell asks the player to carry it through a lane and listen (line
		-- .nell_quillon.07, citing the `shown` fact step 1 wrote).
		quest_step = function(ev: G.QuestStepEvent, ctx: G.Ctx)
			if ev.quest ~= ids.B15 or ev.step ~= "carry" then
				return
			end
			local shown = ctx.history:last({ kind = "core.shown", npc = ids.NELL })
			if shown == nil then
				return
			end
			local why = ctx:cause({ summary = "Nell asks the player to listen in a lane", cites = { shown.id } })
			lines.say(ctx, why, { line = lines.get("line.nell_quillon.07"), fact = shown, npc = ids.NELL }, ids.GDO)
		end,
		-- b15 step 2: carried through a lane, the fragment sings with the lane lamps.
		jump = function(ev: G.JumpEvent, ctx: G.Ctx)
			if ev.ship ~= ids.PLAYER or util.step(ctx, ids.B15) ~= 1 or not carrying(ctx) then
				return
			end
			local why = ctx:cause({ summary = "the fragment sings in the lane", cites = { ev.fact } })
			canon.open(ctx, why, "canon.fragments_sing", "core:b15.2")
		end,
		-- b15 step 3: leave it with the Observatory (it glows in the lens tower for
		-- good) or keep it.
		reply = function(ev: G.ReplyEvent, ctx: G.Ctx)
			if ev.npc ~= ids.NELL or util.step(ctx, ids.B15) ~= 2 or ctx.beats:pilot().docked_at ~= ids.GDO then
				return
			end
			if ev.choice == ids.FRAGMENT_KEEP then
				local why = ctx:cause({ summary = "the player keeps the fragment", cites = { ev.fact } })
				pay_fee(ctx, why, "Kept the fragment", ev.fact)
				return
			end
			if ev.choice ~= ids.FRAGMENT_LEAVE or not carrying(ctx) then
				return
			end
			local why = ctx:cause({ summary = "the fragment stays with the Observatory", cites = { ev.fact } })
			pay_fee(ctx, why, "Left the fragment with Nell", ev.fact)
			ctx.beats:deliver({ station = ids.GDO, ware = ids.FRAGMENT, quantity = 1, ledger = ids.LENS_TOWER }, why)
			ctx.stations:attach(ids.GDO, "part.gdo.fragment_case", why)
			ctx.standing:deed({ faction = ids.LAMP, deed = "deed.lamplighters.fragment_left", delta = 5 }, why)
			util.record(
				ctx,
				why,
				"fragment_left",
				{ npc = ids.NELL, station = ids.GDO, ware = ids.FRAGMENT },
				nil,
				"The fragment glows in the Observatory's lens tower"
			)
		end,
	},
})
