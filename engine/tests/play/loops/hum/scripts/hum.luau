-- Keys send { down = true } and { down = false }; a scripted input without a payload
-- counts as a press. Each records one cue event per real change.
local function pressed(event)
    return event.payload.down ~= false
end

return {
    on = {
        thrust = function(event, ctx)
            ctx.patch("ship", "engine.helm", { thrust = if pressed(event) then 1 else 0 })
        end,
        beam = function(event, ctx)
            ctx.record(if pressed(event) then "beam_on" else "beam_off", {})
        end,
        tap = function(event, ctx)
            if pressed(event) then ctx.record("ping", {}) end
        end,
    },
}
