-- Every look and icon of the core's kit, built only through kit.luau.
local kit = require("./kit")

local ICONS = { "book", "credits", "hold", "ore", "plate", "rations", "power", "mass", "dock", "radio",
    "back", "prev", "next", "switch", "lens", "pods", "thrusters", "controls", "warning", "up", "down",
    "check", "ship", "station", "pin" }

local function icons(ui)
    local cells = {}
    for _, name in ICONS do
        table.insert(cells, ui.column { gap = 2, children = {
            kit.icon(ui, name, { size = 32 }),
            ui.text { text = name, size = 16, width = 70 },
        } })
    end
    return kit.place(ui, { id = "icons", align = "top-right", x = 16, y = 16, width = 400, gap = 8, children = {
        kit.heading(ui, { text = "Icons", size = 22 }),
        ui.grid { columns = 5, gap = 2, children = cells },
        ui.text { text = "On dark:", size = 16 },
        kit.chip(ui, { width = 368, children = { ui.row { gap = 10, children = {
            kit.icon(ui, "credits", { size = 24 }), kit.icon(ui, "hold", { size = 24 }), kit.icon(ui, "ore", { size = 24 }),
            kit.icon(ui, "plate", { size = 24 }), kit.icon(ui, "rations", { size = 24 }), kit.icon(ui, "book", { size = 24 }),
            kit.icon(ui, "radio", { size = 24 }), kit.icon(ui, "power", { size = 24 }), kit.icon(ui, "mass", { size = 24 }),
        } } } }),
    } })
end

local function status(ui)
    return ui.column { gap = 10, children = {
        kit.chip(ui, { width = 220, gap = 4, children = {
            ui.text { icon = "credits", text = "173 cr", size = 22 },
            ui.text { icon = "hold", text = "Hold 4 / 6", size = 18 },
            ui.row { gap = 10, children = {
                ui.text { icon = "ore", text = "3 ore", size = 16 },
                ui.text { icon = "plate", text = "1 plate", size = 16 },
            } },
        } }),
        kit.pill(ui, { width = 300, children = { ui.text { text = "E · dock at the nearest station", size = 16 } } }),
        ui.panel { look = kit.LOOK.radio, width = 300, gap = 4, children = {
            ui.text { icon = "radio", text = "Lantern Dock radio", size = 16 },
            ui.text { spans = { { text = "||", look = "lit" }, { text = "|", look = "unlit" }, "  Fair" }, size = 16 },
        } },
        ui.row { gap = 8, children = {
            kit.label(ui, { text = "Little Mercy", size = 16 }),
            ui.text { text = "Bram Okafor", look = kit.LOOK.sign, size = 16 },
        } },
    } }
end

local function place(ui)
    local function row(ware, price, stock, hold, verbs)
        local children = {
            ui.text { icon = kit.ware(ware), text = ware, width = 104, size = 18 },
            ui.text { text = price, width = 84, size = 18 },
            ui.text { text = stock, width = 56, size = 18 },
            ui.text { text = hold, width = 44, size = 18 },
        }
        return ui.row { gap = 8, children = children }
    end
    return kit.place(ui, { width = 460, gap = 10, children = {
        kit.heading(ui, { text = "Exchange" }),
        ui.text { spans = kit.name("Tova Reyes", "Exchange keeper"), size = 18 },
        kit.sheet(ui, { width = 428, gap = 6, children = {
            kit.heads(ui, { { "Ware", 104 }, { "Price each", 84 }, { "Stock", 56 }, { "Hold", 44 } }),
            row("ore", "pays 16", "12", "4"),
            ui.row { gap = 8, children = {
                kit.button(ui, { text = "Sell 4 for 64 cr", width = 172, size = 16, on = "press" }, "primary"),
                kit.button(ui, { text = "Back", icon = "back", width = 104, size = 16, on = "press" }, "secondary"),
                kit.button(ui, { text = "Buy 1", width = 88, size = 16, on = "press", resting = "Hold full: sell first." }),
            } },
            row("plate", "asks 22", "3", "0"),
        } }),
        kit.warning(ui, { text = "Over budget: the lens needs 2 power; 1 is free.", width = 428, size = 16 }),
        ui.row { gap = 10, children = {
            ui.text { spans = kit.amount(64, "cr"), size = 18, width = 72 },
            ui.text { spans = kit.amount(-90, "cr"), size = 18, width = 72 },
            kit.icon(ui, "up", { look = kit.LOOK.gain }),
            kit.icon(ui, "down", { look = kit.LOOK.loss }),
            kit.tag(ui, "JOB", { width = 48 }), kit.tag(ui, "OFFER", { width = 70 }), kit.tag(ui, "VISIT", { width = 60 }),
        } },
    } })
end

local function tickets(ui)
    return ui.column { gap = 10, children = {
        kit.ticket(ui, { width = 300, children = {
            ui.text { text = "Sold 4 ore at Lantern Dock", size = 16 },
            ui.text { spans = kit.amount(64, "cr", true), size = 18 },
        } }),
        kit.ticket(ui, { width = 300, children = {
            ui.text { text = "The galley is open at Lantern Dock.", size = 16 },
        } }, "NOTICE"),
        kit.bubble(ui, { width = 300, gap = 6, children = {
            ui.text { text = "Ines Varga", look = kit.LOOK.name_tag, size = 20 },
            ui.text { text = "Thanks again for the plate.", size = 18 },
        } }),
        kit.button(ui, { spans = { "Log  ", { text = "3 new", look = "new" } }, icon = "book", icon_at = "top",
            width = 132, height = 64, pad = 6, size = 16, on = "press" }),
    } }
end

return {
    screen = function(_, ui)
        return ui.column { children = {
            ui.row { align = "top-left", x = 16, y = 16, width = 776, gap = 16, children = {
                ui.column { gap = 16, children = { status(ui), tickets(ui) } },
                place(ui),
            } },
            icons(ui),
        } }
    end,
}
