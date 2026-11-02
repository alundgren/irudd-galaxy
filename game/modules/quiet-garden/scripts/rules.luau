local rules = {}
rules.GOAL = 3

function rules.water(current)
    return math.min(current + 1, rules.GOAL)
end

function rules.scale(current)
    return 0.25 + 0.25 * current
end

return rules
