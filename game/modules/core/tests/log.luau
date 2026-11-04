-- The ship log groups its entries into visits (scripts/log.luau): one line per run of
-- entries in one place, newest first, a new one after each leaving, and what happened
-- in flight between two dockings is a line of its own. Each visit comes to one short
-- line, and what is pinned stays pinned however much history has been dropped.
local log = require("../scripts/log")
local layout = require("../scripts/layout")

local function entry(n, at, text, leaves)
    return { n = n, at = at, text = text or "", talk = "", open = false, leaves = leaves or false }
end

-- A world for the log's reads: { [entity] = { [component] = fields } }.
local function world(w)
    return {
        get = function(id, component) return w[id] and w[id][component] end,
        query = function(component)
            local ids = {}
            for id, components in w do
                if components[component] then table.insert(ids, id) end
            end
            table.sort(ids)
            return ids
        end,
    }
end

local function kiln()
    return { ["core.kiln"] = { port = { name = "Cinder Kiln", berth = "core.kiln_berth", visits = 1 } } }
end

return {
    entries_in_one_place_are_one_visit = function()
        local visits = log.visits({
            entry(1, "", "Hold full: 6 ore aboard."),
            entry(2, "core.kiln", "Docked at Cinder Kiln."),
            entry(3, "core.kiln", "Sold 6 ore at Cinder Kiln for 97 cr."),
            entry(4, "core.kiln", "Launched from Cinder Kiln.", true),
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

    a_return_with_nothing_logged_between_is_a_new_visit = function()
        -- Docked, launched, flew about without a log entry, docked at the same station.
        local visits = log.visits({
            entry(1, "core.kiln", "Docked at Cinder Kiln."),
            entry(2, "core.kiln", "Launched from Cinder Kiln.", true),
            entry(3, "core.kiln", "Docked at Cinder Kiln."),
        })
        assert(#visits == 2, "two dockings, two visits: got " .. #visits)
        assert(visits[1].first == 3 and #visits[1].entries == 1, "the return is the newest visit, on its own")
        assert(visits[2].first == 1 and #visits[2].entries == 2, "the first visit ends with its leaving")
    end,

    a_visit_is_one_short_line = function()
        -- The fitting visit of tests/fit.inputs: every entry a full sentence or two.
        local visit = { at = "core.kiln", first = 2, entries = {
            entry(2, "core.kiln", "Docked at Cinder Kiln."),
            entry(3, "core.kiln", "Sold 6 ore to Cinder Kiln for 97 cr."),
            entry(4, "core.kiln", "Fitted the cutting lens at Cinder Kiln for 90 cr: your beam cuts 2 ore a second instead of 1."),
            entry(5, "core.kiln", "Took off the cutting lens at Cinder Kiln, into your locker: fit it again here for nothing."),
            entry(6, "core.kiln", "Fitted the cutting lens at Cinder Kiln: your beam cuts 2 ore a second instead of 1."),
            entry(7, "core.kiln", "Launched from Cinder Kiln.", true),
        } }
        local who, line = log.summary(world(kiln()), visit)
        assert(who == "", "nobody was heard")
        assert(#line <= log.SHORT, "one short line, not " .. #line .. " characters: " .. line)
        assert(string.find(line, "^sold 6 ore"), "it starts with what was done first: " .. line)
        assert(not string.find(line, "beam", 1, true) and not string.find(line, "locker", 1, true),
            "the explanations wait for Lines: " .. line)
        assert(string.find(line, "+2 more", 1, true), "what doesn't fit is counted: " .. line)
    end,

    a_visit_with_people_is_still_one_short_line = function()
        local w = kiln()
        for i, name in { "Ines Varga", "Bram Holt", "Odo Kade" } do
            w["core.talk" .. i] = { ["engine.talk"] = { title = name } }
        end
        local function said(n, i)
            return { n = n, at = "core.kiln", text = "...", talk = "core.talk" .. i, open = false, leaves = false }
        end
        local visit = { at = "core.kiln", first = 1, entries = {
            entry(1, "core.kiln", "Docked at Cinder Kiln."), said(2, 1), said(3, 2), said(4, 3),
            entry(5, "core.kiln", "Bought 3 plate at Cinder Kiln for 86 cr."),
            entry(6, "core.kiln", "Sold 2 ore and 1 plate to Cinder Kiln for 140 cr. 1 plate went into the galley: 4 of 10."),
        } }
        local who, line = log.summary(world(w), visit)
        assert(who == "Ines Varga +2", "the first heard, and how many more: " .. who)
        assert(#who + 3 + #line <= log.SHORT, "names and line together stay short: " .. who .. " · " .. line)
    end,

    an_offer_heard_stays_pinned_after_its_entry_is_dropped = function()
        local w = {
            ["core.lantern"] = { port = { name = "Lantern Dock", visits = 3 } },
            ["core.cook"] = { ["engine.location"] = { place = "core.lantern" }, keeper = { name = "Ines Varga" } },
            ["core.gift"] = {
                gift = { ware = "rations", units = 1, take = "core.take", answered = false, heard = true },
                ["engine.talk"] = { speaker = "core.cook", title = "Ines Varga" },
            },
            ["core.take"] = { ["engine.talk_choice"] = { line = "core.gift2", text = "Take it", order = 1 } },
            ["core.no"] = { ["engine.talk_choice"] = { line = "core.gift2", text = "No, thanks", order = 2 } },
        }
        -- Sixty newer entries, none of them the offer's.
        local list = {}
        for n = 28, 87 do table.insert(list, entry(n, "core.lantern", "Docked at Lantern Dock.")) end
        local pins = log.pinned(world(w), list)
        assert(#pins == 1 and pins[1].kind == "OFFER", "the unanswered offer is still pinned: " .. #pins)
        assert(pins[1].who == "Ines Varga" and pins[1].where == "Lantern Dock")
        w["core.gift"].gift.answered = true
        assert(#log.pinned(world(w), list) == 0, "answered, it goes")
    end,

    a_building_stays_pinned_after_the_visits_are_dropped = function()
        local w = {
            ["core.lantern"] = { port = { name = "Lantern Dock", visits = 1 } },
            ["core.site"] = { build = { station = "core.lantern", ware = "plate", need = 10, given = 3, done = false, name = "Galley" } },
        }
        local pins = log.pinned(world(w), { entry(90, "", "Rock worked out.") })
        assert(#pins == 1 and pins[1].kind == "WANTED", "docked there once: still pinned")
        w["core.lantern"].port.visits = 0
        assert(#log.pinned(world(w), {}) == 0, "never docked there: not pinned")
    end,

    the_open_log_clears_the_ship_at_960x540 = function()
        -- At 960x540 the ship sits mid-window; moored on its cradle its tail reaches
        -- about 416px from the left (game/evidence/core-ui-kit/README.md).
        assert(layout.LOG and layout.M + layout.LOG < 416, "the log must end short of the ship")
    end,

    an_empty_log_has_no_visits = function()
        assert(#log.visits({}) == 0, "nothing logged, nothing shown")
    end,
}
