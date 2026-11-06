-- What people remember of the player's doings (scripts/history.luau): news is said before
-- an introduction not yet heard, a greeting to a stranger is left out, news is told once,
-- and an introduction met late is retold without unsaying what was heard.
local history = require("../scripts/history")

-- A dockmaster with a three-line introduction whose first line greets a stranger. Writes
-- land in the world, so a later call reads them.
local function world()
    local w = {
        ["core.bram"] = { keeper = { name = "Bram Okafor", greeting = "core.bram_intro1" } },
        ["core.bram_intro"] = { ["engine.talk"] = { first = "core.bram_intro1", seen = false, active = false } },
        ["core.bram_intro1"] = { ["engine.talk_line"] = { text = "Welcome to Lantern Dock.", next = "core.bram_intro2" } },
        ["core.bram_intro2"] = { ["engine.talk_line"] = { text = "I'm Bram.", next = "core.bram_intro3" } },
        ["core.bram_intro3"] = { ["engine.talk_line"] = { text = "We both buy ore.", next = "" } },
    }
    local ctx = {
        get = function(id, component) return w[id] and w[id][component] end,
        put = function(id, component, fields)
            w[id] = w[id] or {}
            w[id][component] = table.clone(fields)
        end,
        patch = function(id, component, fields)
            for k, v in fields do w[id][component][k] = v end
        end,
    }
    return ctx, w
end

-- The introduction's lines in order, from its first not yet heard.
local function said(w)
    local texts, line = {}, w["core.bram_intro"]["engine.talk"].first
    while line and line ~= "" do
        table.insert(texts, w[line]["engine.talk_line"].text)
        line = w[line]["engine.talk_line"].next
    end
    return table.concat(texts, " | ")
end

-- The player clicks on through the first line not yet heard (talk.luau moves `first` on).
local function hear(w)
    local intro = w["core.bram_intro"]["engine.talk"]
    intro.first = w[intro.first]["engine.talk_line"].next
end

return {
    news_comes_first_and_the_greeting_is_left_out = function()
        local ctx, w = world()
        history.fold(ctx, "core.bram", "Cold Kettle's moored off our berth for good now.")
        local got = said(w)
        assert(got == "Cold Kettle's moored off our berth for good now. | I'm Bram. | We both buy ore.", got)
    end,

    news_is_told_once = function()
        local ctx, w = world()
        history.fold(ctx, "core.bram", "Cold Kettle's moored off our berth for good now.")
        history.fold(ctx, "core.bram", "Cold Kettle's moored off our berth for good now.")
        local got = said(w)
        assert(got == "Cold Kettle's moored off our berth for good now. | I'm Bram. | We both buy ore.", got)
        hear(w)
        history.fold(ctx, "core.bram", "Cold Kettle's moored off our berth for good now.")
        got = said(w)
        assert(got == "I'm Bram. | We both buy ore.", "heard news isn't told again: " .. got)
    end,

    newer_news_replaces_news_not_yet_heard = function()
        local ctx, w = world()
        history.fold(ctx, "core.bram", "So you're the pilot who towed Cold Kettle in.")
        history.fold(ctx, "core.bram", "Cold Kettle's moored off our berth for good now.")
        local got = said(w)
        assert(got == "Cold Kettle's moored off our berth for good now. | I'm Bram. | We both buy ore.", got)
    end,

    news_after_part_of_the_introduction_goes_before_the_rest = function()
        local ctx, w = world()
        hear(w)
        history.fold(ctx, "core.bram", "Cold Kettle's moored off our berth for good now.")
        local got = said(w)
        assert(got == "Cold Kettle's moored off our berth for good now. | I'm Bram. | We both buy ore.", got)
        hear(w)
        hear(w)
        history.fold(ctx, "core.bram", "Cold Kettle's our cistern now, and the beds bloom. Your doing.")
        got = said(w)
        assert(got == "Cold Kettle's our cistern now, and the beds bloom. Your doing. | We both buy ore.", got)
    end,

    a_heard_introduction_is_left_alone = function()
        local ctx, w = world()
        w["core.bram_intro"]["engine.talk"].seen = true
        history.fold(ctx, "core.bram", "Cold Kettle's moored off our berth for good now.")
        assert(w["core.bram_intro_news"] == nil, "no news line")
        assert(w["core.bram_intro"]["engine.talk"].first == "core.bram_intro1", "first unchanged")
    end,

    an_introduction_showing_now_is_left_alone = function()
        local ctx, w = world()
        w["core.bram_intro"]["engine.talk"].active = true
        history.fold(ctx, "core.bram", "Cold Kettle's moored off our berth for good now.")
        assert(w["core.bram_intro_news"] == nil, "no news line")
    end,

    news_counts_as_told_once_heard = function()
        local ctx, w = world()
        local line = "Your plate built our galley."
        assert(not history.told(ctx, "core.bram", line), "nothing told yet")
        history.fold(ctx, "core.bram", line)
        assert(not history.told(ctx, "core.bram", line), "still to come")
        hear(w)
        assert(history.told(ctx, "core.bram", line), "heard as news")
        assert(not history.told(ctx, "core.bram", "Something else."), "only that line")
    end,

    unfolding_takes_back_news_not_yet_heard = function()
        local ctx, w = world()
        history.fold(ctx, "core.bram", "Cold Kettle's moored off our berth for good now.")
        local first = history.unfold(ctx, "core.bram")
        assert(first == "core.bram_intro2", tostring(first))
        assert(said(w) == "I'm Bram. | We both buy ore.", said(w))
        assert(history.unfold(ctx, "core.bram") == "core.bram_intro2", "nothing more to take back")
    end,

    a_retold_introduction_keeps_what_was_heard = function()
        local ctx, w = world()
        hear(w)
        history.retell(ctx, "core.bram", { "So my Kettle stays moored here for good.", "I'm Hessa.", "I'd sooner have sailed her." })
        local got = said(w)
        assert(got == "I'm Hessa. | I'd sooner have sailed her.", got)
        assert(w["core.bram_intro1"]["engine.talk_line"].text == "Welcome to Lantern Dock.", "a heard line stays as heard")
    end,
}
