-- Ines's greenhouse, the job after the galley (scripts/project.luau): when each offer is
-- on, when the panes are ready, what the log pins at each step and how the hold reads
-- while it carries her panes.
local project = require("../scripts/project")

local function job(step)
    return { step = step, ware = "glassite", need = 4, cargo = "panes", units = 4, fired = 3 }
end

local NAMES = { giver = "Ines Varga", giver_at = "Lantern Dock", maker = "Sefa Lind", maker_at = "Cinder Kiln" }

return {
    an_offer_is_on_only_at_its_own_step = function()
        assert(not project.wanted(job(project.ACCEPTED), project.ASKED, {}, 6, true), "asking is over once accepted")
        assert(not project.wanted(job(project.ASKED), project.ACCEPTED, { glassite = 9 }, 6, true), "not yet accepted")
    end,

    she_asks_only_once_her_gift_is_answered = function()
        assert(project.wanted(job(project.ASKED), project.ASKED, {}, 6, true), "gift answered: she asks")
        assert(not project.wanted(job(project.ASKED), project.ASKED, {}, 6, false), "gift open: she waits")
    end,

    her_galley_is_lived_in_before_she_asks_for_more = function()
        assert(not project.lived(true, 3, 3, false), "the docking that saw it open: she waits")
        assert(project.lived(true, 4, 3, false), "flown out and back, a counted visit: she asks")
        assert(project.lived(false, 0, 0, false), "no galley of hers standing: nothing to wait on")
        assert(project.lived(true, 3, 3, true), "an ask already said stays said (a save from before)")
    end,

    the_glassite_offer_needs_enough_glassite = function()
        assert(not project.wanted(job(project.ACCEPTED), project.ACCEPTED, { glassite = 3 }, 6, true), "3 is short")
        assert(project.wanted(job(project.ACCEPTED), project.ACCEPTED, { glassite = 4 }, 6, true), "4 will do")
    end,

    the_panes_wait_for_room_in_the_hold = function()
        assert(not project.wanted(job(project.READY), project.READY, { ore = 3 }, 3, true), "3 free: no room for 4 panes")
        assert(project.wanted(job(project.READY), project.READY, {}, 4, true), "4 free: room enough")
    end,

    she_takes_her_panes_only_when_they_are_aboard = function()
        assert(project.wanted(job(project.CARRIED), project.CARRIED, { panes = 4 }, 2, true), "panes aboard")
        assert(not project.wanted(job(project.CARRIED), project.CARRIED, {}, 6, true), "no panes aboard")
    end,

    the_panes_are_ready_after_the_next_counted_visit = function()
        assert(not project.fired(job(project.FIRING), 3), "the visit she began on")
        assert(project.fired(job(project.FIRING), 4), "the next counted visit")
        assert(not project.fired(job(project.READY), 9), "already ready")
    end,

    the_log_says_what_to_do_next = function()
        assert(project.step_text(job(project.ASKED), NAMES, 0) == nil, "nothing pinned before it is taken on")
        local bring = project.step_text(job(project.ACCEPTED), NAMES, 2)
        assert(bring == "Bring 4 glassite to Sefa Lind at Cinder Kiln for the panes. You carry 2.", bring)
        local firing = project.step_text(job(project.FIRING), NAMES, 0)
        assert(firing == "Sefa is firing the panes. Go about your work and come back to Cinder Kiln.", firing)
        local carry = project.step_text(job(project.CARRIED), NAMES, 0)
        assert(carry == "Carry Ines's 4 panes to her at Lantern Dock.", carry)
        assert(project.step_text(job(project.BUILDING), NAMES, 0) == nil, "the build's own pin takes over")
    end,

    her_panes_read_as_hers_in_the_hold = function()
        local both = project.list({ ore = 2, panes = 4 }, "panes", "Ines's")
        assert(both == "2 ore, Ines's 4 panes", both)
        local only = project.list({ panes = 4 }, "panes", "Ines's")
        assert(only == "Ines's 4 panes", only)
        assert(project.list({ ore = 2 }, "panes", nil) == "2 ore", "nothing of hers aboard")
    end,

    a_first_name_is_the_first_word = function()
        assert(project.first("Noor Hale") == "Noor", "Noor")
        assert(project.first("Bram") == "Bram", "a single name")
    end,
}
