-- The status chip, top right (chip.luau builds it): credits, the hold and what is in
-- it. It shows in flight and on the whole station; inside a room the room shows what
-- that room needs, so the chip steps aside.
local chip = require("./chip")

return {
    screen = function(ctx, ui)
        local pilot = ctx.get("ship", "pilot")
        local here = ctx.get("ship", "engine.location").place
        local space = here == "core.reach"
        local docked = here ~= "" and ctx.get(here, "port") ~= nil
        if not space and not docked then return nil end
        if docked and pilot.room ~= "" then return nil end
        return chip.build(ctx, ui)
    end,
}
