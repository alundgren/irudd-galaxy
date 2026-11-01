return {
    on = {
        increment = function(_, ctx)
            local state = ctx.get("state", "counter")
            ctx.patch("state", "counter", {value = state.value + 1})
        end,
        toggle = function(event, ctx)
            if event.source == "keyboard" and not event.payload.down then return end
            local state = ctx.get("state", "counter")
            ctx.patch("state", "counter", {open = not state.open})
        end,
    },
    screen = function(ctx, ui)
        local state = ctx.get("state", "counter")
        if not state.open then return nil end
        return ui.panel { width = 360, children = {
            ui.text { id = "title", text = "Screen workshop", size = 26 },
            ui.text { id = "count", text = "Clicks recorded: " .. state.value },
            ui.list { id = "list", children = {
                ui.text { text = "Measured with the bundled font" },
                ui.text { text = "Space opens this screen again" },
            } },
            ui.row { children = {
                ui.button { id = "increment", text = "Add one", on = "increment" },
                ui.button { id = "hide", text = "Hide", on = "toggle" },
            } },
            ui.panel { id = "label", width = 210, anchor = "marker#label", children = {
                ui.text { text = "Following an anchor", size = 18 },
            } },
        } }
    end,
}
