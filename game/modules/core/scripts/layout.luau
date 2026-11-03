-- Screen placement. Each piece sits against a window edge (`align`) with a 24px margin,
-- so the HUD holds at any window size and display scale. Panels are sized for the
-- 1280x720 design size and still fit a 1024x576 window.
local layout = { M = 24, PANEL = 520, BUTTON = 62 }

-- One screen's widgets: each child places itself against the window's edges.
function layout.frame(ui, id, children)
    return ui.column { id = id, children = children }
end

return layout
