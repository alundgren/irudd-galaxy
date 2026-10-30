--!strict
-- Odo's greenhouse ledger: what each Kettle Dock stage needs and adds
-- (content/01-galaxy.md, "Kettle Dock build stages"). Pacing thresholds are pack data,
-- never engine constants. Pure functions, so tests/ledger.luau can check them.

export type Totals = { ice: number, plate: number }
export type Stage = { ice: number, plate: number, parts: { string }, line: string }

local ledger = {}

ledger.ICE = "ware.ice"
ledger.PLATE = "ware.refined_plate"

-- Cumulative needs. Stage 1 comes from the core pack's beat b03, not from here.
ledger.STAGES = {
	[2] = {
		ice = 60,
		plate = 20,
		parts = { "part.kettle.greenhouse_ring_a" },
		line = "Second ring's lit. Mostly your ice. And your plate, mostly.",
	},
	[3] = {
		ice = 120,
		plate = 50,
		parts = { "part.kettle.greenhouse_ring_b", "part.kettle.orchard_lamps" },
		line = "The orchard lamps are on. We'll need glassite for them now, if you're passing.",
	},
} :: { [number]: Stage }

ledger.LAST_STAGE = 3

-- The highest stage these totals pay for (0 if none).
function ledger.stage_for(have: Totals): number
	local stage = 0
	for s = 2, ledger.LAST_STAGE do
		local need = ledger.STAGES[s]
		if have.ice >= need.ice and have.plate >= need.plate then
			stage = s
		end
	end
	return stage
end

-- What's still missing for the stage after `stage`, or nil at the last one.
function ledger.missing(have: Totals, stage: number): Totals?
	local need = ledger.STAGES[math.max(stage + 1, 2)]
	if need == nil then
		return nil
	end
	return { ice = math.max(need.ice - have.ice, 0), plate = math.max(need.plate - have.plate, 0) }
end

return ledger
