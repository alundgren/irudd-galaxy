return {
    screen = function(ctx, ui)
        local here = ctx.get("player", "engine.location").place
        local children = { ui.text { text = ctx.get(here, "engine.room").title, size = 28 },
            ui.text { text = "Click a figure to talk. Click its bubble to continue." } }
        if here == "talk.station" then
            table.insert(children, ui.button { id = "enter_a", hover = "Enter the first conversation room.", text = "Enter Room A", on = "go", payload = { place = "talk.room_a" } })
            table.insert(children, ui.button { id = "enter_b", hover = "Enter the second conversation room.", text = "Enter Room B", on = "go", payload = { place = "talk.room_b" } })
        else
            table.insert(children, ui.button { id = "leave", text = "Return to hall", on = "go", payload = { place = "talk.station" } })
            if here == "talk.room_a" then
                table.insert(children, ui.button { id = "move", hover = "Walk the second speaker to the other spot.", text = "Move second speaker", on = "move_speaker" })
            else
                table.insert(children, ui.button { id = "cover", text = "Cover / uncover face", on = "cover" })
            end
        end
        for _, id in { "first", "second", "third" } do
            if ctx.get(id, "engine.location").place == here then
                table.insert(children, ui.hover { id = id .. "-help", anchor = "being.talk." .. id, text = "Click this speaker to hear its conversation." })
            end
        end
        return ui.panel { id = "room_panel", x = 940, y = 40, width = 280, children = children }
    end,
    on = {
        go = function(event, ctx) ctx.move("player", event.payload.place) end,
        move_speaker = function(_event, ctx)
            local loc = ctx.get("second", "engine.location")
            ctx.walk("second", if loc.spot == "talk.room_a_middle" then "room_a_right" else "room_a_middle", 1.0)
        end,
        cover = function(_event, ctx)
            local model = ctx.get("third", "engine.model").model
            ctx.patch("third", "engine.model", { model = if model == "talk.covered" then "talk.being" else "talk.covered" })
        end,
    },
}
