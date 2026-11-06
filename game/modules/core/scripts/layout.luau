-- Screen placement. Each piece sits against a window edge (`align`) with a 24px margin,
-- so the HUD holds at any window size and display scale. Panels are sized for the
-- 1280x720 design size and still fit a 960x540 window.
-- The corners hold the ship's own things, so the middle stays the work area: the
-- status chip top right (CHIP wide), the log's book bottom left, the radio top left in
-- flight (RADIO wide, one slim strip), and the message and flight keys at the bottom
-- centre (MESSAGE wide). A room or the open log is a panel against one side, at most
-- PANEL wide (about 45% of the design width), on the side away from what it is about.
-- Every room on a station (trading, office, site and the fitting cradle) is ROOM wide:
-- 45% of a 960x540 window, so the room's building and the ship stay in view there too.
-- The open log is LOG wide: the ship sits mid-window, 480px in at 960x540 (moored on
-- its cradle, its tail reaches about 416px), so the log ends at 404px and the smallest
-- window still shows the whole ship.
local layout = { M = 24, PANEL = 520, ROOM = 430, LOG = 380, BUTTON = 62, CHIP = 300, RADIO = 420, MESSAGE = 464 }

-- One screen's widgets: each child places itself against the window's edges.
function layout.frame(ui, id, children)
    return ui.column { id = id, children = children }
end

return layout
