local rules = require("./rules")

local function space(ctx)
    return ctx.get("ship", "engine.location").place == "core.reach"
end

local function pressed(event)
    return event.payload.down ~= false
end

local function message(ctx, text)
    ctx.patch("ship", "pilot", { message = text })
end

local function stop(ctx)
    ctx.put("ship", "engine.helm", {})
    ctx.put("ship", "engine.steer", {})
    ctx.put("ship", "held", {})
    ctx.patch("ship", "pilot", { mining = false, target = "" })
end

local function flight(event, ctx)
    if not space(ctx) then return end
    local held = ctx.get("ship", "held")
    local key = string.sub(event.kind, 6)
    held[key] = event.payload.down == true
    ctx.patch("ship", "held", { [key] = held[key] })
    local function axis(a, b)
        return (held[a] and 1 or 0) - (held[b] and 1 or 0)
    end
    ctx.patch("ship", "engine.helm", {
        thrust = axis("ahead", "back"), turn = axis("left", "right"), boost = held.boost,
    })
    if pressed(event) then
        ctx.put("ship", "engine.steer", {})
        ctx.patch("ship", "pilot", { mining = false, target = "", message = "Flying by hand. Release thrust to stop." })
    end
end

local function nearest(ctx, component)
    local at = ctx.get("ship", "engine.position")
    local best, found = math.huge, nil
    for _, id in ctx.query(component) do
        local usable = component ~= "ore" or ctx.get(id, "ore").left > 0
        if usable then
            local positionId = component == "port" and ctx.get(id, "port").berth or id
            local d = rules.distance2(at, ctx.get(positionId, "engine.position"))
            if d < best then best, found = d, id end
        end
    end
    return found
end

local function mine(event, ctx)
    if not pressed(event) or not space(ctx) then return end
    local pilot = ctx.get("ship", "pilot")
    if pilot.cargo >= rules.CAPACITY then message(ctx, "Hold full. Dock and sell your six ore."); return end
    local target = nearest(ctx, "ore")
    if not target then message(ctx, "The field is worked out. Your ore still has a buyer."); return end
    stop(ctx)
    ctx.put("ship", "engine.steer", { mode = "hold", target = target, range = 1.5 })
    ctx.patch("ship", "pilot", { mining = true, target = target, message = "Approaching ore. One unit per second in reach." })
end

local function dock(event, ctx)
    if not pressed(event) or not space(ctx) then return end
    local station = event.payload.station
    if station ~= "core.lantern" and station ~= "core.kiln" then station = nearest(ctx, "port") end
    local port = ctx.get(station, "port")
    stop(ctx)
    ctx.put("ship", "engine.steer", { mode = "dock", target = port.berth })
    ctx.patch("ship", "pilot", { station = station, message = "Approaching " .. port.name .. "." })
end

local function market(event, ctx)
    if not pressed(event) then return end
    local station = ctx.get("ship", "pilot").station
    if station == "" or ctx.get("ship", "engine.location").place ~= station then return end
    ctx.move("ship", ctx.get(station, "port").market)
    message(ctx, "The posted price is yours whenever you choose to sell.")
end

local function leave(event, ctx)
    if not pressed(event) then return end
    local station = ctx.get("ship", "pilot").station
    if station == "" then return end
    if ctx.get("ship", "engine.location").place == ctx.get(station, "port").market then
        ctx.move("ship", station)
        message(ctx, "Back on the landing deck. U launches your ship.")
    end
end

local function launch(event, ctx)
    if not pressed(event) then return end
    local station = ctx.get("ship", "pilot").station
    if station == "" or ctx.get("ship", "engine.location").place ~= station then return end
    stop(ctx)
    ctx.move("ship", "core.reach", ctx.get(station, "port").departure)
    ctx.put("ship", "engine.body", { mass = 1 })
    ctx.patch("ship", "engine.model", { model = "core.ship" })
    ctx.patch("ship", "engine.camera", { view = 64 })
    message(ctx, "Clear of the berth. Space finds the nearest remaining ore.")
end

local function sell(event, ctx)
    if not pressed(event) then return end
    local pilot = ctx.get("ship", "pilot")
    if pilot.station == "" then return end
    local port = ctx.get(pilot.station, "port")
    if ctx.get("ship", "engine.location").place ~= port.market then return end
    if pilot.cargo == 0 then message(ctx, "Your hold is empty. More ore waits outside."); return end
    local earned = rules.sale(pilot.cargo, port.price)
    ctx.patch("ship", "pilot", { cargo = 0, credits = pilot.credits + earned,
        message = "Sold " .. pilot.cargo .. " ore for " .. earned .. " credits. The stockpile has grown." })
    ctx.patch(pilot.station, "port", { stock = port.stock + pilot.cargo })
    ctx.patch(port.market .. "_stock", "engine.model", { scale = 0.4 + (port.stock + pilot.cargo) * 0.04 })
    ctx.record("sold", { units = pilot.cargo, credits = earned, station = pilot.station })
