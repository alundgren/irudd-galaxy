-- The ship log: one entity per entry, newest kept, oldest dropped.
local log = { KEEP = 24 }

-- Adds one entry. Call at most once per handler (log.add_all logs several): reads see
-- the world as it was when the handler started. `talk` marks an entry that holds a
-- conversation, and `open` one whose conversation goes on. `after` is the entry it
-- follows: the last logged, by default.
local function put(ctx, text, talk, open, after)
    local n = (after or ctx.get("ship", "pilot").logged) + 1
    ctx.put("log" .. n, "entry", { n = n, text = text, talk = talk or "", open = open or false })
    if n > log.KEEP then ctx.remove("log" .. (n - log.KEEP)) end
    return n
end

-- What just happened to the ship: logged and shown as the current message.
function log.add(ctx, text)
    ctx.patch("ship", "pilot", { logged = put(ctx, text), message = text })
end

-- Several things that happened at once, logged in order; the last is the message.
function log.add_all(ctx, texts)
    local n = ctx.get("ship", "pilot").logged
    for _, text in texts do n = put(ctx, text, nil, nil, n) end
    ctx.patch("ship", "pilot", { logged = n, message = texts[#texts] })
end

-- Something a person said: logged only, so talk never follows the ship into space.
function log.say(ctx, text, talk, open)
    ctx.patch("ship", "pilot", { logged = put(ctx, text, talk, open) })
end

return log
