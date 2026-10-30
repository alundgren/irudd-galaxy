--!strict
-- b12, the Rimewater ice shortage (content/04-journey.md, "The ice shortage"): a
-- state-driven beat that reacts to the player's own mining. Once Rimewater is thin and
-- the player has mined enough Shallows ice, at a calm moment the shock lands: ice
-- costs more and is drunk faster at the shortage's stations, and Kettle's lamps dim.
-- The paired opportunity is that ice pays well.
--
-- Two ways to help (way A, ice from outside the Shallows; way B, let Rimewater rest
-- and bring plate for a condenser), or, ignored, the Compact brings ice in after
-- enough Kettle activity. Every number is a `shortage.rimewater_ice.*` tunable, read
-- once when the shortage starts (a game-master pack may vary them while it's dormant).
local G = require("@galaxy")
local ids = require("../lib/ids")
local util = require("../lib/util")
local lines = require("../lib/lines")

local T = "shortage.rimewater_ice."

local REST: { G.TalkChoice } = { { id = ids.REST, text = "Let Rimewater rest" } }

local function get(ctx: G.Ctx, key: string): number
	local v = ctx.store:get("shortage", key)
	return if type(v) == "number" then v else 0
end

local function set(ctx: G.Ctx, why: G.Cause, key: string, value: number)
	ctx.store:set("shortage", key, value, why)
end

-- The stations the shock landed on (frozen when it started).
local function stations(ctx: G.Ctx): { string }
	local v = ctx.store:get("shortage", "stations")
	local out: { string } = {}
	if type(v) == "string" then
		for s in string.gmatch(v, "[^,]+") do
			table.insert(out, s)
		end
	end
	return out
end

-- Ice the player's own ship has mined in the Shallows (Rimewater is its ice field).
local function shallows_ice(ctx: G.Ctx): number
	local n = 0
	for _, f in ipairs(util.find_all(ctx, { kind = "mine", ship = ids.PLAYER, ware = ids.ICE }, nil, 2000)) do
		local rock = f.subjects.rock or ""
		if string.sub(rock, 1, #ids.RIMEWATER_ROCKS) == ids.RIMEWATER_ROCKS then
			n += util.amount(f)
		end
	end
	return n
end

local function since(ctx: G.Ctx): string?
	return util.since(ctx, ids.B12)
end

local function sum_since(ctx: G.Ctx, kind: string): number
	return ctx.history:sum({ kind = `core.{kind}`, other = ids.SHORTAGE, after = since(ctx) })
end

-- Ends the shortage: bases and drink back to normal, the lamps relight, and the way's
-- lasting result. `how` is 1 (way A), 2 (way B) or 0 (ignored: the Compact).
local function resolve(ctx: G.Ctx, why: G.Cause, how: number)
	local drink = util.num(ctx, T .. "drink_factor")
	local levers: { G.Lever } = {}
	for i, station in ipairs(stations(ctx)) do
		local base = get(ctx, `base{i}`)
		if base > 0 then
			table.insert(levers, util.set_base(station, ids.ICE, base))
			table.insert(levers, util.scale("scale_consumers", station, ids.ICE, 1 / drink))
		end
	end
	if how == 1 then
		ctx.mechanics:set("mech.ice_road", true, why)
	end
	if how == 0 then
		-- The Compact's barge brings ice to Kettle.
		local line = ctx.market:line(ids.KETTLE, ids.ICE)
		if line ~= nil then
			table.insert(levers, util.set_stock(ids.KETTLE, ids.ICE, line.stock + util.num(ctx, T .. "compact_ice")))
		end
	elseif how == 2 then
		-- Kettle's condenser: its ice drink falls for good.
		table.insert(levers, util.add_consumers(ids.KETTLE, ids.ICE, -util.num(ctx, T .. "condenser_drink")))
		ctx.stations:attach(ids.KETTLE, "part.kettle.condenser", why)
	end
	ctx.market:levers(levers, why)
	ctx.stations:detach(ids.KETTLE, "part.light.dim_green", why)
	ctx.stations:attach(ids.KETTLE, "part.light.warm_green", why)
	if how > 0 then
		ctx.standing:deed({
			faction = ids.COOP,
			deed = if how == 1 then "deed.shortage.ice_road" else "deed.shortage.rest",
			delta = util.num(ctx, T .. (if how == 1 then "way_a_standing" else "way_b_standing")),
			tags = { "delivery" },
		}, why)
		util.record(
			ctx,
			why,
			"shortage_helped",
			{ ship = ids.PLAYER, station = ids.KETTLE, other = ids.SHORTAGE },
			how,
			if how == 1 then "Ice from outside the Shallows" else "Rimewater rests; Kettle has a condenser"
		)
	end
	util.record(
		ctx,
		why,
		"shortage_resolved",
		{
			station = ids.KETTLE,
			other = ids.SHORTAGE,
			ship = if how > 0 then ids.PLAYER else nil,
			faction = if how == 0 then ids.COMPACT else nil,
		},
		how,
		if how == 0 then "The Compact brought ice to Kettle Dock" else "The Shallows came through the shortage"
	)
	set(ctx, why, "done", 1)
end

-- Whether the shortage is on and unresolved.
local function on(ctx: G.Ctx): boolean
	return ctx.beats:is_active(ids.B12) and get(ctx, "done") == 0
end

-- Restores Rimewater's regrowth after a rest (it lapsed, or the field is full enough).
local function end_rest(ctx: G.Ctx, why: G.Cause)
	ctx.space:set_regrow(ids.RIMEWATER, get(ctx, "rest_regrow"), why)
	set(ctx, why, "rest", 0)
end

return G.script({
	id = "shortage",
	beats = {
		G.beat({
			id = ids.B12,
			act = 3,
			kind = "situation",
			giver = ids.ODO,
			goal = "Rimewater is thin, and Kettle Dock is short of ice. Odo has two ideas.",
			requires = {
				G.done(ids.B11),
				-- A calm moment: the player docked. (Nothing else in core changes the
				-- Shallows in a big way, so no other change can overlap it.)
				G.docked(),
				-- All the ice the player has mined: Shallows ice can't be more.
				G.sum({ kind = "mine", ship = ids.PLAYER, ware = ids.ICE }, G.tunable(T .. "mined")),
				G.test(function(ctx: G.Ctx): boolean
					local rime = ctx.beats:field(ids.RIMEWATER)
					return rime ~= nil
						and rime.left <= util.num(ctx, T .. "trigger_left")
						and shallows_ice(ctx) >= util.num(ctx, T .. "mined")
				end),
			},
			done = G.fact({ kind = "core.shortage_resolved", other = ids.SHORTAGE }, { since = "start" }),
			on_start = function(ctx: G.Ctx, beat: G.BeatInfo)
				local why = beat.why
				local list = util.list(ctx, T .. "stations")
				local price, drink = util.num(ctx, T .. "price_factor"), util.num(ctx, T .. "drink_factor")
				local levers: { G.Lever } = {}
				local hit: { string } = {}
				for i, station in ipairs(list) do
					local line = ctx.market:line(station, ids.ICE)
					if i <= 4 and line ~= nil and line.role ~= "sells" then
						table.insert(hit, station)
						set(ctx, why, `base{#hit}`, line.base_price)
						table.insert(levers, util.scale("scale_base", station, ids.ICE, price))
						table.insert(levers, util.scale("scale_consumers", station, ids.ICE, drink))
					end
				end
				ctx.market:levers(levers, why)
				ctx.store:set("shortage", "stations", table.concat(hit, ","), why)
				set(ctx, why, "quota", util.num(ctx, T .. "way_a_quota"))
				set(ctx, why, "pulses", ctx.market:pulses(ids.KETTLE))
				ctx.stations:detach(ids.KETTLE, "part.light.warm_green", why)
				ctx.stations:attach(ids.KETTLE, "part.light.dim_green", why)
				local tuned = ctx.pacing:tuned(T .. "stations") or ctx.pacing:tuned(T .. "way_a_quota")
				util.record(
					ctx,
					why,
					"shortage_started",
					{ station = ids.KETTLE, region = ids.SHALLOWS, other = ids.SHORTAGE, ware = ids.ICE },
					util.num(ctx, T .. "way_a_quota"),
					`Ice short at {table.concat(hit, ", ")}{if tuned then " (tuned)" else ""}`
				)
			end,
		}),
	},
	on = {
		dock = function(ev: G.DockEvent, ctx: G.Ctx)
			-- Every docking is an activity event. No player visit or timer is needed.
			local ignored = on(ctx) and (get(ctx, "ignored") == 1
				or ctx.market:pulses(ids.KETTLE) - get(ctx, "pulses") >= util.num(ctx, T .. "ignored_pulses"))
			if ignored then
				-- Even Keel puts Kettle at the top of its route menu, and its docking there
				-- brings the ice. If it can't (a save made before named traders, a craft a
				-- game-master pack removed, or held up for `relief_fallback_pulses` more
				-- Kettle activity), the Compact sends the ice anyway: an ignored shortage
				-- always ends.
				local pulses = ctx.market:pulses(ids.KETTLE)
				local flagged = ctx.store:get("shortage", "relief_pulses")
				local keel = ev.ship == ids.EVEN_KEEL and ev.station == ids.KETTLE
				local fallback = ctx.space:craft(ids.EVEN_KEEL) == nil
					or (
						type(flagged) == "number"
						and pulses - flagged >= util.num(ctx, T .. "relief_fallback_pulses")
					)
				if flagged ~= nil and not keel and not fallback then
					return
				end
				local why = ctx:cause({ summary = "the Compact sees Kettle Dock short of ice", cites = { ev.fact } })
				if flagged == nil then
					ctx.mechanics:set("mech.even_keel_relief", true, why)
					set(ctx, why, "ignored", 1)
					set(ctx, why, "relief_pulses", pulses)
				end
				if keel or fallback then
					resolve(ctx, why, 0)
				end
				return
			end
			if ev.ship ~= ids.PLAYER then
				return
			end
			-- Rimewater's rest lasts until it's back to `rest_until` of full.
			if get(ctx, "rest") == 1 then
				local rime = ctx.beats:field(ids.RIMEWATER)
				if rime ~= nil and rime.left >= util.num(ctx, T .. "rest_until") then
					end_rest(ctx, ctx:cause({ summary = "Rimewater has rested", cites = { ev.fact } }))
					return
				end
			end
			if not on(ctx) then
				return
			end
			-- Odo offers his two ideas at Kettle (line.odo_fenn.08, citing the start).
			if ev.station == ids.KETTLE and get(ctx, "way_b") == 0 then
				local f = ctx.history:last({ kind = "core.shortage_started", other = ids.SHORTAGE })
				if f ~= nil then
					local why = ctx:cause({ summary = "Odo has two ideas about the ice", cites = { f.id } })
					lines.say(
						ctx,
						why,
						{ line = lines.get("line.odo_fenn.08"), fact = f, npc = ids.ODO },
						ids.KETTLE,
						REST
					)
					ctx.talk:offer({
						speaker = ids.ODO,
						place = ids.KETTLE,
						text = "Bring ice from outside the Shallows and sell it here, or at the Wharf or the Hook. Or let Rimewater rest, and bring plate for a condenser.",
						cites = { f.id },
					}, why)
				end
			end
		end,
		-- Way B: the player agrees to let Rimewater rest.
		reply = function(ev: G.ReplyEvent, ctx: G.Ctx)
			if ev.npc ~= ids.ODO or ev.choice ~= ids.REST or not on(ctx) or get(ctx, "way_b") ~= 0 then
				return
			end
			local rime = ctx.beats:field(ids.RIMEWATER)
			if rime == nil then
				return
			end
			local why = ctx:cause({ summary = "Rimewater is left to rest", cites = { ev.fact } })
			set(ctx, why, "way_b", 1)
			set(ctx, why, "rest", 1)
			set(ctx, why, "rest_regrow", rime.regrow)
			ctx.space:set_regrow(ids.RIMEWATER, rime.regrow * util.num(ctx, T .. "rest_factor"), why)
			util.record(
				ctx,
				why,
				"shortage_rest",
				{ ship = ids.PLAYER, station = ids.KETTLE, other = ids.SHORTAGE },
				nil,
				"Rimewater rests while the player brings plate for a condenser"
			)
		end,
		mine = function(ev: G.MineEvent, ctx: G.Ctx)
			if ev.ship ~= ids.PLAYER or ev.ware ~= ids.ICE or not on(ctx) then
				return
			end
			local rimewater = string.sub(ev.rock, 1, #ids.RIMEWATER_ROCKS) == ids.RIMEWATER_ROCKS
			if rimewater and get(ctx, "way_b") == 1 then
				-- Mining Rimewater while it rests ends the rest; way A stays open. Odo
				-- mentions it once, with no penalty.
				local why = ctx:cause({ summary = "the player mined Rimewater while it rested", cites = { ev.fact } })
				set(ctx, why, "way_b", 2)
				end_rest(ctx, why)
				util.record(
					ctx,
					why,
					"shortage_rest_lapsed",
					{ ship = ids.PLAYER, rock = ev.rock, other = ids.SHORTAGE },
					nil,
					"Rimewater's rest lapsed"
				)
				util.say(
					ctx,
					why,
					"line.odo_fenn.rest_lapsed",
					ids.ODO,
					ids.KETTLE,
					"Someone's been cutting at Rimewater, so the rest's off. No harm done. Ice from outside still helps.",
					ev.fact
				)
			elseif not rimewater then
				local why = ctx:cause({ summary = "ice from outside the Shallows", cites = { ev.fact } })
				util.record(
					ctx,
					why,
					"outside_ice",
					{ ship = ids.PLAYER, rock = ev.rock, ware = ids.ICE, other = ids.SHORTAGE },
					ev.amount,
					`{ev.amount} ice mined outside the Shallows`
				)
			end
		end,
		buy = function(ev: G.BuyEvent, ctx: G.Ctx)
			if ev.ship ~= ids.PLAYER or ev.ware ~= ids.ICE or not on(ctx) then
				return
			end
			local st = ctx.stations:get(ev.station)
			if st == nil or st.region == ids.SHALLOWS then
				return
			end
			local why = ctx:cause({ summary = "ice from outside the Shallows", cites = { ev.fact } })
			-- The Compact notices every 20 units of its ice bought during the shortage.
			if ev.station == ids.BRASS then
				local per = util.num(ctx, T .. "brass_ice_per")
				local before = ctx.history:sum({
					kind = "core.outside_ice",
					station = ids.BRASS,
					other = ids.SHORTAGE,
					after = since(ctx),
				})
				local deeds = math.floor((before + ev.quantity) / per) - math.floor(before / per)
				if deeds > 0 then
					ctx.standing:deed({ faction = ids.COMPACT, deed = "deed.compact.shortage_ice", delta = deeds }, why)
				end
			end
			util.record(
				ctx,
				why,
				"outside_ice",
				{ ship = ids.PLAYER, station = ev.station, ware = ids.ICE, other = ids.SHORTAGE },
				ev.quantity,
				`{ev.quantity} ice bought outside the Shallows`
			)
		end,
		sell = function(ev: G.SellEvent, ctx: G.Ctx)
			if ev.ship ~= ids.PLAYER or not on(ctx) then
				return
			end
			if ev.ware == ids.ICE and util.has(stations(ctx), ev.station) then
				-- Way A: ice from outside the Shallows, sold where it's short.
				local spare = sum_since(ctx, "outside_ice") - sum_since(ctx, "relief")
				local n = math.min(ev.quantity, spare)
				if n < 1 then
					return
				end
				local why = ctx:cause({ summary = "ice from outside the Shallows reaches the shortage", cites = { ev.fact } })
				util.record(
					ctx,
					why,
					"relief",
					{ ship = ids.PLAYER, station = ev.station, ware = ids.ICE, other = ids.SHORTAGE },
					n,
					`{n} ice toward {ids.ICE_RELIEF}`
				)
				if sum_since(ctx, "relief") + n >= get(ctx, "quota") then
					resolve(ctx, why, 1)
				end
			elseif ev.ware == ids.PLATE and ev.station == ids.KETTLE and get(ctx, "way_b") == 1 then
				-- Way B: plate for Kettle's condenser, paid at Kettle's plate price.
				local why = ctx:cause({ summary = "plate for Kettle's condenser", cites = { ev.fact } })
				util.record(
					ctx,
					why,
					"condenser",
					{ ship = ids.PLAYER, station = ids.KETTLE, ware = ids.PLATE, other = ids.SHORTAGE },
					ev.quantity,
					`{ev.quantity} plate toward {ids.CONDENSER}`
				)
				if sum_since(ctx, "condenser") + ev.quantity >= util.num(ctx, T .. "way_b_plate") then
					resolve(ctx, why, 2)
				end
			end
		end,
	},
})
