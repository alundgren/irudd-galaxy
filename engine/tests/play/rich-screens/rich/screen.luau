local function token(ui, value)
    return ui.drag_source { id = value, text = value == "small" and "Small token" or "Large token",
        value = value, height = 60 }
end

return {
    screen = function(ctx, ui)
        local state = ctx.get("state", "state")
        local rows = {}
        for n = 1, 16 do
            rows[n] = ui.button { id = "entry-" .. n, text = "Entry " .. n,
                on = "choose", payload = { number = n } }
        end
        local function home(value)
            local here = state[value] == ""
            return ui.column { height = 60, children = {
                here and token(ui, value) or ui.panel { height = 60, children = {} },
            } }
        end
        local function slot(id, label)
            local occupant = state.small == id and "small" or state.large == id and "large" or nil
            local function verdict(value)
                if occupant and occupant ~= value then return "Already occupied" end
                if id == "narrow" and value == "large" then return "Needs a wider slot" end
                return ""
            end
            return ui.drop_target { id = id, text = label, on = "place", payload = { slot = id },
                accepts = { small = verdict("small"), large = verdict("large") },
                children = { ui.column { height = 60, children = occupant and { token(ui, occupant) } or {} } } }
        end
        return ui.panel { width = 1200, children = {
            ui.text { text = "Grid, scrolling and drag targets", size = 28 },
            ui.text { text = "Selected: " .. state.chosen .. "    Drops: " .. state.drops },
            ui.row { children = {
                ui.scroll { id = "entries", width = 400, height = 430,
                    on = "scrolled", children = rows },
                ui.column { children = {
                    ui.row { children = {
                        ui.icon { icon = "token", width = 44 },
                        ui.delta { value = 3, width = 120 },
                        ui.delta { value = -12, width = 120 },
                        ui.text { text = "Drag the list handle." },
                    } },
                    ui.grid { columns = 2, children = {
                        home("small"), home("large"),
                        slot("narrow", "Narrow slot"), slot("wide", "Wide slot"),
                    } },
                    ui.text { text = "Pick up a token, then release over a slot. A placed token can be picked up again. F8 keeps a note." },
                } },
            } },
        } }
    end,
    on = {
        choose = function(event, ctx)
            ctx.patch("state", "state", { chosen = event.payload.number })
        end,
        place = function(event, ctx)
            local state = ctx.get("state", "state")
            local value, slot = event.payload.value, event.payload.slot
            if value ~= "small" and value ~= "large" then return end
            if slot ~= "narrow" and slot ~= "wide" then return end
            if value == "large" and slot == "narrow" then return end
            local other = value == "small" and "large" or "small"
            if state[other] == slot then return end
            ctx.patch("state", "state", { [value] = slot, drops = state.drops + 1 })
        end,
        scrolled = function(event, ctx)
            ctx.patch("state", "state", { offset = event.payload.offset })
        end,
    },
}
