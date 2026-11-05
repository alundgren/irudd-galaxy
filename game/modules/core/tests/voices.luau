-- An office keeper met for the first time after their station's build is finished
-- says it is open (scripts/voices.luau), not that it is still being raised.
local voices = require("../scripts/voices")

-- A world with a keeper's three-line intro and one build, recording patches.
local function world(done)
    local w = {
        ["core.galley"] = { build = { station = "core.lantern", name = "Galley", done = done } },
        ["core.bram_intro"] = { ["engine.talk"] = { first = "core.bram_intro1" } },
        ["core.bram_intro1"] = { ["engine.talk_line"] = { text = "Welcome.", next = "core.bram_intro2" } },
        ["core.bram_intro2"] = { ["engine.talk_line"] = { text = "I'm Bram.", next = "core.bram_intro3" } },
        ["core.bram_intro3"] = { ["engine.talk_line"] = { text = "We're raising a galley. It needs plate." } },
    }
    local patched = {}
    local ctx = {
        get = function(id, component) return w[id] and w[id][component] end,
        query = function(component)
            local ids = {}
            for id, components in w do
                if components[component] then table.insert(ids, id) end
            end
            table.sort(ids)
            return ids
        end,
        patch = function(id, component, fields) table.insert(patched, { id = id, component = component, fields = fields }) end,
    }
    return ctx, patched
end

return {
    a_finished_build_closes_the_intro_on_it_being_open = function()
        local ctx, patched = world(true)
        voices.intro_close(ctx, "core.bram", "core.lantern")
        assert(#patched == 1, "one line changed")
        assert(patched[1].id == "core.bram_intro3", "the intro's last line")
        assert(patched[1].fields.text == "Our galley's open now. Go and eat.", patched[1].fields.text)
    end,

    an_unfinished_build_leaves_the_intro_as_written = function()
        local ctx, patched = world(false)
        voices.intro_close(ctx, "core.bram", "core.lantern")
        assert(#patched == 0, "nothing changed")
    end,

    another_station_s_build_leaves_the_intro_as_written = function()
        local ctx, patched = world(true)
        voices.intro_close(ctx, "core.bram", "core.kiln")
        assert(#patched == 0, "nothing changed")
    end,
}
