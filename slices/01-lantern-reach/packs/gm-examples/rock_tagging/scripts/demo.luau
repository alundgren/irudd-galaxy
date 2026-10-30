--!strict
local G=require("@galaxy")
local U=require("../lib/util")
local function demo(ctx:G.Ctx,cite:string)
 if U.calm(ctx)~="station.cinder_hook" or ctx.space:craft("gm.rock_tagging.magpie")==nil or ctx.store:get("progress","started")==true then return end
 local why=ctx:cause({summary="Juno demonstrates a lamp before offering the verb",cites={cite}})
 ctx.beats:expect_docked("station.cinder_hook",why)
 ctx.orders:give("gm.rock_tagging.magpie",{{verb="tag",rock="rock.cinder.iron.01"},{verb="mine",rock="rock.cinder.iron.01"},{verb="dock",station="station.cinder_hook"}},why)
 ctx.store:set("progress","started",true,why)
 U.say(ctx,why,"See Magpie's course? A lamp first, then a hold of ore. Come back next visit and I'll show you.","npc.juno_brask","station.cinder_hook",{cite})
end
return G.script({id="demo",on={dock=function(ev:G.DockEvent,ctx:G.Ctx) demo(ctx,ev.fact) end,session_start=function(ev:G.SessionStartEvent,ctx:G.Ctx) demo(ctx,ev.fact) end}})
