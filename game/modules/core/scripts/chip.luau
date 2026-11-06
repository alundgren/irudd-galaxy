-- The status chip's widgets (status.luau shows it): credits, then the hold and what is
-- in it, each with its icon and a count (direction doc 1.5: dark ink with a
-- lantern-gold border). A changed number flashes briefly, so a sale or a cut is seen.
local rules = require("./rules")
local layout = require("./layout")
local kit = require("./kit")
local fit = require("./fit")
local project = require("./project")

-- The chip's width inside its padding (style.toml's chip pad is 12). Each widget has
-- its own width, so nothing is squeezed: credits (CREDITS, room for five digits) and
-- the hold (HOLD) share the top row, GAP apart, and each ware aboard (WARE) the row
-- below, CARGO_GAP apart; both rows fit in INNER even with all five wares aboard, so the
-- chip stays two rows tall. A WARE holds an icon and two digits: the biggest hold is 36
-- (the pods). A part that lets a hold carry 100 or more needs a wider WARE or a third
-- row, or three-digit counts wrap into fragments.
-- While the beam cuts, the hold's cell holds a short count (SHORT) and what this cut has
-- brought in (GAINED, `+N`) beside it, GAINED_GAP apart, in the same HOLD width.
-- Goods carried for someone are marked with whose they are ("4 Ines's", OWNED wide)
-- while the row has room for it, and on hover always (direction doc 4).
local chip = { INNER = layout.CHIP - 24, CREDITS = 124, HOLD = 140, WARE = 52, OWNED = 104, GAP = 12, CARGO_GAP = 4,
    SHORT = 88, GAINED = 46, GAINED_GAP = 6 }

function chip.build(ctx, ui)
    local pilot = ctx.get("ship", "pilot")
    local hold = ctx.get("ship", "hold")
    local load = rules.load(hold)
    -- Credits and the hold, then each ware aboard: an icon and a count apiece, their
    -- names on hover.
    local wares = {}
    local owner, theirs = project.owner(ctx)
    local kinds = 0
    for _, ware in rules.WARES do
        if (hold[ware] or 0) > 0 then kinds += 1 end
    end
    local marked = owner and kinds * (chip.WARE + chip.CARGO_GAP) + chip.OWNED - chip.WARE <= chip.INNER
    for _, ware in rules.WARES do
        if (hold[ware] or 0) > 0 then
            local mine = ware ~= theirs or not owner
            table.insert(wares, ui.text { id = "cargo_" .. ware, icon = kit.ware(ware),
                text = hold[ware] .. ((not mine and marked) and " " .. owner or ""),
                width = (not mine and marked) and chip.OWNED or chip.WARE, size = kit.SIZE.body, cue = "flash",
                hover = mine and hold[ware] .. " " .. ware or owner .. " " .. hold[ware] .. " " .. ware .. ": carried for her" })
        end
    end
    local hover = "Hold: " .. project.carried(ctx, hold) .. " aboard, room for " .. fit.hold(ctx)
    local count = ui.text { id = "hold", icon = "hold", text = "Hold " .. load .. "/" .. fit.hold(ctx), width = chip.HOLD,
        size = kit.SIZE.body, cue = "flash", hover = hover }
    -- What the beam has cut this time, beside the hold, counting up as each unit lands.
    local gained = pilot.gained or 0
    if gained > 0 then
        count = ui.row { id = "hold_row", width = chip.HOLD, gap = chip.GAINED_GAP, children = {
            ui.text { id = "hold", icon = "hold", text = load .. "/" .. fit.hold(ctx), width = chip.SHORT, size = kit.SIZE.body,
                cue = "flash", hover = hover },
            ui.text { id = "gained", spans = kit.amount(gained, nil, true), width = chip.GAINED, size = kit.SIZE.body, cue = "flash",
                hover = gained .. " cut this time" },
        } }
    end
    local children = { ui.row { id = "purse", gap = chip.GAP, children = {
        ui.text { id = "credits", icon = "credits", text = pilot.credits .. " cr", width = chip.CREDITS, size = 22, cue = "flash",
            hover = "Credits" },
        count,
    } } }
    if #wares > 0 then table.insert(children, ui.row { id = "cargo", gap = chip.CARGO_GAP, children = wares }) end
    return kit.chip(ui, { id = "status", align = "top-right", x = layout.M, y = layout.M, width = layout.CHIP, gap = 4, children = children })
end

return chip