end

return {
    on = {
        ahead = flight, back = flight, left = flight, right = flight, boost = flight,
        mine = mine, dock = dock, market = market, leave = leave, launch = launch, sell = sell,
        cancel = function(event, ctx)
            if pressed(event) and space(ctx) then stop(ctx); message(ctx, "Order cancelled. Coasting to a stop.") end
        end,
        ["engine.arrived"] = function(event, ctx)
            local pilot = ctx.get("ship", "pilot")
            if pilot.station == "" or not space(ctx) then return end
            local port = ctx.get(pilot.station, "port")
            if event.payload.mode ~= "dock" or event.payload.target ~= port.berth then return end
            stop(ctx)
            ctx.remove("ship", "engine.body")
            ctx.patch("ship", "engine.model", { model = "core.pilot" })
            ctx.patch("ship", "engine.camera", { view = 24 })
            ctx.move("ship", pilot.station)
            message(ctx, "Welcome to " .. port.name .. ". M opens the market room.")
            ctx.record("docked", { station = pilot.station })
        end,
    },
    cadence = function(ctx)
        local pilot = ctx.get("ship", "pilot")
        if not space(ctx) or not pilot.mining or pilot.target == "" then return end
        local ore = ctx.get(pilot.target, "ore")
        if rules.distance2(ctx.get("ship", "engine.position"), ctx.get(pilot.target, "engine.position")) > rules.MINE_DISTANCE ^ 2 then return end
        local amount = rules.transfer(pilot.cargo, ore.left)
        if amount == 0 then
            ctx.patch("ship", "pilot", { mining = false, message = pilot.cargo == rules.CAPACITY and "Hold full. E docks at the nearest station." or "Rock exhausted. Space selects another." })
            return
        end
        ctx.patch("ship", "pilot", { cargo = pilot.cargo + amount, message = "+1 ore aboard. The cut in the rock is visible." })
        ctx.patch(pilot.target, "ore", { left = ore.left - amount })
        ctx.patch(pilot.target, "engine.model", { morph = (ore.total - ore.left + amount) / ore.total })
        ctx.record("mined", { rock = pilot.target, units = amount })
    end,
    screen = function(ctx, ui)
        local pilot = ctx.get("ship", "pilot")
        local here = ctx.get("ship", "engine.location").place
        local children = {
            ui.text { text = "LANTERN REACH", size = 26 },
            ui.text { text = "Ore " .. pilot.cargo .. "/6    |    " .. pilot.credits .. " cr", size = 20 },
            ui.text { text = pilot.message, size = 18 },
        }
        if here == "core.reach" then
            table.insert(children, ui.text { text = "W/S thrust  A/D turn  Shift boost\nSpace mine  E dock  0 cancel", size = 16 })
            table.insert(children, ui.button { id = "mine", text = "Mine nearest ore", on = "mine" })
            table.insert(children, ui.button { id = "lantern", text = "Lantern Dock  |  12 cr / ore", on = "dock", payload = { station = "core.lantern" } })
            table.insert(children, ui.button { id = "kiln", text = "Cinder Kiln  |  16 cr / ore", on = "dock", payload = { station = "core.kiln" } })
        else
            local station = pilot.station
            -- The GM stages rooms directly while checking them.
            if here == "core.lantern" or here == "core.lantern_market" then station = "core.lantern" end
            if here == "core.kiln" or here == "core.kiln_market" then station = "core.kiln" end
            local port = ctx.get(station, "port")
            if port then
                table.insert(children, ui.text { text = port.name, size = 24 })
                if here == port.market then
                    table.insert(children, ui.text { text = "ORE EXCHANGE\nBuyer pays " .. port.price .. " cr / ore\nStation stock: " .. port.stock, size = 18 })
                    table.insert(children, ui.button { id = "sell", text = "Sell cargo  |  " .. rules.sale(pilot.cargo, port.price) .. " cr [Enter]", on = "sell" })
                    table.insert(children, ui.button { id = "leave", text = "Back to landing deck [B]", on = "leave" })
                else
                    table.insert(children, ui.button { id = "market", text = "Enter market room [M]", on = "market" })
                    table.insert(children, ui.button { id = "launch", text = "Launch [U]", on = "launch" })
                end
            end
        end
        return ui.panel { id = "ledger", x = 24, y = 24, width = 336, gap = 12, children = children }
    end,
}
