local labels = require("../scripts/labels")

-- The stations' outsides as start.toml places them.
local STATIONS = { { x = -24, y = -18 }, { x = 30, y = -22 } }

local function trader(id, x, y, moored)
    return { id = id, at = { x = x, y = y }, moored = moored or false }
end

return {
    a_trader_near_the_ship_is_named = function()
        local shown = labels.shown({ x = 0, y = 0 }, { trader("a", 5, 5) }, STATIONS)
        assert(shown.a == true)
    end,
    far_off_traders_are_named_too = function()
        -- Distance is no limit: the engine hides a yielding label off screen or over the HUD.
        local shown = labels.shown({ x = 0, y = 0 }, { trader("a", 12.5, 0), trader("b", 0, 30), trader("c", 60, 30) }, STATIONS)
        assert(shown.a == true and shown.b == true and shown.c == true)
    end,
    no_label_over_a_station = function()
        -- Near Lantern Dock's hull, even right beside the player.
        local shown = labels.shown({ x = -20, y = -4 }, { trader("a", -22, -6) }, STATIONS)
        assert(shown.a == false)
    end,
    moored_traders_wear_none = function()
        local shown = labels.shown({ x = 0, y = 0 }, { trader("a", 3, 0, true) }, STATIONS)
        assert(shown.a == false)
    end,
    close_traders_are_both_offered = function()
        -- Two labels that would overlap are kept apart by the engine's yield, not here.
        local shown = labels.shown({ x = 0, y = 0 }, { trader("a", 2, 2), trader("b", 4, 3) }, STATIONS)
        assert(shown.a == true and shown.b == true)
    end,
    docked_player_sees_no_labels = function()
        local shown = labels.shown(nil, { trader("a", 1, 1) }, STATIONS)
        assert(shown.a == false)
    end,
    labels_sit_away_from_the_ship = function()
        assert(labels.below({ x = 0, y = 0 }, { x = 1, y = -3 }), "a trader below the ship: its label below it")
        assert(not labels.below({ x = 0, y = 0 }, { x = -2, y = 3 }), "above the ship: its label above it")
    end,
}
