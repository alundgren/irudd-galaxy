-- The ship log: one entity per entry, newest kept, oldest dropped.
local log = { KEEP = 24 }

-- Adds one entry. Call at most once per handler: reads see the world as it was when
-- the handler started. `talk` marks an entry that holds a conversation.
local function put(ctx, text, talk)
    local n = ctx.get("ship", "pilot").logged + 1
    ctx.put("log" .. n, "entry", { n = n, text = text, talk = talk or "" })
    if n > log.KEEP then ctx.remove("log" .. (n - log.KEEP)) end
    return n
end

-- What just happened to the ship: logged and shown as the current message.
function log.add(ctx, text)
    ctx.patch("ship", "pilot", { logged = put(ctx, text), message = text })
end

-- Something a person said: logged only, so talk never follows the ship into space.
function log.say(ctx, text, talk)
    ctx.patch("ship", "pilot", { logged = put(ctx, text, talk) })
end

return log
