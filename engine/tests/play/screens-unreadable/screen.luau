return { screen = function(_, ui)
    return ui.panel { width = 180, height = 50, children = {
        ui.text { id = "tiny", text = "This is deliberately unreadable", size = 10 },
        ui.text { id = "outside", text = "This is outside its parent", width = 300 },
    } }
end }
