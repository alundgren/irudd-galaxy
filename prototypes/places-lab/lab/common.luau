-- Pieces several screens share: the talk bubble, the status readout, the log, the back
-- button and the receipt.
local data = use("data")
local fit = use("fit")
local common = {}

-- The current line as a bubble over its speaker, or nothing.
function common.bubble(state, ui)
	local t = state.talk
	if not t then
		return nil
	end
	local talk = data.talk[t.id]
	return ui.bubble({
		id = "bubble",
		anchor = if talk.at == "keeper" then "keeper" else "module." .. talk.at,
		speaker = talk.who,
		place_name = talk.place,
		text = talk.lines[t.line],
		more = t.line < #talk.lines,
		on = "talk_next",
	})
end

-- Credits and hold, as a quiet readout.
function common.status(state, ui)
	local total = fit.cargo_total(state)
	local hold = fit.stats(state.fitted).hold
	return ui.panel({
		role = "status",
		ui.row({
			gap = 14,
			ui.row({ gap = 6, ui.icon("coin", 20), ui.figure(`{state.credits} cr`) }),
			ui.row({ gap = 6, ui.icon("hold", 20), ui.figure(`Hold {total} / {hold}`) }),
		}),
	})
end

function common.back(ui)
	return ui.button({ id = "back", "Station", icon = "back", on = "back" })
end

function common.receipt(state, ui)
	local r = state.receipt
	if not r then
		return nil
	end
	local row = ui.row({ gap = 10, ui.text({ r.text, grow = 1 }) })
	if r.amount then
		table.insert(row, ui.state(r.state or "info", r.amount))
	end
	return ui.panel({ role = "receipt", row })
end

-- The ship log: one small button; open, the lines heard so far, newest first.
function common.log(state, ui)
	local col = ui.column({ gap = 6 })
	if state.log_open then
		local box = ui.panel({ role = "status", width = 360, ui.caption("Ship log · newest first") })
		local n = 0
		for i = #state.heard, 1, -1 do
			local h = state.heard[i]
			table.insert(box, ui.small(`{h.who}: {h.text}`))
			n += 1
			if n >= 5 then
				break
			end
		end
		if n == 0 then
			table.insert(box, ui.small("Nothing yet."))
		end
		table.insert(col, box)
	end
	table.insert(col, ui.button({ id = "log", "Log", icon = "log", on = "toggle_log" }))
	col.place = "bottom-left"
	return col
end

-- Adds `child` to `parent` if there is one.
function common.add(parent, child)
	if child then
		table.insert(parent, child)
	end
	return parent
end

return common
