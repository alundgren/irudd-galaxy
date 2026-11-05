-- Docked: the station up close. Its buildings are the menu; a room opens as a panel
-- beside its building, and the station key along the bottom reaches every room.
local rules = require("./rules")
local log = require("./log")
local teach = require("./teach")
local voices = require("./voices")
local signs = require("./signs")
local trade = require("./trade")
local fit = require("./fit")
local cradle = require("./cradle")
local undock = require("./launch")
local kit = require("./kit")
local layout = require("./layout") -- room panels are layout.PANEL wide: at most about 45% of the view

local function pressed(event)
    return event.payload.down ~= false
end

-- The station the ship is docked at, or nil: a place with a port is a station's dock.
local function docked(ctx)
    local pilot = ctx.get("ship", "pilot")
    local here = ctx.get("ship", "engine.location").place
    if here ~= "" and ctx.get(here, "port") then return here, pilot end
    return nil, pilot
end

-- The station's buildings, in key order.
local function facilities(ctx, station)
    local list = {}
    for _, id in ctx.query("facility") do
        if ctx.get(id, "facility").station == station then table.insert(list, id) end
    end
    table.sort(list, function(a, b) return ctx.get(a, "facility").order < ctx.get(b, "facility").order end)
    return list
end

local function short(id)
    return string.match(id, "_(%w+)$") or id
end

-- A room still being built trades nothing yet.
local function trading(ctx, room)
    local site = ctx.get(room, "build")
    return not (site and not site.done)
end

local function open(event, ctx)
    if not pressed(event) then return end
    local station, pilot = docked(ctx)
    if not station then return end
    local id = event.payload.facility or ctx.get(station, "port").exchange
    local facility = ctx.get(id, "facility")
    if not facility or facility.station ~= station or pilot.room == id then return end
    signs.hush(ctx) -- the bubble would ride the glide in over the room's panel
    ctx.patch("ship", "pilot", { room = id, sale = 0, deal = "", log_open = false, gliding = true })
    -- The view glides to frame the building, beside its room's panel.
    ctx.patch(station, "engine.room", { camera = facility.camera })
    signs.show(ctx, station, id, false, nil, true)
end

-- Back (B, Esc, right-click or the Back button): from a room to the whole station,
-- or shut the open log.
local function close(event, ctx)
    if not pressed(event) then return end
    local station, pilot = docked(ctx)
    if station and pilot.room ~= "" then
        signs.hush(ctx) -- the bubble would ride the glide out over the HUD
        ctx.patch("ship", "pilot", { room = "", sale = 0, deal = "", gliding = true })
        ctx.patch(station, "engine.room", { camera = "" })
        signs.show(ctx, station, "", false, nil, true)
    elseif pilot.log_open then
        ctx.patch("ship", "pilot", { log_open = false })
        if station then signs.show(ctx, station, "", false, nil) end
    end
end

local function launch(event, ctx)
    if not pressed(event) then return end
    local station = docked(ctx)
    if station then undock.launch(ctx, station) end
end

-- The site becomes its room: the finished building, its keeper arriving to stand by
-- it, and the goods they trade.
local function build_up(ctx, site, build)
    local station = build.station
    ctx.patch(site, "engine.model", { model = build.model })
    ctx.patch(site, "facility", { name = build.name, keeper = build.keeper, offer = build.offer })
    local keeper = build.keeper
    local act = keeper .. "_act"
    local plan = ctx.get(act, "engine.act")
    ctx.put(keeper, "engine.being", {})
    ctx.put(keeper, "engine.location", { place = station, spot = plan.spot })
    ctx.put(keeper, "engine.position", { angle = plan.angle })
    ctx.put(keeper, "engine.model", { model = "core.keeper", look = "core.prop", scale = 1.5 })
    -- The sale that builds it happens in another room, where only that room's keeper
    -- wears a sign; the new keeper's sign goes up when the player is back on the
    -- station (signs.show).
    ctx.put(keeper, "engine.life", { clock = "core.day", first = act, name = "" })
    -- Their thanks for the building goes on offer once the introduction is heard; the
    -- sale that built it left room in the hold.
    for _, id in ctx.query("gift") do
        local gift = ctx.get(id, "gift")
        if ctx.get(id, "engine.talk").speaker == keeper and not gift.answered then
            ctx.patch(id, "engine.talk", { enabled = true })
            ctx.patch(gift.instead, "engine.talk", { enabled = false })
        end
    end
