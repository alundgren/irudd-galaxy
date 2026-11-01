-- Docked: the whole station from outside. Each module has its sign and takes you to its
-- room; the station key along the bottom does the same in one click.
local common = use("common")

-- Who is in a module's room, by name.
local function keepers(world, module)
	local room = world.places[module].enter
	local names = {}
	for _, p in world.people do
		if p.place == room then
			table.insert(names, p.name)
		end
	end
	table.sort(names)
	return table.concat(names, ", ")
end

local function room_name(world, module)
	local m = world.places[module]
	return if m.enter then world.places[m.enter].name else m.name
end

local function key(world, ui)
	local row = ui.row({ id = "key", gap = 10, place = "bottom" })
	for _, id in world.modules do
		local m = world.places[id]
		table.insert(
			row,
			ui.tile({
				id = "key_" .. id,
				room_name(world, id),
				icon = m.enter or "office",
				on = "go",
				args = { place = id },
				hint = `{keepers(world, id)} · {m.line}`,
			})
		)
	end
	return row
end

return function(state, ui, world)
	local s = ui.screen({ back = "back" })
	for _, id in world.modules do
		local m = world.places[id]
		table.insert(
			s,
			ui.hotspot({
				id = "spot_" .. id,
				anchor = "place." .. id,
				on = "go",
				args = { place = id },
				sign = { title = room_name(world, id), keeper = keepers(world, id), line = m.line },
			})
		)
	end
	-- Where you are, as words on the sky rather than a box.
	table.insert(
		s,
		ui.column({
			place = "top-left",
			gap = 2,
			ui.title(world.station.name),
			ui.small(`{world.station.line} · docked`),
		})
	)
	local status = common.status(state, ui)
	status.place = "top-right"
	table.insert(s, status)
	local goal = state.goal
		or { text = "Ilse Marrow is waiting for you in the dock office", from = "the dock" }
	table.insert(
		s,
		ui.panel({
			id = "goal",
			role = "goal",
			place = "top",
			ui.row({
				gap = 10,
				ui.icon("goal", 22),
				ui.column({ gap = 0, ui.text(goal.text), ui.caption(`Goal from {goal.from}`) }),
			}),
		})
	)
	table.insert(s, key(world, ui))
	table.insert(s, common.log(state, ui))
	return s
end
