local rules = require("./rules")

return {
    on = {
        water = function(_, ctx)
            local growth = ctx.get("garden", "growth")
            local nextWater = rules.water(growth.water)
            ctx.patch("garden", "growth", { water = nextWater })
            ctx.patch("flower", "engine.model", { scale = rules.scale(nextWater) })
            if growth.water < rules.GOAL and nextWater == rules.GOAL then
                ctx.patch("keeper", "engine.being", { state = "Enjoying the bloom" })
                ctx.record("bloomed", {})
            end
        end,
        reset = function(_, ctx)
            ctx.patch("garden", "growth", { water = 0 })
            ctx.patch("flower", "engine.model", { scale = rules.scale(0) })
            ctx.patch("keeper", "engine.being", { state = "Tending the garden" })
        end,
    },
    screen = function(ctx, ui)
        local growth = ctx.get("garden", "growth")
        local keeper = ctx.get("keeper", "engine.being")
        local ready = growth.water == rules.GOAL
        return ui.panel { id = "garden_panel", x = 24, y = 24, width = 340, gap = 16, children = {
            ui.text { text = "Quiet Garden", size = 28 },
            ui.text { text = "A small kindness, one cup at a time.", size = 18 },
            ui.text { text = "Keeper: " .. keeper.state, size = 18 },
            ui.text { id = "progress", text = "Water: " .. tostring(growth.water) .. " / 3", size = 20 },
            ui.text { text = ready and "The flower is fully open. Stay a while." or "Give the flower three cups of water.", size = 18 },
            ready and ui.button { id = "reset", text = "Plant another seed", on = "reset", height = 52 }
                or ui.button { id = "water", text = "Water the flower", on = "water", height = 52 },
        } }
    end,
}
