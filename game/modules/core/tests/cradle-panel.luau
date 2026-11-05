-- The fitting room's click way (scripts/cradle.luau): Choose sits where a part would go
-- from, a chosen part's socket offers Fit here while the others say why not, the
-- preview and Put back take the budgets' place, and a fitted part's choice is taken
-- off from the tray. tests/fit-click.toml plays the same events.
local cradle = require("../scripts/cradle")

local function part(id, name, socket, price, power, mass, fitted)
    return { name = name, socket = socket, price = price, power = power, mass = mass, hold = socket == "hold" and 3 or 0,
        mine = socket == "drill" and 1 or 0, speed = socket == "engine" and 5 or 0, order = price, effect = "does " .. id,
        said = "", model = "", owned = fitted, fitted = fitted }
end

-- On Cinder Kiln's cradle with 200 cr, the lens fitted or not, and `chosen` picked.
local function world(lens_fitted, chosen)
    local w = {
        ["core.ship"] = { hold = { ore = 0 }, fit = { hull_speed = 20 } },
        ["core.lens"] = { part = part("lens", "Cutting lens", "drill", 90, 2, 1, lens_fitted) },
        ["core.pods"] = { part = part("pods", "Hold pods", "hold", 70, 0, 3, false) },
        ["core.thrusters"] = { part = part("thrusters", "Tuned thrusters", "engine", 110, 2, 1, false) },
    }
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
            return component == "part" and { "core.lens", "core.pods", "core.thrusters" } or {}
        end,
    }
    local pilot = { credits = 200, deal = "", sale = 0, chosen_part = chosen or "" }
    return ctx, pilot
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

return {
    choose_sits_in_the_empty_socket = function()
        local w = rows(false, nil)
        assert(w.choose_lens and w.choose_lens.on == "choose" and w.choose_lens.payload.value == "core.lens")
        assert(w.socket_drill.children[1] == w.choose_lens, "the lens is chosen from its own socket")
        assert(w.budget and not w.preview, "the budgets show until a part is chosen")
    end,
    a_chosen_part_is_offered_where_it_goes = function()
        local w = rows(false, "core.lens")
        assert(w.offer_drill.on == "fit" and w.offer_drill.payload.socket == "drill" and w.offer_drill.payload.value == "core.lens")
        assert(string.find(w.offer_hold.text, "^Cannot: ") and w.offer_hold.on == nil, "the wrong socket says why, and offers nothing")
        assert(w.preview and w.put_back.on == "choose" and w.put_back.payload.value == "core.lens")
        assert(string.find(w.changes.text, "^If you fit the cutting lens: 90 cr"), w.changes.text)
        assert(not w.budget, "the preview stands in the budgets' place")
    end,
    a_fitted_part_is_chosen_from_the_tray = function()
        local w = rows(true, nil)
        assert(w.socket_drill.children[1].id == "part_lens", "the fitted lens sits in its socket")
        assert(w.choose_lens and w.off_drill.children[2] == w.choose_lens, "its Choose is in the tray's On the ship")
        local chosen = rows(true, "core.lens")
        assert(chosen.offer_off_drill.on == "unfit" and chosen.offer_off_drill.payload.value == "core.lens")
        assert(string.find(chosen.changes.text, "^If you take off the cutting lens: "), chosen.changes.text)
    end,
}
