-- Every UI action with a sound: buttons to hover and press, one wearing a look with its
-- own press, a resting button that denies, a list to sweep the pointer down, a token to
-- drag onto a slot that takes it or one that refuses it, and a shelf that opens and closes.
local ROWS = { "Ash", "Birch", "Cedar", "Elm", "Fir", "Oak" }

local function token(ui)
    return ui.drag_source { id = "token", text = "Token", value = "token", height = 60 }
end

return {
    on = {
        toggle = function(_, ctx)
            local desk = ctx.get("state", "desk")
            ctx.patch("state", "desk", { shelf = not desk.shelf })
        end,
        pick = function(event, ctx)
            ctx.patch("state", "desk", { picked = event.payload.name })
        end,
        place = function(event, ctx)
            if event.payload.value ~= "token" or event.payload.slot ~= "wide" then return end
            local desk = ctx.get("state", "desk")
            ctx.patch("state", "desk", { at = "wide", drops = desk.drops + 1 })
        end,
        locked = function() end,
    },
    screen = function(ctx, ui)
        local desk = ctx.get("state", "desk")
        local rows = {}
        for i, name in ROWS do
            rows[i] = ui.button { id = "row-" .. string.lower(name), text = name, on = "pick",
                payload = { name = name } }
        end
        local function holder(here)
            return ui.column { height = 60, children = if desk.at == here then { token(ui) } else {} }
        end
        local menu = ui.panel { id = "menu", width = 760, children = {
            ui.text { text = "Picked: " .. (if desk.picked == "" then "nothing" else desk.picked)
                .. ". Drops: " .. desk.drops },
            ui.row { children = {
                ui.button { id = "shelf", text = if desk.shelf then "Close shelf" else "Open shelf",
                    on = "toggle" },
                ui.button { id = "chip", look = "chip", text = "Chip", on = "pick",
                    payload = { name = "Chip" } },
                ui.button { id = "locked", text = "Locked", on = "locked",
                    resting = "This one never opens." },
            } },
            ui.row { children = {
                ui.column { width = 200, children = rows },
                ui.column { width = 160, children = { holder("home") } },
                ui.drop_target { id = "wide", text = "Wide slot", on = "place",
                    payload = { slot = "wide" }, accepts = { token = "" },
                    children = { holder("wide") } },
                ui.drop_target { id = "narrow", text = "Narrow slot", on = "place",
                    payload = { slot = "narrow" }, accepts = { token = "Too narrow for the token" },
                    children = { holder("narrow") } },
            } },
        } }
        if not desk.shelf then
            return menu
        end
        return ui.column { children = {
            menu,
            ui.panel { id = "shelf-panel", align = "top-right", x = 20, y = 20, width = 300,
                children = { ui.text { text = "The shelf is open." } } },
        } }
    end,
}
