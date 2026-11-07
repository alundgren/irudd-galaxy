-- The rumour's plain rules (scripts/rumour.luau): when the ship is close enough to see
-- the find, and what the keeper who gave the lead says while the job needs the ware.
local rumour = require("../scripts/rumour")

return {
    near_counts_the_edge = function()
        local ring = { x = 340, y = 272 }
        assert(rumour.near({ x = 340, y = 232 }, ring, 40), "40 units off is in sight")
        assert(not rumour.near({ x = 340, y = 231.9 }, ring, 40), "just past 40 is not")
        assert(rumour.near({ x = 368, y = 300 }, ring, 40), "28 across and 28 up is about 39.6")
    end,
    needs_says_nothing_until_heard_or_been = function()
        assert(rumour.needs(8, "glassite", "panes", "Ines", false, false) == nil, "the lead itself is a talk")
    end,
    needs_keeps_the_landmark_once_heard = function()
        assert(rumour.needs(8, "glassite", "panes", "Ines", false, true)
            == "8 glassite for Ines's panes. North, a touch east, by the lone buoy.")
    end,
    needs_remembers_the_find = function()
        -- Been out there (heard or not): she doesn't send the player again.
        for _, heard in { false, true } do
            assert(rumour.needs(8, "glassite", "panes", "Ines", true, heard)
                == "8 glassite for Ines's panes, from round that old ring.")
        end
    end,
}
