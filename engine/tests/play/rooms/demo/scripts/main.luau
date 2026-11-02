return {
    screen = function(ctx, ui)
        local here = ctx.get("player", "engine.location").place
        local title = ctx.get(here, "engine.room").title
        local visits = ctx.get("state", "visits").count
        local children = { ui.text { text = title, size = 28 },
            ui.text { text = "Arrivals: " .. visits } }
        if here == "rooms.station" then
            table.insert(children, ui.button { id = "enter_a", text = "Enter Room A", on = "go", payload = { place = "rooms.room_a" } })
            table.insert(children, ui.button { id = "enter_b", text = "Enter Room B", on = "go", payload = { place = "rooms.room_b" } })
        else
            table.insert(children, ui.button { id = "leave", text = "Return to station", on = "go", payload = { place = "rooms.station" } })
        end
        local widgets = { ui.panel { id = "room_panel", x = 940, y = 40, width = 280, children = children } }
        if here == "rooms.room_a" then
            table.insert(widgets, ui.text { text = "First", width = 90, anchor = "being.rooms.first.face", y = -24 })
            table.insert(widgets, ui.text { text = "Second", width = 90, anchor = "being.rooms.second.face", y = -24 })
        elseif here == "rooms.room_b" then
            table.insert(widgets, ui.text { text = "Third", width = 90, anchor = "being.rooms.third.face", y = -24 })
        end
        return ui.column { height = 680, children = widgets }
    end,
    on = {
        go = function(event, ctx) ctx.move("player", event.payload.place) end,
        ["engine.place_arrived"] = function(_event, ctx)
            local visits = ctx.get("state", "visits")
            ctx.patch("state", "visits", { count = visits.count + 1 })
        end,
    },
}
