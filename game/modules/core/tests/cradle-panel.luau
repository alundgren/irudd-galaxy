-- The fitting room's click way (scripts/cradle.luau): Choose sits where a part would go
-- from, a chosen part's socket offers Fit here while the others say why not, the
-- preview and Put back stand in a box beside the panel, which keeps its budgets, and a
-- fitted part's choice is taken off from the tray. tests/fit-click.toml plays the same
-- events.
local cradle = require("../scripts/cradle")
local layout = require("../scripts/layout")

local function part(id, name, socket, price, power, mass, fitted)
    return { name = name, socket = socket, price = price, power = power, mass = mass, hold = socket == "hold" and 3 or 0,
        mine = socket == "drill" and 1 or 0, speed = socket == "engine" and 5 or 0, order = price, effect = "does " .. id,
        said = "", model = "", owned = fitted, fitted = fitted }
end

-- The ship's world with `parts` ({ [bare id] = part }) and 200 cr, `chosen` picked.
local function world_of(parts, chosen)
    local w = { ["core.ship"] = { hold = { ore = 0 }, fit = { hull_speed = 20 } } }
    local ids = {}
    for id, p in parts do
        w["core." .. id] = { part = p }
        table.insert(ids, "core." .. id)
    end
    table.sort(ids)
    local ctx = {
        get = function(id, component)
            local e = w[id] or w["core." .. id]
            local c = e and e[component]
            if not c then return nil end
            local copy = {}
            for k, v in c do copy[k] = v end
            return copy
        end,
        query = function(component)
            return component == "part" and ids or {}
        end,
    }
    local pilot = { credits = 200, deal = "", sale = 0, chosen_part = chosen or "" }
    return ctx, pilot
end

-- On Cinder Kiln's cradle with 200 cr, the lens fitted or not, and `chosen` picked.
local function world(lens_fitted, chosen)
    return world_of({
        lens = part("lens", "Cutting lens", "drill", 90, 2, 1, lens_fitted),
        pods = part("pods", "Hold pods", "hold", 70, 0, 3, false),
        thrusters = part("thrusters", "Tuned thrusters", "engine", 110, 2, 1, false),
    }, chosen)
end

-- A ui that returns each widget as built, its kind beside its fields.
local ui = setmetatable({}, { __index = function(_, kind)
    return function(w)
        w.kind = kind
        return w
    end
end })

-- Every widget in the panel, by ID.
local function by_id(children)
    local found = {}
    local function walk(list)
        for _, c in list do
            if c.id then found[c.id] = c end
            if c.children then walk(c.children) end
        end
    end
    walk(children)
    return found
end

local function rows(lens_fitted, chosen)
    local ctx, pilot = world(lens_fitted, chosen)
    return by_id(cradle.rows(ctx, ui, pilot, "Fitting cradle", "Orrin Vale keeps it."))
end

-- The preview box beside the panel, by widget ID (the box itself as `box`); nil with
-- nothing chosen.
local RIGHT = layout.M + layout.ROOM + 8
local function preview(ctx, pilot)
    local box = cradle.preview(ctx, ui, pilot, RIGHT)
    if not box then return nil end
    local found = by_id({ box })
    found.box = box
    return found
end

-- A text's words, from its spans or its text.
local function words(w)
    if w.text then return w.text end
    local out = ""
    for _, span in w.spans do out ..= type(span) == "string" and span or span.text end
    return out
end

-- The parts as start.toml has them, so the longest lines are the real ones. `fitted`
-- and `owned` are sets of bare IDs.
local function real(fitted, owned)
    local function p(id, name, socket, price, power, mass, hold, mine, speed, order)
        return { name = name, socket = socket, price = price, power = power, mass = mass, hold = hold, mine = mine,
            speed = speed, order = order, effect = "", said = "", model = "", owned = owned[id] or fitted[id] or false,
            fitted = fitted[id] or false }
    end
    return {
        lens = p("lens", "Cutting lens", "drill", 90, 2, 1, 0, 1, 0, 1),
        pods = p("pods", "Hold pods", "hold", 70, 0, 3, 3, 0, 0, 2),
        thrusters = p("thrusters", "Tuned thrusters", "engine", 110, 2, 1, 0, 0, 5, 3),
    }
end

