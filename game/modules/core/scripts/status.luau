-- The status chip, top right: credits, then the hold and what is in it, each with its
-- icon and a count (direction doc 1.5: dark ink with a lantern-gold border). It shows in flight
-- and on the whole station; inside a room the room shows what that room needs, so the
-- chip steps aside. A changed number flashes briefly, so a sale or a cut is seen.
local rules = require("./rules")
local layout = require("./layout")
local kit = require("./kit")
local fit = require("./fit")

local function chip(ctx, ui)
    local pilot = ctx.get("ship", "pilot")
    local hold = ctx.get("ship", "hold")
    local load = rules.load(hold)
    -- Two rows, so the chip stays short: credits, then the hold and each ware aboard,
    -- an icon and a count apiece (their names on hover).
    local row = { ui.text { id = "hold", icon = "hold", text = "Hold " .. load .. "/" .. fit.hold(ctx), size = kit.SIZE.body,
        cue = "flash", hover = "Hold: " .. rules.list(hold) .. " aboard, room for " .. fit.hold(ctx) } }
    for _, ware in rules.WARES do
        if hold[ware] > 0 then
            table.insert(row, ui.text { id = "cargo_" .. ware, icon = kit.ware(ware), text = tostring(hold[ware]),
                size = kit.SIZE.body, cue = "flash", hover = hold[ware] .. " " .. ware })
        end
    end
    local children = {
        ui.text { id = "credits", icon = "credits", text = pilot.credits .. " cr", size = 22, cue = "flash", hover = "Credits" },
        ui.row { id = "cargo", gap = 10, children = row },
    }
    return kit.chip(ui, { id = "status", align = "top-right", x = layout.M, y = layout.M, width = layout.CHIP, gap = 4, children = children })
end

return {
    screen = function(ctx, ui)
        local pilot = ctx.get("ship", "pilot")
        local here = ctx.get("ship", "engine.location").place
        local space = here == "core.reach"
        local docked = here ~= "" and ctx.get(here, "port") ~= nil
        if not space and not docked then return nil end
        if docked and pilot.room ~= "" then return nil end
        return chip(ctx, ui)
    end,
}
