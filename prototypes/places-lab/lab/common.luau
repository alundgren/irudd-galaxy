-- Pieces several screens share: the talk, the people in a room, the room's panel, the
-- status readout, the log, the back button and the receipt.
local data = use("data")
local fit = use("fit")
local common = {}

-- The current line, said by its person. How it shows (a bubble, a portrait...) is the
-- talk variant's choice, not the screen's.
function common.talk(state, ui)
	local t = state.talk
	if not t then
		return nil
	end
	local talk = data.talk[t.id]
	local before = {}
	for i = 1, t.line - 1 do
		table.insert(before, talk.lines[i])
	end
	return ui.talk({
		who = talk.who,
		text = talk.lines[t.line],
		more = t.line < #talk.lines,
		before = before,
		on = "talk_next",
	})
end

-- Everyone in this room, each a hotspot on their figure: click to talk.
function common.people(s, ui, world)
	for _, id in world.people_here do
		local p = world.people[id]
		local first = string.split(p.name, " ")[1]
		table.insert(
			s,
			ui.hotspot({
				id = "person_" .. id,
				anchor = "person." .. id,
				on = "talk_to",
				args = { who = id },
				sign = { title = p.name, keeper = p.role, line = `Talk to {first}` },
			})
		)
	end
	return s
end

-- A room's panel on the right: the way back, its name, who keeps it, then `rest`.
function common.room_panel(world, ui, subtitle, rest)
	local panel = ui.panel({
		id = "room",
		role = "room",
		place = "right",
		gap = 12,
		align = "stretch",
		ui.row({
			gap = 12,
			common.back(ui),
			ui.column({ gap = 0, grow = 1, ui.title(world.place.name), ui.small(subtitle) }),
		}),
	})
	for _, w in rest do
		table.insert(panel, w)
	end
	return panel
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
