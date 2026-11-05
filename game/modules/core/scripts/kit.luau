-- The core's screen kit: one builder per kind of surface, so every script draws the
-- same kind of thing the same way (direction doc 1.5). The colours live in style.toml
-- ([screen.looks.*]); this file only picks looks, icons and spans. Each builder takes
-- the screen's `ui` and a widget table, sets the kind's look (and icon, when it has
-- one) and returns the widget, so any other widget property still works.
--
--   kit.chip(ui, w)        status: dark ink, lantern-gold border (the HUD chip)
--   kit.pill(ui, w)        a key hint: dark ink pill
--   kit.place(ui, w)       a place: the parchment room panel
--   kit.sheet(ui, w)       the trade table's sheet, inside a place
--   kit.heads(ui, cols)    a table's column heads: { { "Ware", 160 }, ... }
--   kit.ticket(ui, w, tag) a receipt: dark ticket; tag "CONFIRMED" (default) or "NOTICE"
--   kit.bubble(ui, w)      a panel dressed as speech (talk bubbles get it from style)
--   kit.button(ui, w, kind) "primary" (mustard), "secondary" (parchment) or "dark";
--                          w.resting = "<reason>" rests it, dashed, with its reason
--   kit.warning(ui, w)     pale vermilion with a warning icon
--   kit.tag(ui, word, w)   a kind word on a cream tag (JOB, OFFER, VISIT)
--   kit.heading(ui, w)     a place's title, large and heavy
--   kit.icon(ui, name, w)  a drawn icon (icons.toml)
--   kit.label(ui, w)       a small dark world label
-- Spans, for ui.text / ui.button `spans`:
--   kit.name(name, role)   the name heavy and underlined amber, the role quieter
--   kit.amount(n, unit, dark) a black amount with a blue + or vermilion - mark
--   kit.bars(level, of)    signal bars: lit gold, unlit dim
-- Icon names:
--   kit.ware(ware)         ore, plate, rations
--   kit.part(socket)       the part for a socket: lens, pods, thrusters, controls
local kit = {}

-- The look of each kind, by name in style.toml.
kit.LOOK = {
    chip = "chip", pill = "pill", place = "place", sheet = "table", column = "column",
    ticket = "ticket", confirmed = "confirmed", notice = "notice", kind = "kind",
    bubble = "bubble", name_tag = "name_tag", name = "name", quiet = "quiet",
    sign = "sign", label = "label", heading = "heading",
    primary = "primary", secondary = "secondary", dark = "dark_button",
    radio = "radio", warning = "warning",
    gain = "gain", loss = "loss", gain_dark = "gain_dark", loss_dark = "loss_dark",
    lit = "lit", unlit = "unlit",
}

-- Text sizes: a place's title, a surface's own heading, body text and small print
-- (the engine's floor is 16).
kit.SIZE = { title = 28, heading = 20, body = 18, small = 16 }

local WARES = { ore = "ore", ice = "ice", glassite = "glassite", plate = "plate", rations = "rations" }
local SOCKETS = { drill = "lens", hold = "pods", engine = "thrusters", controls = "controls" }

local function with(w, look)
    w = w or {}
    if w.look == nil then w.look = look end
    return w
end

function kit.chip(ui, w) return ui.panel(with(w, kit.LOOK.chip)) end
function kit.pill(ui, w) return ui.panel(with(w, kit.LOOK.pill)) end
function kit.place(ui, w) return ui.panel(with(w, kit.LOOK.place)) end
function kit.sheet(ui, w) return ui.panel(with(w, kit.LOOK.sheet)) end
function kit.bubble(ui, w) return ui.panel(with(w, kit.LOOK.bubble)) end
function kit.label(ui, w) return ui.text(with(w, kit.LOOK.label)) end

function kit.heading(ui, w)
    w = with(w, kit.LOOK.heading)
    if w.size == nil then w.size = kit.SIZE.title end
    return ui.text(w)
end

function kit.icon(ui, name, w)
    w = w or {}
    w.icon = name
    if w.size == nil then w.size = 20 end
    if w.width == nil then w.width = w.size end
    return ui.icon(w)
end

function kit.tag(ui, word, w)
    w = with(w, kit.LOOK.kind)
    w.text = word
    if w.size == nil then w.size = kit.SIZE.small end
    return ui.text(w)
end

-- A button of a kind: primary is the base mustard, so it needs no look.
function kit.button(ui, w, kind)
    w = w or {}
    if kind == "secondary" then
        w = with(w, kit.LOOK.secondary)
    elseif kind == "dark" then
        w = with(w, kit.LOOK.dark)
    end
    return ui.button(w)
end

-- A receipt: a dark ticket whose first child is the tag, then the given children.
function kit.ticket(ui, w, tag)
    w = with(w, kit.LOOK.ticket)
    local word = tag or "CONFIRMED"
    local children = { ui.text { text = word, size = kit.SIZE.small,
        look = word == "NOTICE" and kit.LOOK.notice or kit.LOOK.confirmed } }
    for _, child in w.children or {} do table.insert(children, child) end
    w.children = children
    if w.gap == nil then w.gap = 6 end
    return ui.panel(w)
end

-- A warning row: the icon, then the words.
function kit.warning(ui, w)
    w = with(w, kit.LOOK.warning)
    if w.icon == nil then w.icon = "warning" end
    return ui.text(w)
end

-- Column heads in one row: each { text, width }.
function kit.heads(ui, columns, w)
    w = w or {}
    local children = {}
    for _, c in columns do
        table.insert(children, ui.text { text = c[1], width = c[2], size = kit.SIZE.small, look = kit.LOOK.column })
    end
    w.children = children
    if w.gap == nil then w.gap = 8 end
    return ui.row(w)
end

-- Spans: a person's name standing out, then what they do, quieter.
function kit.name(name, role)
    local spans = { { text = name, look = kit.LOOK.name } }
    if role and role ~= "" then
        table.insert(spans, { text = "  " .. role, look = kit.LOOK.quiet })
    end
    return spans
end

-- Spans: an amount in black words with a blue + or vermilion - mark before it. `dark`
-- when it sits on a dark surface.
function kit.amount(n, unit, dark)
    local words = tostring(math.abs(n)) .. (unit and " " .. unit or "")
    if n == 0 then return { words } end
    local look = n > 0 and (dark and kit.LOOK.gain_dark or kit.LOOK.gain) or (dark and kit.LOOK.loss_dark or kit.LOOK.loss)
    return { { text = n > 0 and "+" or "-", look = look }, words }
end

-- Spans: `level` of `of` bars lit.
function kit.bars(level, of)
    local lit, unlit = string.rep("|", level), string.rep("|", of - level)
    local spans = {}
    if lit ~= "" then table.insert(spans, { text = lit, look = kit.LOOK.lit }) end
    if unlit ~= "" then table.insert(spans, { text = unlit, look = kit.LOOK.unlit }) end
    return spans
end

-- The icon for a ware, or the part that goes in a socket.
function kit.ware(ware) return WARES[ware] end
function kit.part(socket) return SOCKETS[socket] end

return kit
