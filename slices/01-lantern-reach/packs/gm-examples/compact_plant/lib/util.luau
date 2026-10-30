--!strict
local G = require("@galaxy")
local U = {}
function U.calm(ctx: G.Ctx): string?
 return ctx.beats:pilot().docked_at
end
function U.say(ctx: G.Ctx, why: G.Cause, text: string, speaker: string, place: string, cites: {string})
 local line: G.LineSpec = {text=text,speaker=speaker,place=place,cites=cites}
 ctx.talk:offer(line,why)
 ctx.map:note(line,why)
end
function U.fact(ctx: G.Ctx, why: G.Cause, kind: string, place: string, person: string, note: string)
 ctx.history:record({kind=kind,subjects={station=place,npc=person},note=note},why)
end
function U.done(ctx: G.Ctx, id: string): boolean
 return ctx.beats:is_done(id)
end
return U