-- How tall a text comes out at size 16, wrapped by words in `width` pixels. Generous
-- on purpose: 8 px a character (the cradle's 16 px text measures about 7 in shots)
-- and 22 px a line, so a pass here has room to spare on screen.
local function wrapped(text, width)
    local per, lines, line = width // 8, 1, 0
    for word in string.gmatch(text, "%S+") do
        local need = line == 0 and #word or line + 1 + #word
        if need > per and line > 0 then
            lines += 1
            line = #word
        else
            line = need
        end
    end
    return lines * 22
end

return {
    choose_sits_in_the_empty_socket = function()
        local w = rows(false, nil)
        assert(w.choose_lens and w.choose_lens.on == "choose" and w.choose_lens.payload.value == "core.lens")
        assert(w.socket_drill.children[1] == w.choose_lens, "the lens is chosen from its own socket")
        assert(w.budget and not w.put_back, "the budgets show, and nothing is chosen")
        assert(preview(world(false, nil)) == nil, "no preview until a part is chosen")
    end,
    a_chosen_part_is_offered_where_it_goes = function()
        local w = rows(false, "core.lens")
        assert(w.offer_drill.on == "fit" and w.offer_drill.payload.socket == "drill" and w.offer_drill.payload.value == "core.lens")
        assert(string.find(w.offer_hold.text, "^Cannot: ") and w.offer_hold.on == nil, "the wrong socket says why, and offers nothing")
        assert(w.budget and w.keeper and not w.put_back, "the panel keeps the keeper and the budgets")
        local p = preview(world(false, "core.lens"))
        assert(p.put_back.on == "choose" and p.put_back.payload.value == "core.lens")
        assert(p.preview_title.text == "If you fit the cutting lens", p.preview_title.text)
        assert(words(p.change_1) == "90 cr", words(p.change_1))
        assert(words(p.change_2) == "Mines 1 to 2 a second + better", words(p.change_2))
    end,
    a_fitted_part_is_chosen_from_the_tray = function()
        local w = rows(true, nil)
        assert(w.socket_drill.children[1].id == "part_lens", "the fitted lens sits in its socket")
        assert(w.choose_lens and w.off_drill.children[2] == w.choose_lens, "its Choose is in the tray's On the ship")
        local chosen = rows(true, "core.lens")
        assert(chosen.offer_off_drill.on == "unfit" and chosen.offer_off_drill.payload.value == "core.lens")
        local p = preview(world(true, "core.lens"))
        assert(p.preview_title.text == "If you take off the cutting lens", p.preview_title.text)
        assert(words(p.change_1) == "Mines 2 to 1 a second - worse", words(p.change_1))
    end,
    every_change_shows_in_full_at_960x540 = function()
        -- Every part chosen with every fit the power allows, bought or from the locker,
        -- on the real numbers. The longest is the lens from the locker with the pods
        -- on: its cost, the beam, the speed and turns the pods' mass costs, and the
        -- power: five changes.
        local fits = { {}, { lens = true }, { pods = true }, { thrusters = true }, { lens = true, pods = true },
            { pods = true, thrusters = true } }
        local tallest, most = 0, 0
        for _, fitted in fits do
            for _, owned in { {}, { lens = true, pods = true, thrusters = true } } do
                for _, id in { "lens", "pods", "thrusters" } do
                    local p = preview(world_of(real(fitted, owned), "core." .. id))
                    local inner = p.box.width - 20
                    local height, count = 0, 0
                    for _, line in p.changes.children do
                        if line.kind == "button" then
                            height += line.height
                        else
                            assert(line.height == nil, line.id .. " grows with its words, never clips them")
                            height += wrapped(words(line), inner) + p.changes.gap
                        end
                        if string.find(line.id, "^change_") then count += 1 end
                    end
                    tallest, most = math.max(tallest, height + 20), math.max(most, count)
                end
            end
        end
        assert(most == 5, "the longest preview has five changes, got " .. most)
        -- At 960x540 the box sits beside the room panel, on screen, and ends above the
        -- ship on its cradle, whose top is about 280 px down.
        local box = preview(world(false, "core.lens")).box
        assert(box.align == "top-right" and box.x == RIGHT, "beside the room panel")
        assert(960 - box.x - box.width >= layout.M, "on screen at 960 wide")
        assert(box.y + tallest <= 280, "the tallest preview ends above the ship: " .. (box.y + tallest))
    end,
}
