--!strict
-- Change type 1: grow a station from deliveries (content/06-game-master.md).
--
-- Every unit of ice or plate delivered or sold at Kettle Dock counts toward Odo's
-- greenhouse. When the totals pay for the next stage, the station's stage goes up,
-- its kit parts are attached, and Odo offers a line citing the deliveries. The world
-- is the memory: totals come from history, the last built stage from the declared
-- store, so nothing lives in Luau variables.
local G = require("@galaxy")
local ledger = require("./ledger")

local KETTLE = "station.kettle_dock"
local ODO = "npc.odo_fenn"
local GLASSITE = "ware.glassite"

local function units(ctx: G.Ctx, ware: string): number
	return ctx.history:sum({ kind = "deliver", station = KETTLE, ware = ware })
		+ ctx.history:sum({ kind = "sell", station = KETTLE, ware = ware })
end

local function totals(ctx: G.Ctx): ledger.Totals
	return { ice = units(ctx, ledger.ICE), plate = units(ctx, ledger.PLATE) }
end

-- The latest delivery or sale of a ware at Kettle, to cite.
local function last_of(ctx: G.Ctx, ware: string): string?
	local best: G.Fact? = nil
	for _, kind in ipairs({ "deliver", "sell" }) do
		local f = ctx.history:last({ kind = kind, station = KETTLE, ware = ware })
		if f and (best == nil or f.tick > best.tick) then
			best = f
		end
	end
	return if best then best.id else nil
end

local function built(ctx: G.Ctx): number
	local v = ctx.store:get("progress", "built")
	return if type(v) == "number" then v else 0
end

local function grow(station: string, fact: string, ctx: G.Ctx)
	if station ~= KETTLE then
		return
	end
	local from = math.max(ctx.stations:stage(KETTLE), built(ctx))
	local to = ledger.stage_for(totals(ctx))
	if to <= from then
		return
	end

	-- Cite the delivery that completed the stage and the latest of each ware.
	local cites = { fact }
	for _, ware in ipairs({ ledger.ICE, ledger.PLATE }) do
		local id = last_of(ctx, ware)
		if id and id ~= fact then
			table.insert(cites, id)
		end
	end
	local why = ctx:cause({ summary = `Kettle Dock greenhouse stage {to} from the player's ice and plate`, cites = cites })

	ctx.stations:set_stage(KETTLE, to, why)
	for s = math.max(from + 1, 2), to do
		for _, part in ipairs(ledger.STAGES[s].parts) do
			ctx.stations:attach(KETTLE, part, why)
		end
	end
	if to >= 3 and ctx.market:line(KETTLE, GLASSITE) == nil then
		-- The orchard lamps need lamp-glass: Kettle starts buying glassite.
		ctx.market:add_line(KETTLE, GLASSITE, {
			stock = 0,
			price = 66,
			base_price = 66,
			min_price = 40,
			max_price = 110,
			consumption = 1,
		}, why)
	end
	ctx.history:record({
		kind = "gm.stage_built",
		subjects = { station = KETTLE },
		amount = to,
		note = `Kettle Dock stage {to}`,
	}, why)
	ctx.talk:offer({ speaker = ODO, place = KETTLE, text = ledger.STAGES[to].line, cites = cites }, why)
	ctx.map:note({ place = KETTLE, text = "Kettle Dock: a new greenhouse ring", cites = cites }, why)
	ctx.store:set("progress", "built", to, why)
end

return G.script({
	id = "kettle_growth",
	on = {
		sell = function(ev: G.SellEvent, ctx: G.Ctx)
			grow(ev.station, ev.fact, ctx)
		end,
		deliver = function(ev: G.DeliverEvent, ctx: G.Ctx)
			grow(ev.station, ev.fact, ctx)
		end,
	},
	after = {
		-- After three dockings at Kettle, Odo tells you what the next ring needs.
		regulars = G.after({ dockings = 3, station = KETTLE }, function(ctx: G.Ctx, _fire: G.TriggerFire)
			local dock = ctx.history:last({ kind = "dock", station = KETTLE })
			local stage = math.max(ctx.stations:stage(KETTLE), built(ctx))
			local need = ledger.missing(totals(ctx), stage)
			if dock == nil or need == nil then
				return
			end
			local why = ctx:cause({ summary = "Odo names what the next greenhouse ring needs", cites = { dock.id } })
			ctx.talk:offer({
				speaker = ODO,
				place = KETTLE,
				text = `Odo's greenhouse needs {need.ice} more ice and {need.plate} more plate for its next ring.`,
				cites = { dock.id },
			}, why)
		end),
	},
})