end

-- Sells the hold's `ware` (or everything this room buys) at the room's posted prices.
local function sell(event, ctx)
    if not pressed(event) then return end
    local station, pilot = docked(ctx)
    if not station or pilot.room == "" or not trading(ctx, pilot.room) then return end
    local port = ctx.get(station, "port")
    local hold = ctx.get("ship", "hold")
    local site, build = trade.building(ctx, station)
    local earned, sold, count, into, grew = 0, {}, 0, 0, false
    for _, id in trade.lines(ctx, pilot.room) do
        local line = ctx.get(id, "market")
        local units = hold[line.ware]
        if line.role == "buys" and units > 0 and (event.payload.ware == nil or event.payload.ware == line.ware) then
            -- Goods a station is building with go into the build, at a steady price,
            -- until it is done; the rest go onto its stock.
            local building = 0
            if build and build.ware == line.ware then
                building = math.min(units, build.need - build.given)
                into = building
                earned += building * rules.price(line)
                grew = build.given + building >= build.need
                ctx.patch(site, "build", { given = build.given + building, done = grew })
            end
            local prices, stock = rules.walk(line, units - building)
            earned += rules.total(prices)
            ctx.patch(id, "market", { stock = stock })
            if line.ware == "ore" then
                ctx.patch(station .. "_stock", "engine.model", { scale = trade.pile(stock) })
            end
            table.insert(sold, units .. " " .. line.ware)
            count += units
            hold[line.ware] = 0
        end
    end
    if #sold == 0 then return end
    local what = table.concat(sold, " and ")
    local text = "Sold " .. what .. " to " .. port.name .. " for " .. earned .. " cr."
    if grew then
        build_up(ctx, site, build)
        text ..= " The " .. string.lower(build.name) .. " is built: " .. ctx.get(build.keeper, "keeper").name .. " " .. build.offer .. " there."
        ctx.record("grown", { station = station, site = site })
    elseif into > 0 then
        text ..= " " .. into .. " " .. build.ware .. " went into the " .. string.lower(build.name) .. ": "
            .. (build.given + into) .. " of " .. build.need .. "."
    end
    ctx.put("ship", "hold", hold)
    local deal = "Sold " .. what .. "." .. (grew and " The " .. string.lower(build.name) .. " is built!" or "")
    ctx.patch("ship", "pilot", { credits = pilot.credits + earned, sale = earned, deal = deal })
    voices.refresh(ctx, station, hold, earned, build and build.need - build.given - into or 0)
    log.add(ctx, text)
    ctx.record("sold", { units = count, credits = earned, station = station })
end

-- Buys `units` of `ware` (0 for as many as fit and are affordable) from the room.
local function buy(event, ctx)
    if not pressed(event) then return end
    local station, pilot = docked(ctx)
    if not station or pilot.room == "" or not trading(ctx, pilot.room) then return end
    local hold = ctx.get("ship", "hold")
    for _, id in trade.lines(ctx, pilot.room) do
        local line = ctx.get(id, "market")
        if line.role == "sells" and line.ware == event.payload.ware then
            local units = rules.affordable(line, fit.hold(ctx) - rules.load(hold), pilot.credits)
            local asked = event.payload.units or 0
            if asked > 0 then units = math.min(units, asked) end
            if units == 0 then return end -- resting: the hold is full, or credits are short
            local prices, stock = rules.walk(line, units)
            local spent = rules.total(prices)
            hold[line.ware] += units
            ctx.patch(id, "market", { stock = stock })
            ctx.put("ship", "hold", hold)
            local what = units .. " " .. line.ware
            ctx.patch("ship", "pilot", { credits = pilot.credits - spent, sale = -spent, deal = "Bought " .. what .. "." })
            local port = ctx.get(station, "port")
            local _, build = trade.building(ctx, station)
            voices.refresh(ctx, station, hold, 0, build and build.need - build.given or 0)
            log.add(ctx, "Bought " .. what .. " at " .. port.name .. " for " .. spent .. " cr.")
            ctx.record("bought", { ware = line.ware, units = units, credits = spent, station = station })
            return
        end
    end
