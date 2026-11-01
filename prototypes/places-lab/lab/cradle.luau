-- Orrin's cradle: the Wren in the bay with its sockets, the rack and the locker on the
-- right. Drag a part onto a socket to fit it, or a fitted part to the locker. When a part
-- is picked up this view runs once and gives every target its verdict; while the part is
-- in hand the engine shows them without asking again.
local data = use("data")
local fit = use("fit")
local common = use("common")

local function part_row(ui, p, extra)
	return ui.row({
		gap = 10,
		ui.icon("part", 24),
		ui.column({ gap = 0, grow = 1, ui.text(p.name), ui.small(p.effect) }),
		extra,
	})
end

-- What fitting the dragged part on `socket` would change.
local function preview(state, ui, drag, socket)
	local p = data.parts[drag.part]
	local before = fit.stats(state.fitted)
	local after = fit.stats(fit.after(state, drag.part, drag.from, socket))
	local col = ui.panel({ role = "status", gap = 4, ui.caption(`IF YOU FIT {string.upper(p.name)}`) })
	-- Every line reads "more is better", so the arrow's direction and its colour agree.
	local function line(label, a, b, unit)
		local word, st = fit.delta(a, b)
		if word then
			table.insert(col, ui.row({ gap = 8, ui.small({ `{label} {a} to {b}{unit}`, grow = 1 }), ui.state(st, word) }))
		end
	end
	local top = data.hull.power
	line("Top speed", before.speed, after.speed, "")
	line("Turn", before.turn, after.turn, "°/s")
	line("Hold", before.hold, after.hold, "")
	line("Power free", top - before.power, top - after.power, ` of {top}`)
	if drag.from == "rack" then
		table.insert(col, ui.row({ gap = 8, ui.small({ "Costs", grow = 1 }), ui.state("loss", `-{p.price} cr`) }))
	end
	local replaced = state.fitted[socket]
	if replaced and replaced ~= drag.part then
		table.insert(col, ui.small(`{data.parts[replaced].name} goes to the locker`))
	end
	return col
end

local function budgets(state, ui)
	local s = fit.stats(state.fitted)
	local h = data.hull
	local mass_label = `Mass {s.mass} · the hull carries {h.free_mass} freely`
	if s.over > 0 then
		mass_label ..= ` · {s.over} over: slower, wider turns`
	end
	return ui.column({
		gap = 10,
		align = "stretch",
		ui.meter({ id = "power", `Power {s.power} of {h.power} used`, value = s.power, max = h.power }),
		ui.meter({ id = "mass", mass_label, value = s.mass, max = h.free_mass }),
		ui.small(`Top speed {s.speed} · turns {s.turn}°/s · hold {s.hold}`),
	})
end

return function(state, ui, world)
	local ship = {}
	for socket, part in pairs(state.fitted) do
		ship[socket] = data.parts[part].look
	end
	local s = ui.screen({ back = "back", ship = ship })
	local drag = state.drag

	-- The sockets: drop targets on the ship.
	for _, sock in data.sockets do
		local part = state.fitted[sock.id]
		local label = `{sock.name} · ` .. (if part then data.parts[part].name else "empty")
		local ok, reason = nil, nil
		local pv = nil
		if drag then
			ok, reason = fit.socket_verdict(state, drag, sock.id)
			if ok then
				pv = preview(state, ui, drag, sock.id)
			end
		end
		table.insert(
			s,
			ui.drop({
				id = "socket_" .. sock.id,
				anchor = "socket." .. sock.id,
				side = sock.side,
				label,
				selected = part ~= nil,
				on = "fit",
				args = { socket = sock.id },
				ok = ok,
				reason = reason,
				preview = pv,
			})
		)
	end
	-- Fitted parts can be picked up off their sockets.
	for _, sock in data.sockets do
		local part = state.fitted[sock.id]
		if part then
			local p = data.parts[part]
			table.insert(
				s,
				ui.drag({
					id = "fitted_" .. sock.id,
					anchor = "socket." .. sock.id,
					on = "pick",
					cancel = "drop_cancel",
					args = { part = part, from = sock.id, look = p.look },
					ui.panel({ role = "plain", pad = 8, part_row(ui, p) }),
				})
			)
		end
	end

	local panel = ui.panel({
		id = "room",
		role = "room",
		place = "right",
		gap = 12,
		align = "stretch",
		ui.row({ gap = 12, common.back(ui), ui.column({ gap = 0, ui.title(world.place.name), ui.small("Orrin Vale fits parts to the Wren") }) }),
		common.status(state, ui),
		budgets(state, ui),
		ui.caption("ON ORRIN'S RACK · DRAG ONTO A SOCKET"),
	})
	for _, id in data.rack do
		local p = data.parts[id]
		table.insert(
			panel,
			ui.drag({
				id = "rack_" .. id,
				on = "pick",
				cancel = "drop_cancel",
				args = { part = id, from = "rack", look = p.look },
				part_row(ui, p, ui.figure(`{p.price} cr`)),
			})
		)
	end
	-- The locker: spare parts, and where fitted parts go when taken off.
	local lok, lreason = nil, nil
	if drag then
		lok, lreason = fit.locker_verdict(state, drag)
	end
	local locker = ui.drop({
		id = "locker",
		on = "unfit",
		ok = lok,
		reason = lreason,
		ui.caption("THE WREN'S LOCKER"),
	})
	for _, id in state.locker do
		local p = data.parts[id]
		table.insert(
			locker,
			ui.drag({
				id = "locker_" .. id,
				on = "pick",
				cancel = "drop_cancel",
				args = { part = id, from = "locker", look = p.look },
				part_row(ui, p),
			})
		)
	end
	if #state.locker == 0 then
		table.insert(locker, ui.small("Empty. Parts you take off wait here."))
	end
	table.insert(panel, locker)
	common.add(panel, common.receipt(state, ui))
	table.insert(s, panel)
	common.people(s, ui, world)
	common.add(s, common.talk(state, ui))
	return s
end
