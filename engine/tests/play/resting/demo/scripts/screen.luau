-- Two buttons. Each rests, with its reason, while it can't act: Add while the tray is
-- full, Empty while it is empty. Neither ever disappears.
local CAPACITY = 2

return {
    on = {
        add = function(_, ctx)
            local tray = ctx.get("state", "tray")
            ctx.patch("state", "tray", { count = tray.count + 1 })
        end,
        empty = function(_, ctx)
            ctx.patch("state", "tray", { count = 0 })
        end,
    },
    screen = function(ctx, ui)
        local tray = ctx.get("state", "tray")
        return ui.panel { id = "tray", width = 420, children = {
            ui.text { text = "Tray: " .. tray.count .. " of " .. CAPACITY },
            ui.row { children = {
                ui.button { id = "add", text = "Add one", on = "add",
                    resting = if tray.count >= CAPACITY then "The tray is full: empty it first." else nil },
                ui.button { id = "empty", text = "Empty", on = "empty",
                    resting = if tray.count == 0 then "Nothing to empty yet." else nil },
            } },
        } }
    end,
}
