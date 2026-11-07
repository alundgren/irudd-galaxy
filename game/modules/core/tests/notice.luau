-- A message keeps one kind of thing; its goal, lesson and key are parts beside it
-- (scripts/notice.luau), shown only while it is the message.
local notice = require("../scripts/notice")

return {
    parts_follow_their_message = function()
        local pilot = notice.with({}, "Cutting.", { press = "Let go of Space · stop" })
        local goal, lesson, press = notice.parts(pilot)
        assert(goal == "" and lesson == "" and press == "Let go of Space · stop", "shown with their message")
        pilot.message = "+1 ore · hold 1 of 24"
        goal, lesson, press = notice.parts(pilot)
        assert(goal == "" and lesson == "" and press == "", "another message, set any way, hides them")
    end,

    a_lesson_alone = function()
        local pilot = notice.with({}, "", { lesson = "Fly into a berth's ring to dock." })
        local _, lesson = notice.parts(pilot)
        assert(pilot.message == "" and lesson == "Fly into a berth's ring to dock.")
    end,

    radio_speech = function()
        local who, from, words = notice.speech("Bram on Lantern Dock's radio: \u{201C}Welcome, pilot.\u{201D}")
        assert(who == "Bram" and from == "Lantern Dock" and words == "\u{201C}Welcome, pilot.\u{201D}")
        who, from = notice.speech("Hessa Lund on Cold Kettle's radio: \u{201C}A line!\u{201D}")
        assert(who == "Hessa Lund" and from == "Cold Kettle")
        assert(notice.speech("Order cancelled. Coasting to a stop.") == nil, "a notice is no speech")
        assert(notice.speech("Rock cut down. Kettle on Bram's radio: no") == nil, "a sentence before it is no name")
    end,
}
