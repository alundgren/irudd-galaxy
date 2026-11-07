-- Catch rides on the moving marker. Spend rests until something was caught, and each
-- spend uses one up. Space hides and shows the screen. Both keep to the lower part of the
-- screen, clear of the other play fixtures loaded beside this one.
return {
    on = {
        catch = function(_, ctx)
            local t = ctx.get("state", "tally")
            ctx.patch("state", "tally", { caught = t.caught + 1 })
        end,
        spend = function(_, ctx)
            local t = ctx.get("state", "tally")
            ctx.patch("state", "tally", { spent = t.spent + 1 })
        end,
        toggle = function(event, ctx)
            if not event.payload.down then return end
            local t = ctx.get("state", "tally")
            ctx.patch("state", "tally", { open = not t.open })
        end,
    },
    screen = function(ctx, ui)
        local t = ctx.get("state", "tally")
        if not t.open then return nil end
        return ui.panel { id = "desk", align = "bottom-right", x = 16, y = 16, width = 320, children = {
            ui.text { text = "Caught " .. t.caught .. ", spent " .. t.spent },
            ui.button { id = "spend", text = "Spend one", on = "spend",
                resting = if t.spent >= t.caught then "Nothing caught to spend." else nil },
            ui.panel { id = "tag", width = 160, anchor = "marker#tag", children = {
                ui.button { id = "catch", text = "Catch", on = "catch" },
            } },
        } }
    end,
}
