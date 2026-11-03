-- What each keeper says after their introduction: one short line that follows the
-- shift, so a second click hears something current instead of the same speech again.
local voices = {}

-- Ore still in the field's rocks.
function voices.field(ctx)
    local left = 0
    for _, id in ctx.query("ore") do
        left += ctx.get(id, "ore").left
    end
    return left
end

-- The exchange keeper: about the hold. `cargo` and `sold` are the values after the
-- calling handler's own writes, because reads still see the world before them.
function voices.trader(cargo, sold)
    if cargo > 0 then return "I'll take your " .. cargo .. " ore whenever you like." end
    if sold > 0 then return "Thanks. Your ore's on the pile already." end
    return "Empty hold? The rocks are just outside."
end

-- The office keeper: about the field.
function voices.master(left)
    if left > 0 then return "The field still holds " .. left .. " ore." end
    return "The field's worked out. Good shift."
end

-- Sets the current line of every keeper at `station`.
function voices.refresh(ctx, station, cargo, sold)
    local exchange = ctx.get(station, "port").exchange
    local left = voices.field(ctx)
    for _, id in ctx.query("facility") do
        local facility = ctx.get(id, "facility")
        if facility.station == station then
            local text = id == exchange and voices.trader(cargo, sold) or voices.master(left)
            ctx.patch(facility.keeper .. "_now", "engine.talk_line", { text = text })
        end
    end
end

return voices