end

-- The view has settled at a station: arrived, or framed a building. Signs go back up.
local function settled(event, ctx)
    local place = event.payload.place
    if event.payload.entity ~= "core.ship" or not ctx.get(place, "port") then return end
    local pilot = ctx.get("ship", "pilot")
    if not pilot.gliding then return end
    ctx.patch("ship", "pilot", { gliding = false })
    signs.show(ctx, place, pilot.room, pilot.log_open, signs.speaking(ctx), false)
end

-- The skiffs ferry crates across their deck, back and forth, without end.
local function skiffs(ctx)
    for _, id in ctx.query("skiff") do
        local walk = ctx.get(id, "engine.walk")
        if not walk or not walk.active then
            local route = ctx.get(id, "skiff")
            local at = ctx.get(id, "engine.location").spot
            ctx.walk(id, at == route.a and route.b or route.a, 1.5)
        end
    end
end

-- The trade table (direction doc 1.5): one sheet, its column heads, then a row per ware,
-- its name with its icon, the price and the counts as plain labels, then its verbs under
-- it. Only verbs are buttons: selling is the gold primary, buying the quieter parchment.
-- The columns fit the sheet: INNER wide inside the panel's padding, SHEET inside the
-- sheet's own.
local INNER = layout.ROOM - 32
local SHEET = INNER - 16
local COLUMNS = { { "Ware", 120 }, { "Price each", 104 }, { "Stock", 64 }, { "Hold", 54 } }

local function table_row(ui, id, ware, name, price, stock, held)
    return ui.row { id = id, gap = 8, children = {
        ui.text { width = COLUMNS[1][2], icon = kit.ware(ware), text = name, size = kit.SIZE.body },
        ui.text { width = COLUMNS[2][2], size = kit.SIZE.body, spans = price },
        ui.text { width = COLUMNS[3][2], size = kit.SIZE.body, text = stock, cue = "flash" },
        ui.text { width = COLUMNS[4][2], size = kit.SIZE.body, text = held, cue = "flash" },
    } }
end

-- "pays 17 cr": the word quiet, the figure heavy.
local function price_spans(word, price)
    return { word .. " ", { text = price .. " cr", look = kit.LOOK.figure } }
end

