--!strict
-- The native canon hook enforces the authored graph, opener and beat step.
local G=require("@galaxy")
local canon={}
function canon.open(ctx:G.Ctx,why:G.Cause,id:string,_opener:string)
 ctx.canon:open(id,why)
end
return canon
