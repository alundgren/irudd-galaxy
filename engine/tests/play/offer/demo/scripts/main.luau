-- Records each answer the host's offer gets. A declined offer is disabled, so the host
-- falls back to its repeat talk and never offers again.
return {
    screen = function(ctx, ui)
        local a = ctx.get("state", "answers")
        local children = {
            ui.text { text = "An offer", size = 26 },
            ui.text { text = "Click the host, then answer in the bubble." },
            ui.button { id = "answers", text = if a.open then "Hide answers" else "Show answers", on = "answers" },
        }
        if a.open then
            table.insert(children, ui.text { id = "last", text = "Last answer: " .. (if a.last == "" then "none" else a.last) })
            table.insert(children, ui.text { text = "Lamp taken: " .. (if a.taken then "yes" else "no") })
            table.insert(children, ui.text { text = "Offer declined: " .. (if a.declined then "yes" else "no") })
        end
        return ui.panel { id = "panel", align = "top-right", x = 20, y = 20, width = 300, children = children }
    end,
    on = {
        answers = function(_event, ctx)
            ctx.patch("state", "answers", { open = not ctx.get("state", "answers").open })
        end,
        ["engine.talk_chosen"] = function(event, ctx)
            local choice = event.payload.choice
            local text = ctx.get(choice, "engine.talk_choice").text
            if choice == "offer.take" then
                ctx.patch("state", "answers", { last = text, taken = true })
            else
                ctx.patch("state", "answers", { last = text, declined = true })
                ctx.patch("gift", "engine.talk", { enabled = false })
            end
        end,
    },
}
