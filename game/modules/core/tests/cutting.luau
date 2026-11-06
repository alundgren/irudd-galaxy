-- Cutting takes judgment (rules.HEAT): simulated holds, second by second as the game's
-- cadence banks them, by a pilot who clicks a rock and waits for it to run out against
-- one who moves the beam off a rock when it glazes, onto the warmest other rock with ore.
-- Moving the beam costs `move` seconds (a turn and a click beside a seam's spall or among
-- the floes, a short hop between glassite crystals), and every rock cools meanwhile.
-- These guard the difference, so drift shows: attentive play beats click-and-wait.
local rules = require("../scripts/rules")

local CAPACITY = 24

-- Seconds of cutting (and moving) to fill a hold from `rocks` ({ ware, left, rich }), and
-- the beam moves made, by `style` ("wait" or "attentive"), or `steady` as the rig cuts.
local function hold(rocks, style, move, steady)
    local list = {}
    for _, r in rocks do
        table.insert(list, { ware = r.ware, left = r.left, rich = r.rich or 1, heat = 0 })
    end
    local function cool(except)
        for _, o in list do
            if o ~= except then o.heat = rules.cooled(o.ware, o.heat, 1) end
        end
    end
    local function choose(candidates)
        local best = nil
        for _, i in candidates do
            local o = list[i]
            local warm = rules.phase(o.ware, o.heat) == "warm"
            if best == nil then
                best = i
            else
                local b = list[best]
                local bwarm = rules.phase(b.ware, b.heat) == "warm"
                if (warm and not bwarm) or (warm == bwarm and o.heat > b.heat) then best = i end
            end
        end
        return best
    end
    local cur, got, seconds, work, moves = 1, 0, 0, 0, 0
    while got < CAPACITY and seconds < 600 do
        local r = list[cur]
        local rate = steady and 1 or rules.heat_rate(r.ware, r.heat)
        work += math.floor(rules.TICKS * rate * r.rich + 0.5)
        if not steady then r.heat = rules.warmed(r.ware, r.heat, 1) end
        cool(r)
        local units
        units, work = rules.cut(work)
        units = math.min(units, r.left, CAPACITY - got)
        r.left -= units
        got += units
        seconds += 1
        if got >= CAPACITY then break end
        local nxt = nil
        if r.left <= 0 then
            work = 0
            local left = {}
            for i, o in list do
                if o.left > 0 then table.insert(left, i) end
            end
            if #left == 0 then break end
            nxt = style == "attentive" and choose(left) or left[1]
        elseif style == "attentive" and not steady and rules.phase(r.ware, r.heat) == "glazed" then
            local fresh = {}
            for i, o in list do
                if i ~= cur and o.left > 0 and rules.phase(o.ware, o.heat) ~= "glazed" then table.insert(fresh, i) end
            end
            if #fresh > 0 then
                nxt = choose(fresh)
                work = 0
            end
        end
        if nxt and nxt ~= cur then
            cur = nxt
            moves += 1
            for _ = 1, move do
                seconds += 1
                cool(nil)
            end
        end
    end
    assert(got == CAPACITY, "the hold fills")
    return seconds, moves
end

local function rocks(ware, sizes, rich)
    local list = {}
    for i, n in sizes do
        table.insert(list, { ware = ware, left = n, rich = rich and rich[i] or 1 })
    end
    return list
end

-- The drifts as the game has them: four near ore rocks, a rich seam and its spall, the
-- glassite crystals, the big ice floes.
local NEAR = rocks("ore", { 7, 6, 8, 6 })
local SEAM = rocks("ore", { 24, 6 }, { 1.5, 1 })
local GLASSITE = rocks("glassite", { 8, 6, 7, 9 })
local ICE = rocks("ice", { 20, 16, 14, 12 })

return {
    the_stone_warms_then_glazes = function()
        for ware, h in rules.HEAT do
            assert(rules.phase(ware, 0) == "cold", ware)
            assert(rules.phase(ware, h.warm) == "warm", ware)
            assert(rules.phase(ware, h.glaze) == "glazed", ware)
            assert(h.hot > 1 and h.hot > h.cold and h.cold > h.glazed and h.glazed > 0,
                ware .. ": warm cuts fastest, glazed slowest, and glazed still cuts")
            assert(rules.cooled(ware, h.glaze, 60) == 0, ware .. ": a rock left alone cools right down")
            local most = rules.warmed(ware, 0, 600)
            assert(rules.phase(ware, rules.cooled(ware, most, 3)) ~= "glazed" or h.cool < 1,
                ware .. ": however long the beam sat on it, a glazed rock cools back soon")
        end
        assert(rules.heat_rate("plate", 0) == rules.HEAT.ore.cold, "any other ware cuts like ore")
    end,
    a_seam_rewards_moving_the_beam = function()
        local wait = hold(SEAM, "wait", 1)
        local attentive, moves = hold(SEAM, "attentive", 1)
        assert(attentive <= 0.75 * wait, "seam: " .. attentive .. " s against " .. wait .. " s")
        assert(moves >= 2, "onto the spall and back")
        -- An attentive hold is no slower than the steady beam the rig holds, plus a move or two.
        assert(attentive <= hold(SEAM, "wait", 1, true) + 2)
    end,
    ice_floes_reward_moving_the_beam = function()
        local wait = hold(ICE, "wait", 1)
        local attentive = hold(ICE, "attentive", 1)
        assert(attentive <= 0.7 * wait, "ice: " .. attentive .. " s against " .. wait .. " s")
    end,
    glassite_rewards_working_the_crystals_in_turn = function()
        local wait = hold(GLASSITE, "wait", 2)
        local attentive = hold(GLASSITE, "attentive", 2)
        assert(attentive <= 0.8 * wait, "glassite: " .. attentive .. " s against " .. wait .. " s")
    end,
    near_ore_is_never_slower_attentive = function()
        local wait = hold(NEAR, "wait", 2)
        local attentive = hold(NEAR, "attentive", 2)
        assert(attentive <= wait, "near ore: " .. attentive .. " s against " .. wait .. " s")
    end,
    waiting_still_pays = function()
        -- Click and wait fills every hold, never more than twice the steady beam's time.
        for _, drift in { NEAR, SEAM, GLASSITE, ICE } do
            assert(hold(drift, "wait", 1) <= 2 * hold(drift, "wait", 1, true))
        end
    end,
}
