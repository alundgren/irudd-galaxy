-- Happenings (direction doc 3.3): short pieces of visible life at a station, each with a
-- cause, a first-person line and a length counted in dockings there. A job begins one
-- (a lamp night when the greenhouse grows, a supper when a barge is kept); its props hang
-- on the station's deck while it lasts, and its speaker says its line meanwhile. Only
-- counted dockings use it up, so a quick launch and redock doesn't.
local happen = {
    -- How many later counted dockings at its station a happening lasts.
    LENGTH = 3,
}

-- The happening line `keeper` says at `station` while one lasts there, or nil.
function happen.line(ctx, keeper, station)
    for _, id in ctx.query("happening") do
        local happening = ctx.get(id, "happening")
        if happening.left > 0 and happening.station == station and happening.speaker == keeper then return happening.line end
    end
    return nil
end

-- Begins happening `id`: its props go up on the deck, each at its place and in its model,
-- and it lasts happen.LENGTH counted dockings. Returns its cause, for the log.
function happen.begin(ctx, id)
    local happening = ctx.get(id, "happening")
    if not happening then return nil end
    ctx.patch(id, "happening", { left = happen.LENGTH })
    local at = string.split(happening.at, " ")
    local models = string.split(happening.models or "", " ")
    for i, prop in string.split(happening.props, " ") do
        local xy = string.split(at[i] or "", ",")
        if prop ~= "" and #xy == 2 then
            local model = models[i]
            if model == nil or model == "" then model = "core.lamp_string" end
            ctx.put(prop, "engine.location", { place = happening.station })
            ctx.put(prop, "engine.position", { x = tonumber(xy[1]), y = tonumber(xy[2]), angle = (i % 2 == 0) and 1.2 or -1.2 })
            ctx.put(prop, "engine.model", { model = model, look = "core.prop" })
        end
    end
    return happening.cause
end

-- The ship docked at `station`; `counted` when it is a visit. A happening there counts
-- the visit, and takes its props down when it is over. Returns its cause for the
-- docking's log entry while it still lasts, or nil.
function happen.arrived(ctx, station, counted)
    local note = nil
    for _, h in ctx.query("happening") do
        local happening = ctx.get(h, "happening")
        if happening.station == station and happening.left > 0 and counted then
            local left = happening.left - 1
            ctx.patch(h, "happening", { left = left })
            if left > 0 then
                note = happening.cause
            else
                for _, prop in string.split(happening.props, " ") do
                    if prop ~= "" and ctx.has(prop, "engine.model") then ctx.remove(prop) end
                end
            end
        end
    end
    return note
end

return happen