-- One good in a trading room: its row, then its verbs.
local function line_rows(ctx, ui, id, hold, capacity, credits, rows, sellable, build)
    local line = ctx.get(id, "market")
    local name = string.upper(string.sub(line.ware, 1, 1)) .. string.sub(line.ware, 2)
    local price = rules.price(line)
    local w = line.ware
    local inside = rows
    if line.role == "buys" then
        local building = build and build.ware == w
        table.insert(inside, table_row(ui, "about_" .. w, w, name, price_spans("pays", price),
            building and "-" or tostring(line.stock), tostring(hold[w])))
        if building then
            table.insert(inside, ui.text { id = "builds_" .. w, width = SHEET, size = kit.SIZE.small, spans = {
                "Builds the " .. string.lower(build.name) .. ": ",
                { text = build.given .. " of " .. build.need, look = kit.LOOK.figure }, " in" } })
        end
        if hold[w] > 0 then
            local units = hold[w]
            local building = build and build.ware == w and math.min(units, build.need - build.given) or 0
            local value = building * price + rules.total((rules.walk(line, units - building)))
            local key = sellable == 1 and " · Enter" or ""
            table.insert(inside, kit.button(ui, { id = "sell_" .. w, width = SHEET, on = "sell", payload = { ware = w },
                icon = kit.ware(w), text = "Sell " .. units .. " " .. w .. " for " .. value .. " cr" .. key }, "primary"))
        end
    else
        table.insert(inside, table_row(ui, "about_" .. w, w, name, price_spans("asks", price), tostring(line.stock), tostring(hold[w])))
        local room = capacity - rules.load(hold)
        local most = rules.affordable(line, room, credits)
        -- Buying that can't happen now rests in place, with its reason beside it.
        local why = nil
        if room == 0 then
            why = "Hold full: sell first."
        elseif line.stock == 0 then
            why = "Sold out until the stock comes back."
        elseif most == 0 then
            why = "Not enough credits for one " .. w .. "."
        end
        if why then
            table.insert(inside, ui.row { id = "buy_" .. w, gap = 8, children = {
                kit.button(ui, { id = "buy1_" .. w, width = 150, on = "buy", payload = { ware = w, units = 1 },
                    text = "Buy 1 · " .. price .. " cr", resting = why }, "secondary"),
                ui.text { id = "why_" .. w, width = SHEET - 158, text = why, size = kit.SIZE.small },
            } })
        else
            local buttons = { kit.button(ui, { id = "buy1_" .. w, width = 150, on = "buy", payload = { ware = w, units = 1 },
                text = "Buy 1 · " .. price .. " cr" }, "secondary") }
            if most > 1 then
                table.insert(buttons, kit.button(ui, { id = "buymax_" .. w, width = SHEET - 158, on = "buy", payload = { ware = w, units = 0 },
                    text = "Buy " .. most .. " · " .. rules.total((rules.walk(line, most))) .. " cr" }, "secondary"))
            end
            table.insert(inside, ui.row { id = "buy_" .. w, gap = 8, children = buttons })
        end
    end
end

-- The receipt for the last deal: a dark ticket, what changed hands, and the amount.
local function receipt(ui, pilot)
    local spans = { pilot.deal .. (pilot.sale >= 0 and " Paid " or " Spent ") }
    for _, span in kit.amount(pilot.sale, "cr", true) do table.insert(spans, span) end
    return kit.ticket(ui, { id = "receipt", width = INNER, children = {
        ui.text { id = "deal", width = INNER - 20, size = kit.SIZE.body, spans = spans, cue = "flash" },
    } })
end

local function trade_rows(ctx, ui, station, pilot, children)
    local hold = ctx.get("ship", "hold")
    local _, build = trade.building(ctx, station)
    local lines = trade.lines(ctx, pilot.room)
    local sellable = 0
    for _, id in lines do
        local line = ctx.get(id, "market")
        if line.role == "buys" and hold[line.ware] > 0 then sellable += 1 end
    end
    local capacity = fit.hold(ctx)
    -- What you bring to the table: credits and the hold, as the status chip shows them.
    table.insert(children, ui.row { id = "ledger", gap = 8, children = {
        ui.text { id = "ledger_credits", width = 110, icon = "credits", size = kit.SIZE.body, cue = "flash",
            spans = { { text = pilot.credits .. " cr", look = kit.LOOK.figure } } },
        ui.text { id = "ledger_hold", width = INNER - 118, icon = "hold", size = kit.SIZE.body, cue = "flash",
            spans = { { text = rules.load(hold) .. "/" .. capacity, look = kit.LOOK.figure }, " aboard: " .. rules.list(hold) } },
    } })
    local rows = { kit.heads(ui, COLUMNS, { id = "columns" }) }
    for _, id in lines do
        line_rows(ctx, ui, id, hold, capacity, pilot.credits, rows, sellable, build)
    end
    table.insert(children, kit.sheet(ui, { id = "wares", width = INNER, gap = 6, children = rows }))
    if sellable > 1 then
        table.insert(children, kit.button(ui, { id = "sell", width = INNER, on = "sell", text = "Sell everything here · Enter" }, "primary"))
    end
    if pilot.deal ~= "" then table.insert(children, receipt(ui, pilot)) end
end

