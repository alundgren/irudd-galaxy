--!strict
-- `galaxy pack test packs/gm-examples/kettle_growth`. Each function is a test; it
-- passes if it returns without an error.
local ledger = require("../scripts/ledger")

return {
	stage_needs_both_wares = function()
		assert(ledger.stage_for({ ice = 60, plate = 19 }) == 0, "plate short: no stage")
		assert(ledger.stage_for({ ice = 60, plate = 20 }) == 2, "60 ice and 20 plate: stage 2")
		assert(ledger.stage_for({ ice = 500, plate = 50 }) == 3, "enough for both: stage 3")
	end,

	missing_counts_down_to_the_next_ring = function()
		local need = ledger.missing({ ice = 70, plate = 20 }, 2)
		assert(need ~= nil and need.ice == 50 and need.plate == 30, "stage 3 still needs 50 ice and 30 plate")
		assert(ledger.missing({ ice = 0, plate = 0 }, 3) == nil, "nothing after the last stage")
	end,
}
