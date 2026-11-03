-- The ship log: one entity per entry, newest kept, oldest dropped.
local log = { KEEP = 24 }

-- Adds one entry and makes it the current message. Call at most once per handler:
-- reads see the world as it was when the handler started.
function log.add(ctx, text)
    local n = ctx.get("ship", "pilot").logged + 1
    ctx.put("log" .. n, "entry", { n = n, text = text })
    if n > log.KEEP then ctx.remove("log" .. (n - log.KEEP)) end
    ctx.patch("ship", "pilot", { logged = n, message = text })
end

return log
