-- The exchange panel (scripts/station.luau) keeps selling on screen however much is
-- aboard (direction doc 8): Sell everything and the last deal's receipt come before the
-- wares, and the wares scroll in a list no taller than a 960x540 window leaves them.
local station = require("../scripts/station")

local ROOM = "core.lantern_exchange"

local function line(ware, role, base, stock)
    return { room = ROOM, ware = ware, role = role, base = base, target = 30, stock = stock, rate = 3 }
end

-- Docked at Lantern Dock in its exchange (the ship is in the exchange's room, a place
-- inside the station's), with every ware aboard and a deal just made.
-- The exchange here trades all five wares, more than any core room does, as the worst case.
local function world(hold, deal)
    local w = {
        ship = {
            pilot = { room = ROOM, credits = 69, deal = deal, sale = 37, log_open = false, gliding = false },
            ["engine.location"] = { place = "core.lantern_exchange_room" },
            hold = hold,
            fit = { hold = 9 },
            taught = { talk = 0 },
        },
        ["core.lantern"] = { port = { name = "Lantern Dock" } },
        ["core.lantern_exchange_room"] = { ["engine.place"] = { parent = "core.lantern" } },
        [ROOM] = {
            facility = { station = "core.lantern", name = "Exchange", keeper = "core.lantern_trader", fits = false, order = 1,
                place = "core.lantern_exchange_room" },
            ["engine.position"] = { x = 5, y = -5 },
        },
        ["core.lantern_trader"] = { keeper = { name = "Tova Reyes" } },
        ["core.galley"] = { build = { station = "core.lantern", ware = "plate", need = 10, given = 0, name = "Galley", done = false } },
        ["core.m_ore"] = { market = line("ore", "buys", 12, 24) },
        ["core.m_ice"] = { market = line("ice", "buys", 10, 22) },
        ["core.m_glassite"] = { market = line("glassite", "buys", 20, 10) },
        ["core.m_plate"] = { market = line("plate", "buys", 34, 18) },
        ["core.m_rations"] = { market = line("rations", "sells", 14, 30) },
    }
    local kinds = { market = { "core.m_ore", "core.m_ice", "core.m_glassite", "core.m_plate", "core.m_rations" },
        build = { "core.galley" }, facility = { ROOM } }
    return {
        get = function(id, component) return w[id] and w[id][component] end,
        query = function(component) return kinds[component] or {} end,
    }
end

-- A ui that returns each widget as built, its kind beside its fields.
local ui = setmetatable({}, { __index = function(_, kind)
    return function(w)
        w.kind = kind
        return w
    end
end })

local function panel(hold, deal)
    local frame = station.screen(world(hold, deal), ui)
    return frame.children[1]
end

local function index_of(children, id)
    for i, c in children do
        if c.id == id then return i end
    end
    return nil
end

local EVERY_WARE = { ore = 2, ice = 3, glassite = 1, plate = 1, rations = 1 }

return {
    selling_comes_before_the_wares = function()
        local p = panel(EVERY_WARE, "Sold 1 plate.")
        local sell, receipt, wares = index_of(p.children, "sell"), index_of(p.children, "receipt"), index_of(p.children, "wares")
        assert(sell and receipt and wares, "Sell everything, the receipt and the wares all show")
        assert(sell < wares, "Sell everything sits above the wares")
        assert(receipt < wares, "the receipt sits above the wares")
    end,

    the_wares_scroll_within_a_small_window = function()
        local p = panel(EVERY_WARE, "Sold 1 plate.")
        local wares = p.children[index_of(p.children, "wares")]
        local list = wares.children[#wares.children]
        assert(list.kind == "scroll", "the wares are a scrolling list")
        -- 540 tall, less the margins, the title, keeper, a two-line ledger, Sell
        -- everything, the receipt and the sheet's own frame.
        assert(list.height <= 150, "the list is " .. list.height .. "px, more than 960x540 leaves it")
        assert(index_of(list.children, "sell_plate"), "each ware's own Sell is in the list")
    end,

    a_short_list_does_not_stand_taller_than_its_rows = function()
        local p = panel({ ore = 0, ice = 0, glassite = 0, plate = 0, rations = 0 }, "")
        local wares = p.children[index_of(p.children, "wares")]
        local list = wares.children[#wares.children]
        assert(index_of(p.children, "sell") == nil and index_of(p.children, "receipt") == nil)
        -- The frame, the heads, four ware rows, the galley line and the rations row with
        -- its buttons: all of it shows, with nothing to scroll.
        assert(list.height == 24 + 24 + 4 * 32 + 26 + 94, "the list is " .. list.height .. "px")
    end,
}
