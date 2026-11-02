return {
    screen = function(_, ui)
        return ui.panel { width = 560, children = {
            ui.text { text = "A quiet place", size = 28 },
            ui.text { text = "Music cued on engine.start plays from the first frame and loops." },
        } }
    end,
}
