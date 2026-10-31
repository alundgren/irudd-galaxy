--!strict
-- Shared numbers and checks, loaded with require("./rules").

local rules = {}

rules.STEP = 10
rules.PACED_EVERY = 5

-- On the ground layer and lifted to the deck's height: step onto the deck.
function rules.reached_deck(at): boolean
	return at.layer == 0 and at.height >= 1.9
end

return rules
