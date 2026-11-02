--!strict
-- The counting rules, loaded with require("./sums").

local sums = {}

-- How much one bump adds. Wrong on purpose: the tests expect 1.
function sums.step(): number
	return 2
end

function sums.add(a: number, b: number): number
	return a + b
end

return sums
