-- The engine's widget constructors, given to every screen as `ui`. A screen builds a tree
-- with these and returns it; the engine owns each widget's look, layout and behaviour.
-- Screens never pick colours: a surface takes its look from its role, and text on it is
-- pure black or pure white.
local ui = {}

local function make(kind, defaults)
	return function(t)
		if type(t) ~= "table" then
			t = { t }
		end
		t.kind = kind
		if defaults then
			for k, v in pairs(defaults) do
				if t[k] == nil then
					t[k] = v
				end
			end
		end
		return t
	end
end

-- The root: `scene` ("station" or "cradle"), `focus` (a module the camera glides to),
-- `back` (the event for Esc and right-click), `ship` (the fit the cradle shows).
ui.screen = make("screen")

-- Layout. `gap`, `pad`, `grow`, `width`, `align` ("start", "center", "end", "stretch").
-- Top-level layers take a `place`: top, top-left, top-right, bottom, bottom-left,
-- bottom-right, left, right, center.
ui.column = make("column", { gap = 8 })
ui.row = make("row", { gap = 8, align = "center" })
ui.spacer = make("spacer")
ui.divider = make("divider")

-- A surface in a style role: room, speech, goal, status, action, sign, radio, receipt,
-- plain.
ui.panel = make("panel", { gap = 8 })

-- Text in a size the engine owns: title, heading, body, figure, small, caption.
ui.text = make("text")
local function styled(style)
	return function(t)
		if type(t) ~= "table" then
			t = { t }
		end
		t.style = style
		return ui.text(t)
	end
end
ui.title = styled("title")
ui.heading = styled("heading")
ui.small = styled("small")
ui.caption = styled("caption")
ui.figure = styled("figure")

-- Something you can press: `on` and `args` are the event it sends. `rest = "reason"`
-- makes it rest (dashed, with the reason on hover) instead.
ui.button = make("button")
-- A square button with an icon; `ribbon = { "new", "New" }` hangs news off it and `hint`
-- is its one-line headline on hover.
ui.tile = make("tile")
-- A state word with its colour and shape: gain, loss, ok, refused, new, changed, closed,
-- info.
function ui.state(state, word)
	return { kind = "state", state = state, text = word }
end
-- A labelled bar for a real quantity: `value`, `max` (the free allowance) and its label.
ui.meter = make("meter")
function ui.icon(name, size)
	return { kind = "icon", icon = name, size = size }
end

-- Attached to the scene. `anchor` names a scene point ("module.market", "socket.nose").
-- A bubble: `speaker`, `place_name`, `text`, `more`, and the event a click sends.
ui.bubble = make("bubble")
-- A clickable place in the scene with its sign `{ title, keeper, line, state }`.
ui.hotspot = make("hotspot")

-- Drag and drop. A drag sends `on` when picked up and `cancel` if let go nowhere. A drop
-- target carries the verdict for the current drag: `ok = true` (with an optional
-- `preview` tree), `ok = false` with its `reason`, or no `ok` when it isn't a target for
-- this drag. Letting go on a target that takes it sends the target's `on` with both
-- widgets' args.
ui.drag = make("drag")
ui.drop = make("drop")

return ui
