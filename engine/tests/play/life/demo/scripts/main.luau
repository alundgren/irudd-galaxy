return {
    screen = function(ctx, ui)
        local here = ctx.get("player", "engine.location").place
        local life = ctx.get("walker", "engine.life")
        local place = ctx.get("walker", "engine.location").place
        local state = ctx.get("walker", "engine.being").state
        local children = {
            ui.text { text = "A day per being", size = 26 },
            ui.text { text = "Clock: " .. ctx.get("clock", "engine.clock").value },
            ui.text { text = "Being A: " .. ctx.get(place, "engine.room").title .. " / " .. (if life.phase == "" then state else "Moving") },
            ui.text { text = "Click the being to talk. Advance to see the next act." },
            ui.button { id = "advance", hover = "Advance the authored day by one clock unit.", text = "Advance clock", on = "advance" },
            ui.button { id = "go", hover = "Travel to the other place.", text = if here == "life.room" then "Leave for hall" else "Enter day room", on = "go" },
            ui.button { id = "presentation", hover = "Switch between visible walks and immediate arrival.", text = "Presentation: " .. life.presentation, on = "presentation" },
        }
        if here == place then
            table.insert(children, ui.hover { id = "walker-help", anchor = "being.life.walker", text = "Being A follows the authored day. Click to talk." })
        end
        return ui.row { y = 30, pad = 0, gap = 0, children = {
            ui.column {},
            ui.panel { id = "panel", width = 300, children = children },
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
