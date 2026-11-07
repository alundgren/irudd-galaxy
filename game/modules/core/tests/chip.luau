-- The status chip (scripts/chip.luau) stays readable with every ware aboard: each row's
-- widgets have widths, and those widths and the gaps between them fit inside the chip,
-- so no count is ever squeezed into fragments, and it stays two rows tall so it keeps
-- clear of a bubble below it at 960x540.
local chip = require("../scripts/chip")

local function world(hold, size, gained)
    local w = {
        ship = {
            pilot = { credits = 12345, gained = gained or 0 },
            hold = hold,
            fit = { hold = size },
        },
    }
    return { get = function(id, component) return w[id] and w[id][component] end, query = function() return {} end }
end

-- A ui that returns each widget as built, its kind beside its fields.
local ui = setmetatable({}, { __index = function(_, kind)
    return function(w)
        w.kind = kind
        return w
    end
end })

local function rows_fit(widget)
    local inner = chip.INNER
    for _, child in widget.children do
        if child.kind == "row" then
            local used = (#child.children - 1) * (child.gap or 0)
            for _, c in child.children do
                assert(c.width, (c.id or c.kind) .. " has no width of its own")
                used += c.width
            end
            assert(used <= inner, (child.id or "row") .. " needs " .. used .. "px of " .. inner)
            -- A cell that is itself a row (the hold and what this cut brought in) fits its width.
            for _, c in child.children do
                if c.kind == "row" then
                    local own = (#c.children - 1) * (c.gap or 0)
                    for _, cc in c.children do own += cc.width end
                    assert(own <= c.width, c.id .. " needs " .. own .. "px of " .. c.width)
                end
            end
        else
            assert(child.width and child.width <= inner, (child.id or child.kind) .. " is wider than the chip")
        end
    end
end

local function cargo(widget)
    for _, child in widget.children do
        if child.id == "cargo" then return child end
    end
    return nil
end

return {
    every_ware_aboard_fits = function()
        local w = chip.build(world({ ore = 20, ice = 12, glassite = 1, plate = 1, rations = 1 }, 36), ui)
        rows_fit(w)
        local row = cargo(w)
        assert(row and #row.children == 5, "all five wares show")
        assert(#w.children == 2, "two rows: credits and the hold, then the wares")
        for _, c in row.children do assert(c.icon, c.id .. " has an icon") end
    end,

    a_full_pods_hold_fits = function()
        rows_fit(chip.build(world({ ore = 12, plate = 12, rations = 12 }, 36), ui))
    end,

    -- Mid-cut, the hold's cell shows two two-digit counts and what the cut brought in.
    a_full_hold_mid_cut_fits = function()
        local w = chip.build(world({ ore = 36 }, 36, 17), ui)
        rows_fit(w)
        assert(w.children[1].children[2].id == "hold_row", "the cut's count shows beside the hold")
    end,

    an_empty_hold_has_no_cargo_row = function()
        local w = chip.build(world({ ore = 0, plate = 0, rations = 0 }, 24), ui)
        rows_fit(w)
        assert(cargo(w) == nil, "nothing aboard, no cargo row")
    end,
}
