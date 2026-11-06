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

    only_a_mended_barge_s_skipper_offers_her_fee = function()
        local done = job(salvage.DONE)
        assert(salvage.wanted(done, salvage.DONE, true, nil, nil, "mend"), "mended: Hessa pays for the tow")
        assert(not salvage.wanted(done, salvage.DONE, true, nil, nil, "keep"), "kept for good: no mended-engine fee")
        assert(not salvage.wanted(done, salvage.DONE, true, nil, nil, nil), "no haven chosen: nothing")
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

    every_tow_action_has_a_click_as_well_as_a_key = function()
        -- A world with the job and the barge only, as salvage.click reads it.
        local function world(j)
            j.barge = "core.kettle"
            return { query = function(c) return c == "salvage" and { "core.kettle_job" } or {} end,
                get = function(id, c)
                    if c == "salvage" and id == "core.kettle_job" then return j end
                    if c == "trader" and id == "core.kettle" then return { name = "Cold Kettle", stopped = true } end
                    return nil
                end }
        end
        assert(salvage.click(world(job(salvage.ASKED)), "core.kettle") == nil, "before the search: a click does nothing")
        local adrift = salvage.click(world(job(salvage.SEARCH)), "core.kettle")
        assert(adrift == "hitch", "adrift: a click passes her a line")
        local moored, said = salvage.click(world(job(salvage.MOORED, { at = "core.lantern" })), "core.kettle")
        assert(moored == "hitch", "lying off a berth: a click passes the line again")
        assert(string.find(said, "click her to pass the line again", 1, true), said)
        local towing, about = salvage.click(world(job(salvage.SEARCH, { towing = true })), "core.kettle")
        assert(towing == "cancel", "in tow: a click casts her off, as 0 does")
        assert(string.find(about, "a click on her casts off", 1, true), about)
        assert(salvage.click(world(job(salvage.WORKING, { at = "core.lantern" })), "core.kettle") == nil, "handed over: nothing")
        assert(salvage.click(world(job(salvage.SEARCH)), "core.lamp") == nil, "another trader: nothing")
    end,

    the_skipper_met_aboard_starts_with_what_was_chosen_for_her_barge = function()
        local kept = salvage.met("keep", false, "Cold Kettle", "Noor", "Hessa", "Lantern Dock")
        assert(#kept == 3, "as long as her introduction")
        assert(kept[1] == "So my Kettle stays moored here for good.", kept[1])
        assert(kept[3] == "Her tanks for Noor's beds, then. I'd sooner have sailed her.", kept[3])
        local bloomed = salvage.met("keep", true, "Cold Kettle", "Noor", "Hessa", "Lantern Dock")
        assert(bloomed[3] == "Noor's beds bloom on her tanks. I'd sooner have sailed her.", bloomed[3])
        local mended = salvage.met("mend", false, "Cold Kettle", "Noor", "Hessa", "Cinder Kiln")
        assert(mended[1] == "You brought my Kettle to the Kiln to be mended.", mended[1])
        assert(mended[2] == "I'm Hessa. Thirty years she and I hauled ice.", mended[2])
        assert(mended[3] == "Noor's beds can wait. I want her sailing.", mended[3])
        local done = salvage.met("mend", true, "Cold Kettle", "Noor", "Hessa", "Cinder Kiln")
        assert(done[3] == "Noor's beds waited for their ice. Bless you.", done[3])
        for _, line in kept do assert(not string.find(line, "froze", 1, true), "not the radio's story again") end
    end,
}
