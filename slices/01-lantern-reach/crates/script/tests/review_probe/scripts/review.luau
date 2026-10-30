--!strict
local G = require("@galaxy")

local PLAYER = "ship.player"
local KETTLE = "station.kettle_dock"
local WHARF = "station.lantern_wharf"
local BRASS = "station.brassmouth_exchange"
local COMPACT = "faction.tollway_compact"

return G.script({
	id = "review",
	beats = {
		G.beat({
			id = "beat.t03_views",
			act = 2,
			kind = "situation",
			goal = "Fly near the far spot while the Shallows ice is thin.",
			-- b10 and b12's shape: a spot within 500 u, and a field at 70% or less.
			done = G.test(function(ctx: G.Ctx): boolean
				local d = ctx.beats:distance({ x = 5950, y = 1500, region = "region.cinder_drift" })
				local f = ctx.beats:field("field.shallows.ice")
				return d ~= nil and d <= 500 and f ~= nil and f.left <= 0.7
			end),
		}),
	},
	on = {
		dock = function(ev: G.DockEvent, ctx: G.Ctx)
			if ev.ship ~= PLAYER then
				return
			end
			local why = ctx:cause({ summary = "the review probe reacts to a docking", cites = { ev.fact } })
			if ev.station == BRASS then
				-- Two deeds in one call: the second must see the first.
				ctx.standing:deed({ faction = COMPACT, deed = "deed.review.one", delta = 8 }, why)
				ctx.standing:deed({ faction = COMPACT, deed = "deed.review.two", delta = 8 }, why)
			elseif ev.station == KETTLE and (ctx.beats:pilot().cargo["ware.ice"] or 0) >= 5 then
				ctx.beats:deliver({ station = KETTLE, ware = "ware.ice", quantity = 5 }, why)
			elseif ev.station == WHARF then
				local p = ctx.beats:pilot().position
				local r = ctx.beats:rock("rock.shallows.ice.03")
				local d = ctx.beats:distance(KETTLE)
				local note = string.format(
					"%.0f,%.0f rock=%d/%d field=%s distance=%.0f",
					if p then p.x else -1,
					if p then p.y else -1,
					if r then r.remaining else -1,
					if r then r.initial else -1,
					if r then r.field else "?",
					d or -1
				)
				ctx.history:record({ kind = "review.views", note = note }, why)
			end
		end,
	},
})
