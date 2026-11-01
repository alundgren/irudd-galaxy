-- Juno's market: wares with plain price labels, real verbs as buttons, and buy buttons
-- that rest (with the reason) while the hold is full.
local data = use("data")
local fit = use("fit")
local common = use("common")

local function ware_row(state, ui, w, full)
	local have = state.cargo[w.id] or 0
	local buttons = ui.row({ gap = 8, width = 150 })
	if w.side == "sell" then
		local none = if have == 0 then `You have no {string.lower(w.name)}` else nil
		table.insert(buttons, ui.button({ id = `sell1_{w.id}`, "Sell 1", on = "sell", args = { ware = w.id }, rest = none }))
		table.insert(buttons, ui.button({ id = `sellall_{w.id}`, "All", on = "sell", args = { ware = w.id, all = true }, rest = none }))
	else
		local why = if full
			then "Hold full: buy buttons rest until you sell"
			elseif state.credits < w.price then `You need {w.price} cr`
			else nil
		table.insert(buttons, ui.button({ id = `buy1_{w.id}`, "Buy 1", on = "buy", args = { ware = w.id }, rest = why }))
		table.insert(buttons, ui.button({ id = `buymax_{w.id}`, "Max", on = "buy", args = { ware = w.id, max = true }, rest = why }))
	end
	-- Prices and counts are plain labels in fixed columns; only the verbs are buttons.
	return ui.row({
		id = "ware_" .. w.id,
		gap = 8,
		ui.text({ w.name, grow = 1 }),
		ui.text({ (if w.side == "sell" then "pays " else "asks ") .. `{w.price} cr`, style = "small", width = 84 }),
		ui.figure({ tostring(have), width = 44 }),
		buttons,
	})
end

return function(state, ui, world)
	local s = ui.screen({ back = "back" })
	local full = fit.cargo_total(state) >= fit.stats(state.fitted).hold
	local panel = ui.panel({
		id = "room",
		role = "room",
		place = "right",
		gap = 12,
		align = "stretch",
		ui.row({ gap = 12, common.back(ui), ui.column({ gap = 0, ui.title(world.place.name), ui.small("Juno Pell keeps the stall") }) }),
		common.status(state, ui),
		ui.row({
			gap = 8,
			ui.caption({ "WARE", grow = 1 }),
			ui.caption({ "PRICE", width = 84 }),
			ui.caption({ "HOLD", width = 44 }),
			ui.spacer({ size = 150 }),
		}),
	})
	for _, w in data.wares do
		table.insert(panel, ware_row(state, ui, w, full))
	end
	if full then
		table.insert(panel, ui.state("info", "Hold full: buy buttons rest until you sell"))
	end
	common.add(panel, common.receipt(state, ui))
	table.insert(s, panel)
	common.people(s, ui, world)
	common.add(s, common.talk(state, ui))
	return s
end
