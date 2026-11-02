--!strict
-- `galaxy-gm test <this module folder>`: each function is a test that passes if it
-- returns without an error.
local rules = require("../scripts/rules")

return {
	a_walker_reaches_the_deck_at_its_height = function()
		assert(rules.reached_deck({ layer = 0, height = 2.0 }), "2.0 high on the ground reaches the deck")
		assert(rules.reached_deck({ layer = 0, height = 1.9 }), "1.9 is close enough")
	end,

	a_walker_below_or_on_the_deck_does_not_climb = function()
		assert(not rules.reached_deck({ layer = 0, height = 1.0 }), "1.0 is half way up the ramp")
		assert(not rules.reached_deck({ layer = 1, height = 2.0 }), "already on the deck")
	end,

	steps_pace_every_few_seconds = function()
		assert(rules.STEP == 1, "one step a second")
		assert(rules.PACED_EVERY == 5, "paced every fifth step")
	end,
}
