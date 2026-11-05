-- The ship log's screen (log.luau keeps the entries): the book button at the bottom
-- left, and the open log at the top left, pinned items above one line per visit.
local layout = require("./layout")
local kit = require("./kit")
local log = require("./log")

local PINNED_HEIGHT, LIST_HEIGHT, LIST_MIN = 100, 360, 150

local INNER = layout.LOG - 32 -- the panel's width inside its padding
local ROW = INNER - 64 -- a row inside the scrolling list, beside its lane
local TAG = 76 -- a pin's kind word (WANTED)
local VISIT_TAG = 64 -- a visit's (HERE, VISIT, FLIGHT)

-- A pinned item: its kind, then who or what and where, then what is waiting.
local function pin(ui, p, i)
    return kit.sheet(ui, { id = "pin" .. i, width = INNER, children = { ui.row { gap = 8, children = {
        kit.tag(ui, p.kind, { width = TAG }),
        ui.column { width = INNER - 16 - TAG - 8, gap = 2, children = {
            ui.text { size = kit.SIZE.small, spans = kit.name(p.who, p.where) },
            ui.text { size = kit.SIZE.small, text = p.text },
        } },
    } } } })
end

-- One visit: its kind, where, and Lines to open it; under them one short line of who
-- was heard and what happened (log.summary). `here` marks the visit going on now.
local function visit_row(ctx, ui, visit, open, here)
    local who, summary = log.summary(ctx, visit)
    if here and #visit.entries == 1 then summary = "here now" end
    local where = visit.at ~= "" and ctx.get(visit.at, "port")
    local line = { summary }
    if who ~= "" then line = { { text = who, look = kit.LOOK.name }, " · " .. summary } end
    local children = {
        ui.row { gap = 8, children = {
            kit.tag(ui, here and "HERE" or where and "VISIT" or "FLIGHT", { width = VISIT_TAG }),
            ui.text { width = ROW - 16 - VISIT_TAG - 72 - 16, size = kit.SIZE.small, spans = kit.name(where and where.name or "In flight") },
            kit.button(ui, { id = "lines" .. visit.first, width = 72, pad = 12, size = kit.SIZE.small,
                text = open and "Hide" or "Lines", on = "log_visit", payload = { first = visit.first },
                hover = open and "Hide this visit's lines" or "What was said and done here, in order" }, "secondary"),
        } },
        ui.text { id = "summary" .. visit.first, width = ROW - 16, size = kit.SIZE.small, spans = line },
    }
    if open then
        for _, entry in visit.entries do
            table.insert(children, ui.text { width = ROW - 16, size = kit.SIZE.small, text = entry.text })
        end
    end
    return kit.sheet(ui, { id = "visit" .. visit.first, width = ROW, gap = 6, children = children })
end

local function panel(ctx, ui)
    local list = log.entries(ctx)
    local book = ctx.get("ship", "logbook")
    local open = book and book.visit or 0
    local children = {
        ui.row { id = "log_heading", gap = 12, children = {
            kit.icon(ui, "book", { size = 28 }),
            kit.heading(ui, { id = "log_title", text = "Ship log", width = INNER - 40 - 132 }),
            kit.button(ui, { id = "log_close", width = 120, pad = 12, size = kit.SIZE.small, text = "Close · L", on = "log",
                hover = "Close the log" }, "secondary"),
        } },
    }
    local pins = log.pinned(ctx, list)
    for i, p in pins do table.insert(children, pin(ui, p, i)) end
    local rows = {}
    local place = ctx.get("ship", "engine.location").place
    for i, visit in log.visits(list) do
        local here = i == 1 and visit.at ~= "" and visit.at == place
        table.insert(rows, visit_row(ctx, ui, visit, visit.first == open, here))
    end
    if #rows == 0 then rows = { ui.text { text = "Nothing logged yet.", size = kit.SIZE.body } } end
    table.insert(children, ui.text { id = "log_order", size = kit.SIZE.small, text = "Newest first, one line per visit." })
    table.insert(children, ui.scroll { id = "entries", height = math.max(LIST_MIN, LIST_HEIGHT - PINNED_HEIGHT * #pins),
        gap = 8, children = rows })
    return kit.place(ui, { id = "log", align = "top-left", x = layout.M, y = layout.M, width = layout.LOG, gap = 10, children = children })
end

-- The book button, with how many entries came in since the log was last open.
local function button(ctx, ui)
    local pilot = ctx.get("ship", "pilot")
    local book = ctx.get("ship", "logbook")
    local new = pilot.logged - (book and book.seen or 0)
    local spans = { new > 0 and "Log  " or "Log" }
    if new > 0 then table.insert(spans, { text = new .. " new", look = "new" }) end
    return kit.button(ui, { id = "log_open", align = "bottom-left", x = layout.M, y = layout.M, width = new > 0 and 132 or 88,
        height = 64, pad = 6, icon = "book", icon_at = "top", size = kit.SIZE.small, spans = spans,
        hover = "The ship log · L: what happened, what was said, and what is waiting", on = "log" })
end

-- The open visit's Lines, and what has been seen, live on the ship's logbook.
local function book(ctx)
    return ctx.get("ship", "logbook") or { visit = 0, seen = 0 }
end

local on = {
    -- Opening or closing the log marks what is in it as seen (hud.luau opens and
    -- closes it, and may already have run on this press).
    log = function(event, ctx)
        if event.payload.down == false then return end
        local pilot = ctx.get("ship", "pilot")
        if pilot.room ~= "" then return end
        local b = book(ctx)
        ctx.put("ship", "logbook", { visit = b.visit, seen = pilot.logged })
    end,
    -- Lines on a visit opens it; on the open one, closes it.
    log_visit = function(event, ctx)
        local b = book(ctx)
        local first = event.payload.first or 0
        ctx.put("ship", "logbook", { visit = b.visit == first and 0 or first, seen = b.seen })
    end,
}

local function screen(ctx, ui)
    local pilot = ctx.get("ship", "pilot")
    local here = ctx.get("ship", "engine.location").place
    local space = here == "core.reach"
    local docked = here ~= "" and ctx.get(here, "port") ~= nil
    if not space and not docked then return nil end
    if pilot.log_open then return panel(ctx, ui) end
    if docked and pilot.room ~= "" then return nil end
    return button(ctx, ui)
end

return { on = on, screen = screen }
