-- The status chip's widgets (status.luau shows it): credits, then the hold and what is
-- in it, each with its icon and a count (direction doc 1.5: dark ink with a
-- lantern-gold border). A changed number flashes briefly, so a sale or a cut is seen.
local rules = require("./rules")
local layout = require("./layout")
local kit = require("./kit")
local fit = require("./fit")

-- The chip's width inside its padding (style.toml's chip pad is 12). Each widget has
-- its own width, so nothing is squeezed: credits (CREDITS, room for five digits) and
-- the hold (HOLD) share the top row, and each ware aboard (WARE) the row below, GAP
-- apart; both rows fit in INNER, so the chip stays two rows tall.
local chip = { INNER = layout.CHIP - 24, CREDITS = 124, HOLD = 110, WARE = 64, GAP = 12 }

function chip.build(ctx, ui)
    local pilot = ctx.get("ship", "pilot")
    local hold = ctx.get("ship", "hold")
    local load = rules.load(hold)
    -- Credits and the hold, then each ware aboard: an icon and a count apiece, their
    -- names on hover.
    local wares = {}
    for _, ware in rules.WARES do
        if hold[ware] > 0 then
            table.insert(wares, ui.text { id = "cargo_" .. ware, icon = kit.ware(ware), text = tostring(hold[ware]),
                width = chip.WARE, size = kit.SIZE.body, cue = "flash", hover = hold[ware] .. " " .. ware })
        end
    end
    local children = { ui.row { id = "purse", gap = chip.GAP, children = {
        ui.text { id = "credits", icon = "credits", text = pilot.credits .. " cr", width = chip.CREDITS, size = 22, cue = "flash",
            hover = "Credits" },
        ui.text { id = "hold", icon = "hold", text = "Hold " .. load .. "/" .. fit.hold(ctx), width = chip.HOLD, size = kit.SIZE.body,
            cue = "flash", hover = "Hold: " .. rules.list(hold) .. " aboard, room for " .. fit.hold(ctx) },
    } } }
    if #wares > 0 then table.insert(children, ui.row { id = "cargo", gap = chip.GAP, children = wares }) end
    return kit.chip(ui, { id = "status", align = "top-right", x = layout.M, y = layout.M, width = layout.CHIP, gap = 4, children = children })
end

return chip