-- The office: every station's posted prices on its own sheet, then how the builds and
-- the field stand.
local function office_rows(ctx, ui, station, children)
    table.insert(children, ui.text { id = "prices", size = kit.SIZE.small, look = kit.LOOK.column, text = "Posted prices" })
    for _, id in ctx.query("port") do
        local name = ctx.get(id, "port").name
        local posted = string.sub(trade.posted(ctx, id), #name + 2)
        table.insert(children, kit.sheet(ui, { id = "posted_" .. short(id), width = INNER, gap = 2, children = {
            ui.text { width = SHEET, size = kit.SIZE.body, spans = kit.name(name, id == station and "here" or nil) },
            ui.text { width = SHEET, size = kit.SIZE.small, text = posted },
        } }))
    end
    for _, id in ctx.query("build") do
        local build = ctx.get(id, "build")
        local where = ctx.get(build.station, "port").name
        local text = build.done and where .. "'s " .. string.lower(build.name) .. " is open."
            or where .. " is building a " .. string.lower(build.name) .. ": " .. build.given .. " of " .. build.need .. " " .. build.ware .. " in."
        table.insert(children, ui.text { id = "build_" .. short(id), width = INNER, text = text, size = kit.SIZE.body })
    end
    local left, total = 0, 0
    for _, id in ctx.query("ore") do
        local ore = ctx.get(id, "ore")
        if ore.ware == "ore" or ore.ware == "" then left, total = left + ore.left, total + ore.total end
    end
    table.insert(children, ui.text { id = "field", width = INNER, icon = "ore", size = kit.SIZE.body,
        spans = { "Ore left in the field: ", { text = left .. " of " .. total, look = kit.LOOK.figure } } })
end

local function site_rows(ui, build, children)
    table.insert(children, ui.text { id = "plan", width = INNER, size = kit.SIZE.body, text = "A " .. string.lower(build.name)
        .. " goes up here once the station has the " .. build.ware .. " for it." })
    table.insert(children, ui.text { id = "progress", size = 22, icon = kit.ware(build.ware), cue = "flash",
        spans = { { text = build.given .. " of " .. build.need, look = kit.LOOK.figure }, " " .. build.ware .. " in" } })
    table.insert(children, ui.text { id = "how", width = INNER, size = kit.SIZE.small, text = "Plate sold at the exchange builds it. When it opens, a cook will sell rations here." })
end

local function room_panel(ctx, ui, station, pilot)
    local facility = ctx.get(pilot.room, "facility")
    local at = ctx.get(pilot.room, "engine.position")
    local site = ctx.get(pilot.room, "build")
    local keeper_text, keeper_spans = nil, nil
    if facility.keeper ~= "" then
        local keeper = ctx.get(facility.keeper, "keeper")
        local first = string.match(keeper.name, "^%S+")
        local rest = "keeps it." .. (teach.new(ctx.get("ship", "taught"), "talk") and " Click " .. first .. " to talk." or "")
        keeper_text = keeper.name .. " " .. rest
        keeper_spans = kit.name(keeper.name, rest)
    end
    local children
    if facility.fits then
        -- The fitting room has the most to show, so it packs its own rows.
        children = cradle.rows(ctx, ui, pilot, facility.name, keeper_text)
    else
        -- The title with Back beside it, so the room's own rows start high.
        children = { ui.row { id = "heading", gap = 8, children = {
            kit.heading(ui, { id = "title", width = INNER - 148, text = facility.name }),
            kit.button(ui, { id = "close", width = 140, size = kit.SIZE.small, icon = "back", text = "Back · B", on = "close",
                hover = "Back to the whole station: B, Esc or a right-click" }, "secondary"),
        } } }
        if keeper_spans then table.insert(children, ui.text { id = "keeper", width = INNER, size = kit.SIZE.small, spans = keeper_spans }) end
        if site and not site.done then
            site_rows(ui, site, children)
        elseif #trade.lines(ctx, pilot.room) > 0 then
            trade_rows(ctx, ui, station, pilot, children)
        else
            office_rows(ctx, ui, station, children)
        end
    end
    -- The panel opens on the side away from its building, so the building stays in view.
    return layout.frame(ui, "room", {
        kit.place(ui, { id = "room", align = at.x < 0 and "top-right" or "top-left", x = layout.M, y = layout.M,
            width = layout.ROOM, gap = facility.fits and 4 or 8, children = children }),
    })
end

-- What a building's sign says on hover: its keeper and offer, or how its build stands.
local function about(ctx, id)
    local facility = ctx.get(id, "facility")
    local site = ctx.get(id, "build")
    if site and not site.done then
        return facility.name .. " · " .. site.given .. " of " .. site.need .. " " .. site.ware .. " in"
    end
    return ctx.get(facility.keeper, "keeper").name .. " · " .. facility.offer
end

-- The station key's icon for a room: what kind of room it is.
local function key_icon(ctx, id, facility)
    if facility.fits then return "ship" end
    local site = ctx.get(id, "build")
    if site and not site.done then return "station" end
    if #trade.lines(ctx, id) > 0 then return "credits" end
    return "pin"
end

-- The docked HUD along the bottom: the log's book (logbook.luau) bottom left, the key
-- centred and Launch bottom right. KEY wide icons, KEY_GAP apart, and a LAUNCH wide
-- button leave the key clear of both at 960x540 with three rooms (direction doc 3.1).
local KEY, KEY_GAP, LAUNCH = 160, 10, 176

local function overview(ctx, ui, station, pilot)
    local port = ctx.get(station, "port")
    local children = {
        -- The station's plate: its name, who holds it, and plainly that you are docked
        -- there. No faction holds a core station yet, so each says so (direction doc 3.1).
        -- Dark ink and compact, so it reads as a name plate and leaves the station clear.
        kit.plate(ui, { id = "plate", align = "top-left", x = layout.M, y = layout.M, width = 240, gap = 0, children = {
            ui.text { id = "station", text = port.name, size = 24, look = kit.LOOK.plate_name },
            ui.text { id = "owner", icon = "dock", text = "Unclaimed · docked here", size = kit.SIZE.small },
        } }),
    }
    local key = {}
    local talking = signs.speaking(ctx) ~= nil
    for _, id in facilities(ctx, station) do
        local facility = ctx.get(id, "facility")
        local text = about(ctx, id)
        -- Signs rest while the view glides or someone speaks, so they never crowd the
        -- moving view or a bubble.
        if not pilot.gliding and not talking then
            table.insert(children, ui.button { id = "sign_" .. short(id), anchor = id .. "#sign", width = 170, text = facility.name,
                hover = text, on = "open", payload = { facility = id } })
        end
        -- The building itself: it lifts and outlines under the pointer, and a click opens it.
        table.insert(children, ui.target { id = "target_" .. short(id), anchor = "being." .. id, text = text,
            on = "open", payload = { facility = id } })
        -- Its key icon: dark like the rest of the HUD, so the gold signs on the station
        -- and Launch stand out as the things to press.
        table.insert(key, kit.button(ui, { id = "key_" .. short(id), width = KEY, size = kit.SIZE.small, icon = key_icon(ctx, id, facility),
            text = facility.name, hover = text, on = "open", payload = { facility = id } }, "dark"))
    end
    table.insert(children, ui.row { id = "key", align = "bottom", y = layout.M, width = #key * (KEY + KEY_GAP) - KEY_GAP, gap = KEY_GAP,
        children = key })
    table.insert(children, kit.button(ui, { id = "launch", align = "bottom-right", x = layout.M, y = layout.M, width = LAUNCH,
        icon = "ship", text = "Launch · U", hover = "Fly off from the berth: U, or just fly with W, A, S or D", on = "launch" }, "primary"))
    return layout.frame(ui, "dock", children)
end

return {
    on = {
        open = open, close = close, launch = launch, sell = sell, buy = buy,
        fit = cradle.fit, unfit = cradle.unfit,
        ["engine.place_arrived"] = settled, ["engine.camera_settled"] = settled,
    },
    cadence = skiffs,
    screen = function(ctx, ui)
        local station, pilot = docked(ctx)
        if not station or pilot.log_open then return nil end
        if pilot.room ~= "" then return room_panel(ctx, ui, station, pilot) end
        return overview(ctx, ui, station, pilot)
    end,
}
