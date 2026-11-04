-- Control hints that retire: each is shown until the player has used it a few times.
local teach = { USES = 3 }

function teach.used(ctx, what)
    local taught = ctx.get("ship", "taught")
    if taught[what] < teach.USES then ctx.patch("ship", "taught", { [what] = taught[what] + 1 }) end
end

-- Whether the player still needs the hint for `what`.
function teach.new(taught, what)
    return taught[what] < teach.USES
end

return teach
