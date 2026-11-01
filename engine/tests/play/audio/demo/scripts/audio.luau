local function pressed(event)
    return event.source ~= "keyboard" or event.payload.down == true
end

return {
    on = {
        tap = function(event, ctx)
            if pressed(event) then ctx.record("tone", {}) end
        end,
        music = function(event, ctx)
            if pressed(event) then ctx.record("music_start", {}) end
        end,
        stop = function(event, ctx)
            if pressed(event) then ctx.record("music_stop", {}) end
        end,
        change = function(event, ctx)
            if not pressed(event) then return end
            local location = ctx.get("player", "engine.location")
            if location.place == "audio.first" then
                ctx.move("player", "second")
            else
                ctx.move("player", "first")
            end
        end,
    },
    screen = function(ctx, ui)
        local location = ctx.get("player", "engine.location")
        local first = location.place == "audio.first"
        return ui.panel { width = 560, children = {
            ui.text { text = "Sound workshop", size = 28 },
            ui.text { text = first and "First place: low tone (220 Hz)" or "Second place: high tone (440 Hz)" },
            ui.text { text = "Enter changes place with a one-second crossfade." },
            ui.text { text = "Space plays a short ping. M starts music. S fades it out." },
            ui.button { id = "change", text = "Change place", on = "change" },
            ui.button { id = "tap", text = "Play ping", on = "tap" },
            ui.button { id = "music", text = "Start music", on = "music" },
            ui.button { id = "stop", text = "Stop music", on = "stop" },
        } }
    end,
}
