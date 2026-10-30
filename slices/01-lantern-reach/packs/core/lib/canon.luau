--!strict
-- Ancients chapter 1: the canon graph (content/06-game-master.md). The canon hook
-- (`canon.read`/`canon.write`) is still reserved, so the graph is core data kept here
-- and a node's status is read from history: a sealed node is open once a
-- `core.canon_opened` fact names it. Only its named opener may open it; core never
-- names a game-master ID (a `pack` opener is any dependent pack, after b15).
local G = require("@galaxy")

export type Status = "locked" | "sealed" | "question" | "forbidden"
export type Node = {
	id: string,
	status: Status,
	statement: string,
	-- Who may open a sealed node: a core beat step (`core:b15.1`), `pack`, or nil.
	opener: string?,
}
export type Edge = { from: string, to: string, rel: "supports" | "relates" | "raises" | "explains" }

local canon = {}

canon.nodes = {
	{
		id = "canon.lantern_makers",
		status = "locked",
		statement = "Long before the Reach was settled, a people the Lamplighters call the Lantern-makers built the lane currents. Nobody knows what they looked like.",
	},
	{
		id = "canon.lane_lanterns",
		status = "locked",
		statement = "The lamps along the lanes are Lantern-maker work. The Lamplighter Society was founded to keep them lit, and doesn't know how they work.",
	},
	{
		id = "canon.glass_drift_lens",
		status = "locked",
		statement = "The Glass Drift is the wreck of one great lens. Glassite is its dust.",
	},
	{
		id = "canon.no_fragment_trade",
		status = "locked",
		statement = "The Society never buys or sells fragments. It keeps them, or returns them to whoever found them.",
	},
	{
		id = "canon.fragment_is_lens_shard",
		status = "sealed",
		statement = "The fragment is an unbroken shard of the great lens, not dust. It is warm and can't be scratched.",
		opener = "core:b15.1",
	},
	{
		id = "canon.fragments_sing",
		status = "sealed",
		statement = "Carried through a lane current, a fragment sings in tune with the lane lamps.",
		opener = "core:b15.2",
	},
	{
		id = "canon.second_fragment",
		status = "sealed",
		statement = "The fragment's song is loudest when facing Brassmouth Narrows. (A direction only, never a location.)",
		opener = "pack",
	},
	{
		id = "canon.lens_purpose",
		status = "question",
		statement = "What the great lens was for.",
	},
	{
		id = "canon.who_broke_the_lens",
		status = "question",
		statement = "Who or what broke it.",
	},
	{
		id = "canon.no_living_ancients",
		status = "forbidden",
		statement = "Any claim that a Lantern-maker is alive or present in the Reach, or that an Ancients object grants power over ships, minds or prices.",
	},
} :: { Node }

canon.edges = {
	{ from = "canon.lantern_makers", to = "canon.lane_lanterns", rel = "supports" },
	{ from = "canon.lantern_makers", to = "canon.glass_drift_lens", rel = "supports" },
	{ from = "canon.glass_drift_lens", to = "canon.fragment_is_lens_shard", rel = "supports" },
	{ from = "canon.fragment_is_lens_shard", to = "canon.fragments_sing", rel = "supports" },
	{ from = "canon.fragments_sing", to = "canon.second_fragment", rel = "supports" },
	{ from = "canon.fragments_sing", to = "canon.lane_lanterns", rel = "relates" },
	{ from = "canon.glass_drift_lens", to = "canon.lens_purpose", rel = "raises" },
	{ from = "canon.glass_drift_lens", to = "canon.who_broke_the_lens", rel = "raises" },
	{ from = "canon.no_fragment_trade", to = "canon.glass_drift_lens", rel = "explains" },
} :: { Edge }

function canon.node(id: string): Node
	for _, n in ipairs(canon.nodes) do
		if n.id == id then
			return n
		end
	end
	error(`no canon node {id}`)
end

-- Whether a node is open: locked from the start, or a sealed one a fact has opened.
function canon.is_open(ctx: G.Ctx, id: string): boolean
	local n = canon.node(id)
	if n.status == "locked" then
		return true
	end
	if n.status ~= "sealed" then
		return false
	end
	return ctx.history:count({ kind = "core.canon_opened", other = id }) > 0
end

-- Opens a sealed node as `opener` (`core:b15.1`): records `core.canon_opened`. Any
-- other opener, or a node that isn't sealed, is an authoring error.
function canon.open(ctx: G.Ctx, why: G.Cause, id: string, opener: string)
	local n = canon.node(id)
	if n.status ~= "sealed" or n.opener ~= opener then
		error(`{opener} can't open {id}`)
	end
	ctx.history:record({
		kind = "core.canon_opened",
		subjects = { other = id },
		note = n.statement,
	}, why)
end

return canon
