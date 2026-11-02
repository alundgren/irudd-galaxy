return {
    screen = function(ctx, ui)
        local here = ctx.get("player", "engine.location").place
        local life = ctx.get("walker", "engine.life")
        local place = ctx.get("walker", "engine.location").place
        local state = ctx.get("walker", "engine.being").state
        return ui.panel { id = "panel", x = 940, y = 30, width = 300, children = {
            ui.text { text = "A day per being", size = 26 },
            ui.text { text = "Clock: " .. ctx.get("clock", "engine.clock").value },
            ui.text { text = "Being A: " .. ctx.get(place, "engine.room").title .. " / " .. (if life.phase == "" then state else "Moving") },
            ui.text { text = "Click the being to talk. Advance to see the next act." },
            ui.button { id = "advance", text = "Advance clock", on = "advance" },
            ui.button { id = "go", text = if here == "life.room" then "Leave for hall" else "Enter day room", on = "go" },
            ui.button { id = "presentation", text = "Presentation: " .. life.presentation, on = "presentation" },
        } }
    end,
    on = {
        advance = function(_event, ctx) ctx.advance_clock("clock", 1) end,
        go = function(_event, ctx)
            local here = ctx.get("player", "engine.location").place
            ctx.move("player", if here == "life.room" then "hall" else "room")
        end,
        presentation = function(_event, ctx)
            local mode = ctx.get("walker", "engine.life").presentation
            ctx.patch("walker", "engine.life", { presentation = if mode == "states" then "instant" else "states" })
        end,
    },
}
