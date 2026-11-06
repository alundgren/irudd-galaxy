-- What people remember of what the player has done with them (research: "Salience and
-- memory"): the most specific line wins over the generic one. A keeper with something to
-- say about it says that first, even before an introduction they haven't given yet, and an
-- introduction met only now no longer greets a stranger the player has just worked with.
-- The lines themselves follow the jobs (salvage.news, project.news, voices.refresh).
--
-- Lines already heard stay heard: talk.luau moves an introduction's `first` on line by
-- line, so these only ever change what is still to come.
local history = {}

-- Folds `text` into `keeper`'s introduction while it is unheard: it is said first, in front
-- of the first line not yet heard, and a greeting there (`keeper.greeting`) is left out.
-- Said once: a line already told isn't told again, and newer news replaces news not yet
-- heard. Nothing changes while the introduction is showing.
function history.fold(ctx, keeper, text)
    if not text or text == "" then return end
    local id = keeper .. "_intro"
    local intro = ctx.get(id, "engine.talk")
    if not intro or intro.seen or intro.active then return end
    local news = keeper .. "_intro_news"
    local told = ctx.get(news, "engine.talk_line")
    if told and told.text == text then return end -- still to come, or heard already
    if told and intro.first == news then
        ctx.patch(news, "engine.talk_line", { text = text })
        return
    end
    local first = intro.first or ""
    local greeting = ctx.get(keeper, "keeper").greeting or ""
    if first ~= "" and first == greeting then first = ctx.get(greeting, "engine.talk_line").next or "" end
    ctx.put(news, "engine.talk_line", { text = text, next = first })
    ctx.patch(id, "engine.talk", { first = news })
end

-- Says `keeper`'s unheard introduction the way `texts` has it, line for line from the end,
-- so the lines already heard stay heard and only the rest is said the new way.
function history.retell(ctx, keeper, texts)
    local intro = ctx.get(keeper .. "_intro", "engine.talk")
    if not intro or intro.seen or intro.active then return end
    local lines, line = {}, intro.first
    while line and line ~= "" and #lines < 16 do
        table.insert(lines, line)
        line = ctx.get(line, "engine.talk_line").next
    end
    local skip = #texts - #lines
    for i, id in lines do
        local text = texts[i + skip]
        if text then ctx.patch(id, "engine.talk_line", { text = text }) end
    end
end

return history
