-- Clickable figures, a gliding camera, Esc and right-click bound to back, and widgets
-- placed against the window's edges.
return {
    screen = function(ctx, ui)
        local here = ctx.get("player", "engine.location").place
        local choice = ctx.get("state", "choice")
        local inside = here == "targets.hall"
        local framed = if choice.picked == "" then "Nothing framed" else "Framed: " .. choice.picked
        local widgets = {
            ui.panel { id = "status", align = "top-right", x = 16, y = 16, width = 300, children = {
                ui.text { text = if inside then "In the hall" else "In the yard" },
                ui.text { text = framed },
                ui.text { text = "Back presses: " .. choice.backs },
            } },
            ui.panel { id = "keys", align = "bottom", y = 16, width = 480, children = {
                ui.text { text = "Esc or right-click goes back" },
            } },
            ui.text { id = "log", align = "bottom-left", x = 16, y = 16, width = 200,
                text = if inside then "Entered the hall" else "Outside" },
        }
        if inside then
            table.insert(widgets, ui.target { id = "first-target", anchor = "being.targets.first",
                on = "pick", payload = { who = "first" }, text = "First figure: click to frame" })
            table.insert(widgets, ui.target { id = "second-target", anchor = "being.targets.second",
                on = "pick", payload = { who = "second" } })
            -- A painted label over the second figure's face blocks clicks there.
            table.insert(widgets, ui.text { id = "cover", text = "Covered", width = 110,
                anchor = "being.targets.second.face", y = 14 })
        else
            table.insert(widgets, ui.button { id = "enter", align = "bottom-right", x = 16, y = 96, width = 260,
                text = "Enter the hall", on = "go" })
        end
        return ui.column { children = widgets }
    end,
    on = {
        go = function(_event, ctx)
            ctx.move("player", "targets.hall")
        end,
        pick = function(event, ctx)
            ctx.patch("state", "choice", { picked = event.payload.who })
            ctx.patch("targets.hall", "engine.room", { camera = "camera_" .. event.payload.who })
        end,
        back = function(event, ctx)
            if not event.payload.down then
                return
            end
            local choice = ctx.get("state", "choice")
            ctx.patch("state", "choice", { backs = choice.backs + 1 })
            if choice.picked ~= "" then
                ctx.patch("state", "choice", { picked = "" })
                ctx.patch("targets.hall", "engine.room", { camera = "" })
            elseif ctx.get("player", "engine.location").place == "targets.hall" then
                ctx.move("player", "targets.yard")
            end
        end,
    },
}
