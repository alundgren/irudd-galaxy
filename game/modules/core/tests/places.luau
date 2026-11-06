-- Beings dressed for where they rest (scripts/places.luau): at an act with a `pose` a
-- being wears the pose's model (a seated figure at a table), and its keeper figure again
-- once its day moves on. One walking in visible states keeps the figure it walks in.
local places = require("../scripts/places")

-- Hessa at her table act, or at her bench. Writes land in the world.
local function world(current, presentation)
    local w = {
        ["core.kettle_skipper"] = {
            keeper = { name = "Hessa Kell", figure = "core.person_hessa" },
            ["engine.life"] = { current = current, presentation = presentation or "instant" },
            ["engine.model"] = { model = "core.person_hessa" },
        },
        ["core.kettle_skipper_act"] = { ["engine.act"] = { state = "minding the tanks" } },
        ["core.kettle_skipper_act2"] = {
            ["engine.act"] = { state = "eating at Ines's table" },
            pose = { model = "core.person_hessa_seated" },
        },
    }
    local ctx = {
        get = function(id, component) return w[id] and w[id][component] end,
        query = function(component)
            local ids = {}
            for id, parts in w do
                if parts[component] then table.insert(ids, id) end
            end
            table.sort(ids)
            return ids
        end,
        patch = function(id, component, fields)
            for k, v in fields do w[id][component][k] = v end
        end,
    }
    return ctx, w
end

local function worn(w)
    return w["core.kettle_skipper"]["engine.model"].model
end

return {
    at_a_posed_act_she_sits = function()
        local ctx, w = world("core.kettle_skipper_act2")
        places.dress(ctx)
        assert(worn(w) == "core.person_hessa_seated", worn(w))
    end,

    her_day_moved_on_she_stands_again = function()
        local ctx, w = world("core.kettle_skipper_act2")
        places.dress(ctx)
        w["core.kettle_skipper"]["engine.life"].current = "core.kettle_skipper_act"
        places.dress(ctx)
        assert(worn(w) == "core.person_hessa", worn(w))
    end,

    one_who_walks_keeps_the_figure_they_walk_in = function()
        local ctx, w = world("core.kettle_skipper_act2", "states")
        places.dress(ctx)
        assert(worn(w) == "core.person_hessa", worn(w))
    end,
}
