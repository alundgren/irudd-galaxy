--!strict
local G = require("@galaxy")
local U = require("../lib/util")

local function open(ctx:G.Ctx,cite:string)
 if U.calm(ctx)~="station.glass_drift_observatory" or not U.done(ctx,"beat.b15_fragment") or ctx.canon:status("canon.second_fragment")=="open" then return end
 local fact=ctx.history:last({kind="beat_completed",other="beat.b15_fragment"})
 local cites={if fact~=nil then fact.id else cite}
 local why=ctx:cause({summary="Nell writes down the fragment's bearing",cites=cites})
 ctx.beats:expect_docked("station.glass_drift_observatory",why)
 ctx.canon:open("canon.second_fragment",why)
 local s=ctx.stations:get("station.glass_drift_observatory")
 local destination=ctx.stations:get("station.brassmouth_exchange")
 if s~=nil and destination~=nil then ctx.kit:overlay("gm.ancients_bearing.song",{region=s.region,center=s.position,radius=900,direction={x=destination.position.x-s.position.x,y=destination.position.y-s.position.y}},why) end
 U.say(ctx,why,"Nell hears the song strongest toward Brassmouth Narrows. A direction, not a promise of what lies there.","npc.nell_quillon","station.glass_drift_observatory",cites)
end
return G.script({id="main",on={dock=function(ev:G.DockEvent,ctx:G.Ctx) open(ctx,ev.fact) end,session_start=function(ev:G.SessionStartEvent,ctx:G.Ctx) open(ctx,ev.fact) end}})
