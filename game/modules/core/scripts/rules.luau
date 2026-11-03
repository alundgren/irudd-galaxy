local rules = { CAPACITY = 6, MINE_DISTANCE = 7 }

function rules.distance2(a, b)
    return (a.x - b.x) ^ 2 + (a.y - b.y) ^ 2
end

function rules.transfer(cargo, left)
    return math.max(0, math.min(1, rules.CAPACITY - cargo, left))
end

function rules.sale(cargo, price)
    return cargo * price
end

return rules
