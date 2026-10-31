-- The other rooms (office, notices, canteen): a small panel beside the module.
local data = use("data")
local common = use("common")

return function(state, ui)
	local room
	for _, r in data.rooms do
		if r.id == state.room then
			room = r
		end
	end
	local s = ui.screen({ scene = "station", focus = room.id, back = "back" })
	local panel = ui.panel({
		id = "room",
		role = "room",
		place = "right",
		width = 420,
		gap = 12,
		align = "stretch",
		ui.row({
			gap = 12,
			common.back(ui),
			ui.column({ gap = 0, grow = 1, ui.title(room.name), ui.small(`{room.keeper} · {room.line}`) }),
		}),
	})
	if room.id == "notices" then
		for _, n in data.notices do
			table.insert(panel, ui.panel({ role = "sign", ui.text(n) }))
		end
	elseif room.news and room.news[1] == "closed" then
		table.insert(panel, ui.state("closed", "Closed for now"))
		table.insert(panel, ui.text(room.news[3]))
	else
		table.insert(panel, ui.text("Ilse looks up from the berth book."))
	end
	table.insert(s, ui.hotspot({
		anchor = "module." .. room.id,
		on = "back",
		selected = true,
		sign = { title = room.name, keeper = room.keeper, line = "Back to the whole station" },
	}))
	table.insert(s, panel)
	common.add(s, common.bubble(state, ui))
	return s
end
