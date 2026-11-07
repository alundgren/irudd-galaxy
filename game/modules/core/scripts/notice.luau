-- A message and what comes with it. The bottom of the flight screen (hud.luau) draws
-- each kind of thing its own way, so one message never mixes kinds: what happened or
-- who said it (`text`), a goal it gives (`goal`), a lesson about the game (`lesson`)
-- and the key that fits it (`press`, "<key> · <what it does>"). The parts show only
-- while their message does: `aside` names it, so any later message hides them, however
-- it was set.
local notice = {}

-- `fields` (pilot fields, or a new table) with the message and its parts set: an
-- empty `text` shows only the parts.
function notice.with(fields, text, parts)
    fields = fields or {}
    parts = parts or {}
    fields.message = text
    fields.goal = parts.goal or ""
    fields.lesson = parts.lesson or ""
    fields.press = parts.press or ""
    fields.aside = text
    return fields
end

-- The parts showing with the pilot's message: goal, lesson and press, each "" when
-- there is none.
function notice.parts(pilot)
    if pilot.aside ~= pilot.message then return "", "", "" end
    return pilot.goal, pilot.lesson, pilot.press
end

-- A person's line on the radio, "<name> on <place>'s radio: <words>": the name, the
-- place and the words; nil for anything else.
function notice.speech(text)
    local who, from, words = string.match(text, "^(.-) on (.-)'s radio: (.+)$")
    if not who or who == "" or string.find(who, "[%.:]") then return nil end
    return who, from, words
end

return notice
