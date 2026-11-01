return {
    screen = function(ctx, ui)
        local state = ctx.get("state", "state")
        local rows = {}
        for n = 1, 16 do
            rows[n] = ui.button { id = "entry-" .. n, text = "Entry " .. n,
                on = "choose", payload = { number = n } }
        end
        return ui.panel { width = 1200, children = {
            ui.text { text = "Grid, scrolling and drag targets", size = 28 },
            ui.text { text = "Selected: " .. state.chosen .. "    Drops: " .. state.drops },
            ui.row { children = {
                ui.scroll { id = "entries", width = 400, height = 390,
                    on = "scrolled", children = rows },
                ui.column { children = {
                    ui.row { children = {
                        ui.icon { icon = "token", width = 44 },
                        ui.delta { value = 3, width = 120 },
                        ui.delta { value = -12, width = 120 },
                        ui.text { text = "Wheel over the list." },
                    } },
                    ui.grid { columns = 2, children = {
                        ui.drag_source { id = "small", text = "Small token", value = "small", pad = 16 },
                        ui.drag_source { id = "large", text = "Large token", value = "large", pad = 16 },
                        ui.drop_target { id = "narrow", text = "Narrow slot", on = "place", pad = 16,
                            accepts = { small = "", large = "Needs a wider slot" } },
                        ui.drop_target { id = "wide", text = "Wide slot", on = "place", pad = 16,
                            accepts = { small = "", large = "" } },
                    } },
                    ui.text { text = "Pick up a token to see every verdict. Release over a green slot to place it. F8 keeps a note, including an unfinished drag." },
                } },
            } },
        } }
    end,
    on = {
        choose = function(event, ctx)
            ctx.patch("state", "state", { chosen = event.payload.number })
        end,
        place = function(_, ctx)
            local state = ctx.get("state", "state")
            ctx.patch("state", "state", { drops = state.drops + 1 })
        end,
        scrolled = function(event, ctx)
            ctx.patch("state", "state", { offset = event.payload.offset })
        end,
    },
}
