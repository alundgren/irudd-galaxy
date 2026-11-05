-- Markets and what stations build, read from the world. Pricing itself is in rules.
local rules = require("./rules")

local trade = {}

-- The market lines traded in `room`, in ware order.
function trade.lines(ctx, room)
    local found = {}
    for _, id in ctx.query("market") do
        local line = ctx.get(id, "market")
        if line.room == room then table.insert(found, id) end
    end
    local order = {}
    for i, ware in rules.WARES do
        order[ware] = i
    end
    table.sort(found, function(a, b) return order[ctx.get(a, "market").ware] < order[ctx.get(b, "market").ware] end)
    return found
end

-- The station a market line belongs to: the station of its room.
function trade.station(ctx, line)
    return ctx.get(line.room, "facility").station
end

-- Every market line at `station`.
function trade.station_lines(ctx, station)
    local found = {}
    for _, id in ctx.query("market") do
        if trade.station(ctx, ctx.get(id, "market")) == station then table.insert(found, id) end
    end
    return found
end

-- What `station` is still building, if anything: its site's ID and build.
function trade.building(ctx, station)
    for _, id in ctx.query("build") do
        local build = ctx.get(id, "build")
        if build.station == station and not build.done then return id, build end
    end
    return nil, nil
end

-- The crate pile beside an exchange grows with the ore its station holds.
function trade.pile(stock)
    return math.max(0.3, 0.04 * stock - 0.4)
end

-- The stack of ore crates in the exchange room: a person's height at the start, never
-- up to the ceiling.
function trade.room_pile(stock)
    return math.min(1.2, 0.45 + 0.5 * trade.pile(stock))
end

-- Sets both of a station's crate piles to its ore stock: the one on its deck and the
-- one in its exchange.
function trade.piles(ctx, station, stock)
    ctx.patch(station .. "_stock", "engine.model", { scale = trade.pile(stock) })
    ctx.patch(station .. "_stock_room", "engine.model", { scale = trade.room_pile(stock) })
end

-- The day moves on with the work done at the stations (a trade, a fit), never with
-- time: their people go on to their next act, and in the room you are in you watch
-- them get up and walk (start.toml, engine.life). Both stations share the one day.
function trade.day(ctx)
    ctx.advance_clock("day", 1)
end

-- "Lantern Dock buys ore at 13, plate at 38; sells rations at 14": every open line.
function trade.posted(ctx, station)
    local buys, sells = {}, {}
    for _, room in ctx.query("facility") do
        local facility = ctx.get(room, "facility")
        local site = ctx.get(room, "build")
        if facility.station == station and not (site and not site.done) then
            for _, id in trade.lines(ctx, room) do
                local line = ctx.get(id, "market")
                local text = line.ware .. " at " .. rules.price(line)
                table.insert(line.role == "buys" and buys or sells, text)
            end
        end
    end
    local name = ctx.get(station, "port").name
    local parts = {}
    if #buys > 0 then table.insert(parts, "buys " .. table.concat(buys, ", ")) end
    if #sells > 0 then table.insert(parts, "sells " .. table.concat(sells, ", ")) end
    return name .. " " .. table.concat(parts, "; ")
end

return trade
