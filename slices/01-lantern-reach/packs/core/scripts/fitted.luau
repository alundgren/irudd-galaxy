--!strict
-- The "fitted" moment (#33): when the player fits an upgrade or a tender is built for
-- them, the person who did it says so in the first person, with the part's literal
-- effect text from `upgrades.toml` ("I've fitted your lane chart. Click a place on the
-- map and your ship goes there."). Fitting is always the player's own purchase from
-- that person's list, gifts included: nothing is fitted for them.
local G = require("@galaxy")

-- The effect as a sentence: "Click a place ..." -> "Click a place ... ."
local function sentence(effect: string): string
	local e = string.gsub(effect, "%.+$", "")
	return if e == "" then "" else ` {e}.`
end

-- Says `text` from whoever sells `id`, where they sell it, if the player is docked
-- there. `cite` is the fitting's fact.
local function say(ctx: G.Ctx, id: string, cite: string, text: (s: G.ServiceView) -> string)
	local s = ctx.beats:service(id)
	if s == nil or s.seller == nil or s.station == nil then
		return
	end
	if ctx.beats:pilot().docked_at ~= s.station then
		return
	end
	local why = ctx:cause({ summary = `{s.name} fitted, and its giver says so`, cites = { cite } })
	ctx.talk:offer({
		speaker = s.seller :: string,
		place = s.station :: string,
		text = text(s),
		cites = { cite },
	}, why)
end

return G.script({
	id = "fitted",
	on = {
		upgrade_fitted = function(ev: G.UpgradeFittedEvent, ctx: G.Ctx)
			say(ctx, ev.upgrade, ev.fact, function(s: G.ServiceView): string
				return `There: I've fitted your {string.lower(s.name)}.{sentence(s.effect)}`
			end)
		end,
		tender_acquired = function(ev: G.TenderAcquiredEvent, ctx: G.Ctx)
			say(ctx, ev.craft, ev.fact, function(s: G.ServiceView): string
				return `I've built {s.name} for you. She's yours now.{sentence(s.effect)}`
			end)
		end,
	},
})
