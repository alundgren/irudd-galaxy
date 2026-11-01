return {
    on = {
        move = function(_event, ctx)
            ctx.move("visitor", "block_b", nil, "visiting")
            ctx.move("player", "block_a")
            ctx.advance_clock("clock", 2)
            ctx.advance_clock("clock", 3)
        end,
        walk = function(_event, ctx)
            ctx.walk("visitor", "b_table", 1, "resting")
        end,
    },
}
