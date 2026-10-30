--!strict
-- Small helpers the journey's scripts share. Pure reads plus two writers (`say` and
-- `record`) that always take the caller's one cause.
local G = require("@galaxy")
local ids = require("./ids")

local util = {}

-- A number from pacing.toml. Missing data is an error, never a silent default.
function util.num(ctx: G.Ctx, id: string): number
	local v = ctx.pacing:get(id)
	if type(v) ~= "number" then
		error(`pacing.toml has no number {id}`)
	end
	return v
end

-- A list of IDs from pacing.toml.
function util.list(ctx: G.Ctx, id: string): { string }
	local v = ctx.pacing:get(id)
	if type(v) ~= "table" then
		error(`pacing.toml has no list {id}`)
	end
	return v
end

-- Whether a list holds a value.
function util.has(list: { string }, value: string?): boolean
	if value == nil then
		return false
	end
	for _, v in ipairs(list) do
		if v == value then
			return true
		end
	end
	return false
end

-- A fact's sequence number: `fact.42` -> 42. History orders facts by it.
function util.seq(id: string?): number
	if id == nil then
		return 0
	end
	return tonumber(string.sub(id, 6)) or 0
end

-- The fact a beat's "since start" counts after (nil: all of history).
function util.since(ctx: G.Ctx, beat: string): string?
	local s = ctx.beats:state(beat)
	return if s then s.since else nil
end

-- The step a beat is on (0 = first), or -1 when it isn't active.
function util.step(ctx: G.Ctx, beat: string): number
	local s = ctx.beats:state(beat)
	if s == nil or s.status ~= "active" then
		return -1
	end
	return s.step
end

-- The event a native fact records (empty for a pack fact's note).
function util.event(f: G.Fact): { [string]: any }
	local p = f.payload :: any
	return if type(p.event) == "table" then p.event else {}
end

-- A fact's note (empty for a native event's fact).
function util.note(f: G.Fact): string
	local p = f.payload :: any
	return if type(p.note) == "string" then p.note else ""
end

-- A fact's `amount`, or 0.
function util.amount(f: G.Fact): number
	return f.amount or 0
end

-- The newest fact matching `q` before `tick` (exclusive).
function util.before(ctx: G.Ctx, q: G.HistoryQuery, tick: number): G.Fact?
	if tick < 1 then
		return nil
	end
	local q2 = table.clone(q)
	q2["until"] = tick - 1
	return ctx.history:last(q2)
end

-- Facts matching `q` after fact `after` (nil: all), oldest first, paging past the
-- 50-fact cap up to `most` facts.
function util.find_all(ctx: G.Ctx, q: G.HistoryQuery, after: string?, most: number): { G.Fact }
	local out: { G.Fact } = {}
	local cursor = after
	while #out < most do
		local page = table.clone(q)
		page.after = cursor
		page.limit = 50
		local got = ctx.history:find(page)
		for _, f in ipairs(got) do
			table.insert(out, f)
		end
		if #got < 50 then
			break
		end
		cursor = got[#got].id
	end
	return out
end

-- Market levers, typed (a table literal passed to `table.insert` doesn't narrow).
function util.set_base(station: string, ware: string, cents: number): G.Lever
	return { lever = "set_base", station = station, ware = ware, base_price = math.floor(cents + 0.5) }
end
function util.scale(lever: "scale_base" | "scale_consumers", station: string, ware: string, factor: number): G.Lever
	if lever == "scale_base" then
		return { lever = "scale_base", station = station, ware = ware, factor = factor }
	end
	return { lever = "scale_consumers", station = station, ware = ware, factor = factor }
end
function util.add_consumers(station: string, ware: string, per_pulse: number): G.Lever
	return { lever = "add_consumers", station = station, ware = ware, per_pulse = per_pulse }
end
function util.set_stock(station: string, ware: string, stock: number): G.Lever
	return { lever = "set_stock", station = station, ware = ware, stock = stock }
end

-- Whether a ship is the player's own (the flagship or one of their tenders).
function util.players(ctx: G.Ctx, ship: string): boolean
	if ship == ids.PLAYER then
		return true
	end
	local c = ctx.space:craft(ship)
	return c ~= nil and c.owner == "player"
end

-- Records a pack fact under `why`. `kind` is written without the pack prefix.
function util.record(
	ctx: G.Ctx,
	why: G.Cause,
	kind: string,
	subjects: G.Subjects,
	amount: number?,
	note: string
)
	ctx.history:record({ kind = `core.{kind}`, subjects = subjects, amount = amount, note = note }, why)
end

-- Says an authored line in dock talk, citing `cite`, and remembers it was said
-- (`core.npc_line`: who, which line, where, and the cited fact's number).
function util.say(
	ctx: G.Ctx,
	why: G.Cause,
	line: string,
	npc: string,
	place: string,
	text: string,
	cite: string,
	choices: { G.TalkChoice }?
)
	ctx.talk:offer({ speaker = npc, place = place, text = text, cites = { cite }, choices = choices }, why)
	ctx.history:record({
		kind = "core.npc_line",
		subjects = { npc = npc, station = place, other = line },
		amount = util.seq(cite),
		note = `{line}: {text}`,
	}, why)
end

-- Whether `line` was already said citing fact `cite`.
function util.said(ctx: G.Ctx, line: string, cite: string): boolean
	local n = util.seq(cite)
	for _, f in ipairs(util.find_all(ctx, { kind = "core.npc_line", other = line }, nil, 200)) do
		if f.amount == n then
			return true
		end
	end
	return false
end

-- Whole credits for a line ("14.3").
function util.cr(cents: number): string
	local whole = math.floor(cents / 10 + 0.5) / 10
	if whole == math.floor(whole) then
		return string.format("%d", whole)
	end
	return string.format("%.1f", whole)
end

return util
