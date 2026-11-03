-- Screen placement at the 1280x720 design size. Screens place their widgets inside
-- one frame that keeps a 24px margin from the window edge; x and y are frame pixels.
local layout = { W = 1232, H = 672, PANEL = 520, BUTTON = 62 }

function layout.frame(ui, id, children)
    return ui.column { id = id, x = 24, y = 24, pad = 0, width = layout.W, height = layout.H, children = children }
end

return layout
