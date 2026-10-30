--!strict
local G = require("@galaxy")
local U = require("../lib/util")

local function draw(ctx:G.Ctx,cite:string)
 local dock=U.calm(ctx)
 if dock==nil then return end
 local owner=ctx.territory:station_owner("station.cinder_hook")
 local resolved=U.done(ctx,"beat.b09_hook_dispute")
 local key=if resolved then owner or "unclaimed" else "contested"
 if ctx.store:get("progress","owner")==key then return end
 local fact=ctx.history:last({kind="core.territory_changed"})
 local cites={if fact~=nil then fact.id else cite}
 local why=ctx:cause({summary="Hesper redraws the Reach's borders",cites=cites})
 ctx.beats:expect_docked(dock,why)
 local zones={
  {id="gm.territory_map.shallows",station="station.lantern_wharf",owner="faction.wharf_cooperative",radius=1000},
  {id="gm.territory_map.narrows",station="station.brassmouth_exchange",owner="faction.tollway_compact",radius=850},
  {id="gm.territory_map.glass",station="station.glass_drift_observatory",owner="faction.lamplighter_society",radius=650},
 }
 for _,z in ipairs(zones) do
  local s=ctx.stations:get(z.station)
  if s~=nil then ctx.kit:overlay(z.id,{region=s.region,center=s.position,radius=z.radius,owner=z.owner},why) end
 end
 local s=ctx.stations:get("station.cinder_hook")
 if s~=nil then ctx.kit:overlay("gm.territory_map.hook",{region=s.region,center=s.position,radius=700,owner=if resolved then owner else nil,contested=not resolved},why) end
 ctx.store:set("progress","owner",key,why)
 U.say(ctx,why,if resolved then "Hesper's border now takes in Cinder Hook under its new banner." else "Hesper marks the Cinder Hook border as contested; the depot's lamps stay open.","npc.hesper_lune","station.cinder_hook",cites)
 U.fact(ctx,why,"gm.border_moved","station.cinder_hook","npc.hesper_lune","Hesper drew the depot's current border")
end
return G.script({id="main",on={beat_completed=function(ev:G.BeatCompletedEvent,ctx:G.Ctx) draw(ctx,ev.fact) end,dock=function(ev:G.DockEvent,ctx:G.Ctx) draw(ctx,ev.fact) end,session_start=function(ev:G.SessionStartEvent,ctx:G.Ctx) draw(ctx,ev.fact) end}})

