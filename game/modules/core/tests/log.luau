-- The ship log groups its entries into visits (scripts/log.luau): one line per run of
-- entries in one place, newest first, and what happened in flight between two dockings
-- is a line of its own.
local log = require("../scripts/log")

local function entry(n, at, text)
    return { n = n, at = at, text = text or "", talk = "", open = false }
end

return {
    entries_in_one_place_are_one_visit = function()
        local visits = log.visits({
            entry(1, "", "Hold full: 6 ore aboard."),
            entry(2, "core.kiln", "Docked at Cinder Kiln."),
            entry(3, "core.kiln", "Sold 6 ore at Cinder Kiln for 97 cr."),
            entry(4, "core.kiln", "Launched from Cinder Kiln."),
            entry(5, "core.lantern", "Docked at Lantern Dock."),
        })
        assert(#visits == 3, "flight, the Kiln and Lantern Dock")
        assert(visits[1].at == "core.lantern" and visits[1].first == 5, "newest first")
        assert(visits[2].at == "core.kiln" and #visits[2].entries == 3, "the Kiln's visit holds its three entries")
        assert(visits[3].at == "" and visits[3].first == 1, "the flight before it is its own line")
    end,

    a_return_to_a_station_is_a_new_visit = function()
        local visits = log.visits({
            entry(1, "core.kiln"), entry(2, ""), entry(3, "core.kiln"),
        })
        assert(#visits == 3, "flying away between two dockings splits them")
    end,

    an_empty_log_has_no_visits = function()
        assert(#log.visits({}) == 0, "nothing logged, nothing shown")
    end,
}
