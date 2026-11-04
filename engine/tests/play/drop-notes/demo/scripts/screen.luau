-- One token and two slots that both accept it. The far slot says what a drop there
-- changes, so the note shows at pickup, beside the plain `Drop here` of the near slot.
local function token(ui)
    return ui.drag_source { id = "token", text = "Token", value = "token", height = 60 }
end

return {
    on = {
        place = function(event, ctx)
            local slot = event.payload.slot
            if event.payload.value ~= "token" then return end
            if slot ~= "near" and slot ~= "far" then return end
            local tray = ctx.get("state", "tray")
            ctx.patch("state", "tray", { at = slot, drops = tray.drops + 1 })
        end,
    },
    screen = function(ctx, ui)
        local tray = ctx.get("state", "tray")
        local function holder(here)
            return ui.column { height = 60, children = if tray.at == here then { token(ui) } else {} }
        end
        local function slot(id, label, notes)
            return ui.drop_target { id = id, text = label, on = "place", payload = { slot = id },
                accepts = { token = "" }, notes = notes, children = { holder(id) } }
        end
        return ui.panel { id = "tray", width = 760, children = {
            ui.text { text = "Drops: " .. tray.drops },
            ui.row { children = {
                ui.column { width = 200, children = { holder("home") } },
                slot("near", "Near slot", nil),
                slot("far", "Far slot", { token = "Farther away: 2 more steps" }),
            } },
        } }
    end,
}
