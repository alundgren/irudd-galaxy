-- The rooms without a screen of their own (the office, the canteen): the room's panel
-- on the right, the people in it to talk to.
local data = use("data")
local common = use("common")

local function keepers(world)
	local names = {}
	for _, id in world.people_here do
		local p = world.people[id]
		table.insert(names, `{p.name}, {string.lower(p.role)}`)
	end
	return if #names > 0 then table.concat(names, " · ") else "Nobody in just now"
end

return function(state, ui, world)
	local s = ui.screen({ back = "back" })
	local rest = {}
	if world.here == "office" then
		table.insert(rest, ui.caption("THE BERTH BOOK"))
		table.insert(rest, ui.text("Berth three: the Wren, yours."))
		for _, n in data.notices do
			table.insert(rest, ui.panel({ role = "sign", ui.small(n) }))
		end
		if state.goal then
			table.insert(
				rest,
				ui.panel({
					role = "goal",
					ui.row({ gap = 10, ui.icon("goal", 22), ui.text({ state.goal.text, grow = 1 }) }),
				})
			)
		end
	elseif world.here == "canteen" then
		table.insert(rest, common.status(state, ui))
		table.insert(
			rest,
			ui.row({
				gap = 10,
				ui.column({ gap = 0, grow = 1, ui.text(data.bowl.name), ui.small(`{data.bowl.price} cr a bowl`) }),
				ui.button({
					id = "buy_bowl",
					"Buy a bowl",
					on = "buy_bowl",
					rest = if state.credits < data.bowl.price then `You need {data.bowl.price} cr` else nil,
				}),
			})
		)
		common.add(rest, common.receipt(state, ui))
	end
	table.insert(s, common.room_panel(world, ui, keepers(world), rest))
	common.people(s, ui, world)
	common.add(s, common.talk(state, ui))
	return s
end
