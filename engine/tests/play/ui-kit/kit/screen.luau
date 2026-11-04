-- A generic screen kit: named looks, image icons, text spans and UI motion.
return {
    screen = function(ctx, ui)
        local w = ctx.get("wallet", "wallet")
        local chip = ui.panel { id = "chip", look = "chip", align = "top-right", x = 20, y = 20,
            width = 320, children = {
                ui.row { gap = 8, children = {
                    ui.icon { icon = "coin", size = 28, width = 28 },
                    ui.text { text = "Credits", width = 110 },
                    ui.delta { id = "credits", value = w.credits, cue = "count", width = 120 },
                } },
            } }
        local main = ui.panel { id = "main", look = "parchment", width = 600, children = {
            ui.text { text = "Screen kit", size = 28 },
            ui.text { spans = { { text = "Odo", look = "name" }, " has fresh ice, cheap today." } },
            ui.row { gap = 8, children = {
                ui.icon { icon = "up", look = "gain", size = 20, width = 20 },
                ui.text { spans = { { text = "•", look = "gain" }, " +" .. w.last .. " credits" },
                    width = 220 },
                ui.text { text = "CONFIRMED", look = "ticket", size = 16 },
            } },
            ui.text { id = "earned", text = "Earnings: " .. w.earned, cue = "flash" },
            ui.row { children = {
                ui.button { id = "earn", text = "Earn 73", on = "earn" },
                ui.button { id = "log", icon = "book", icon_at = "top", text = "Log",
                    on = "shelf", hover = "Open the log" },
                ui.button { id = "shelf", text = w.shelf and "Close shelf" or "Open shelf",
                    on = "shelf" },
            } },
        } }
        local children = { chip, main }
        if w.shelf then
            children[3] = ui.panel { id = "shelf-panel", look = "parchment", align = "bottom-left",
                x = 20, y = 20, width = 420, children = {
                    ui.text { icon = "book", text = "Ship log" },
                    ui.text { text = "Earnings so far: " .. w.earned .. "." },
                } }
        end
        return ui.column { children = children }
    end,
    on = {
        earn = function(_, ctx)
            local w = ctx.get("wallet", "wallet")
            ctx.patch("wallet", "wallet", { credits = w.credits + 73, last = 73, earned = w.earned + 1 })
        end,
        shelf = function(_, ctx)
            local w = ctx.get("wallet", "wallet")
            ctx.patch("wallet", "wallet", { shelf = not w.shelf })
        end,
    },
}
