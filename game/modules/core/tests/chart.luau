local chart = require("../scripts/chart")
local fit = require("../scripts/fit")

local GLASS = { name = "Chart glass", socket = "rack", price = 2500, ware = "plate", units = 20, blueprint = true }

local function key(kind, down, source)
    return { kind = "core." .. kind, payload = { down = down }, source = source }
end

return {
    first_flight_is_a_movement_key_by_hand = function()
        assert(chart.first_flight(key("ahead", true, "keyboard")))
        assert(chart.first_flight(key("left", true, "ui")))
        assert(not chart.first_flight(key("ahead", false, "keyboard")), "a key let go is not a flight")
        assert(not chart.first_flight(key("ahead", true, "script")), "a script's order is not the pilot's")
        assert(not chart.first_flight(key("mine", true, "keyboard")), "only the movement keys")
    end,
    the_goal_pins_before_the_drawing_and_points_nowhere = function()
        local pin = chart.pin(nil, 0, 0)
        assert(pin.kind == "GOAL" and pin.who == "A chart of the reach")
        assert(chart.pin({ name = "Chart glass", found = false, price = 2500, units = 20, ware = "plate" }, 0, 0).who == pin.who)
        assert(not string.find(pin.text, "%d"), "no numbers in a direction")
    end,
    the_pin_says_what_is_still_to_bring = function()
        local p = table.clone(GLASS)
        p.found = true
        local pin = chart.pin(p, 5, 1000)
        assert(pin.who == "Chart glass" and pin.where == "Cinder Kiln")
        assert(string.find(pin.text, "Still to bring: 15 more plate and 1500 more cr.", 1, true), pin.text)
        assert(string.find(chart.pin(p, 20, 2500).text, "You have what it takes.", 1, true))
    end,
    the_drawing_names_its_price = function()
        assert(chart.drawing(GLASS) == "Under the light, lines show in the tag's glass: the drawing of a chart glass. Orrin can make it from 20 plate and 2500 cr.")
        assert(fit.price_text(GLASS) == "20 plate and 2500 cr")
    end,
}
