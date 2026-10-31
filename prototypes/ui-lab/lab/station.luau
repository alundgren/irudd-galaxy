-- Docked: the whole station up close. Each module has its sign and opens its room; the
-- station key along the bottom does the same in one click, with news ribbons.
local data = use("data")
local common = use("common")

local function key(state, ui)
	local row = ui.row({ id = "key", gap = 10, place = "bottom" })
	for _, r in data.rooms do
		local news = r.news and not state.seen[r.id] and r.news
		table.insert(
			row,
			ui.tile({
				id = "key_" .. r.id,
				r.name,
				icon = r.icon,
				on = "open",
				args = { room = r.id },
				ribbon = news and { news[1], news[2] } or nil,
				hint = if news then news[3] else `{r.keeper} · {r.line}`,
			})
		)
	end
	return row
end

return function(state, ui)
	local s = ui.screen({ scene = "station", back = "back" })
	for _, r in data.rooms do
		local closed = r.news and r.news[1] == "closed"
		table.insert(
			s,
			ui.hotspot({
				id = "spot_" .. r.id,
				anchor = "module." .. r.id,
				on = "open",
				args = { room = r.id },
				sign = {
					title = r.name,
					keeper = r.keeper,
					line = r.line,
					state = if closed then { "closed", "Closed" } else nil,
				},
			})
		)
	end
	-- Where you are, as words on the sky rather than a box.
	table.insert(
		s,
		ui.column({
			place = "top-left",
			gap = 2,
			ui.title(data.station.name),
			ui.small(`{data.station.faction} · docked`),
		})
	)
	local status = common.status(state, ui)
	status.place = "top-right"
	table.insert(s, status)
	-- The goal appears once the talk that gives it is over.
	if state.goal and not state.talk then
		table.insert(
			s,
			ui.panel({
				id = "goal",
				role = "goal",
				place = "top",
				ui.row({
					gap = 10,
					ui.icon("goal", 22),
					ui.column({ gap = 0, ui.text(state.goal.text), ui.caption(`Goal from {state.goal.from}`) }),
				}),
			})
		)
	end
	table.insert(s, key(state, ui))
	table.insert(s, common.log(state, ui))
	common.add(s, common.bubble(state, ui))
	return s
end
