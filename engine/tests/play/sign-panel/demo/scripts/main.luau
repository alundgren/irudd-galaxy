-- Deliberately wrong: the button opens a wide panel across the middle of the window,
-- over the host's name sign. The first screen is clear, so only check's presses find it.
return {
    screen = function(ctx, ui)
        local open = ctx.get("state", "ui_state").open
        local button = ui.button { id = "toggle", align = "bottom-left", x = 20, y = 20, width = 200, text = if open then "Close list" else "Open list", on = "toggle" }
        if not open then return button end
        return ui.column { children = {
            button,
            ui.panel { id = "list", align = "center", width = 1000, height = 520, children = {
                ui.text { text = "A list that a script opened" },
            } },
        } }
    end,
    on = {
        toggle = function(_event, ctx)
            ctx.patch("state", "ui_state", { open = not ctx.get("state", "ui_state").open })
        end,
    },
}
