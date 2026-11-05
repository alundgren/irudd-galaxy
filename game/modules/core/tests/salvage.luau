-- Cold Kettle adrift, the job after the greenhouse (scripts/salvage.luau): when a line can
-- be passed and when it slips, when each offer is on, and what the log pins at each step.
local salvage = require("../scripts/salvage")

local function job(step, fields)
    local j = { step = step, towing = false, at = "", slips = 0 }
    for k, v in fields or {} do j[k] = v end
    return j
end

local NAMES = { barge = "Cold Kettle", giver = "Noor Hale", skipper = "Hessa Kell", at = "Lantern Dock", keeper = "Bram Okafor" }

return {
    a_line_is_passed_close_and_slow = function()
        assert(salvage.can_pass(salvage.REACH, salvage.SLOW), "at the edge of reach, at the limit: yes")
        assert(not salvage.can_pass(salvage.REACH + 0.5, 2), "too far off")
        assert(not salvage.can_pass(3, salvage.SLOW + 1), "too fast")
    end,

    she_takes_a_line_only_once_the_search_is_on = function()
        assert(not salvage.hitchable(job(salvage.ASKED)), "not before Noor's ask is accepted")
        assert(salvage.hitchable(job(salvage.SEARCH)), "searching: yes")
        assert(not salvage.hitchable(job(salvage.SEARCH, { towing = true })), "already in tow")
        assert(salvage.hitchable(job(salvage.MOORED, { at = "core.lantern" })), "lying off a berth: she can be towed on")
        assert(not salvage.hitchable(job(salvage.WORKING)), "handed over")
    end,

    only_a_hard_knock_on_something_fixed_slips_the_line = function()
        assert(salvage.slips(true, salvage.STRUCK), "a rock, hard")
        assert(not salvage.slips(true, salvage.STRUCK - 0.5), "a rock, gently")
        assert(not salvage.slips(false, 9), "the ship or a trader, however hard")
    end,

    noor_asks_once_her_thanks_are_heard = function()
        assert(salvage.wanted(job(salvage.ASKED), salvage.ASKED, true, nil), "thanks heard: she asks")
        assert(not salvage.wanted(job(salvage.ASKED), salvage.ASKED, false, nil), "not yet")
        assert(not salvage.wanted(job(salvage.SEARCH), salvage.ASKED, true, nil), "asking is over once accepted")
    end,

    a_dockmaster_offers_only_for_the_barge_off_their_own_berth = function()
        local off_lantern = job(salvage.MOORED, { at = "core.lantern" })
        assert(salvage.wanted(off_lantern, salvage.MOORED, true, "core.lantern"), "Bram, with her off his berth")
        assert(not salvage.wanted(off_lantern, salvage.MOORED, true, "core.kiln"), "Mags, with her off Bram's")
        local in_tow = job(salvage.MOORED, { at = "core.lantern", towing = true })
        assert(not salvage.wanted(in_tow, salvage.MOORED, true, "core.lantern"), "towed on again: no offer")
    end,

    the_work_waits_on_its_delivery_aboard = function()
        local kept = job(salvage.NEEDS, { at = "core.lantern" })
        assert(salvage.wanted(kept, salvage.NEEDS, true, "core.lantern", true), "the plate aboard: Bram's offer is on")
        assert(not salvage.wanted(kept, salvage.NEEDS, true, "core.lantern", false), "short of plate: not yet")
        assert(not salvage.wanted(kept, salvage.NEEDS, true, "core.kiln", true), "Mags has no say once Bram has her")
    end,

    the_log_says_what_to_do_next = function()
        local find = salvage.step_text(job(salvage.SEARCH), NAMES)
        assert(find == "Find Cold Kettle, dark out west past the ice, and pass her a line.", find)
        local again = salvage.step_text(job(salvage.SEARCH, { slips = 1 }), NAMES)
        assert(string.find(again, "She lies where the line slipped.", 1, true), again)
        local tow = salvage.step_text(job(salvage.SEARCH, { towing = true }), NAMES)
        assert(tow == "Tow Cold Kettle in by hand: Lantern Dock keeps her tanks for Noor's beds; Cinder Kiln mends her for Hessa.", tow)
        local off = salvage.step_text(job(salvage.MOORED), NAMES)
        assert(off == "Cold Kettle lies off Lantern Dock. See Bram in the office, or tow her on.", off)
        local names = table.clone(NAMES)
        names.kind, names.haven_at, names.keeper = "keep", "Lantern Dock", "Bram Okafor"
        names.need, names.ware, names.carry = 4, "plate", 1
        local plate = salvage.step_text(job(salvage.NEEDS), names)
        assert(plate == "Bring 4 plate to Bram at Lantern Dock for her pipes. You carry 1.", plate)
        names.kind, names.haven_at, names.trips = "mend", "Cinder Kiln", 2
        local mend = salvage.step_text(job(salvage.WORKING), names)
        assert(mend == "Her engine is being mended. Come back to Cinder Kiln after two more trips out.", mend)
        names.trips = 1
        local last = salvage.step_text(job(salvage.WORKING), names)
        assert(last == "Her engine is being mended. Come back to Cinder Kiln after your next trip out.", last)
        assert(salvage.step_text(job(salvage.DONE), names) == nil, "nothing pinned once it is done")
    end,
}
