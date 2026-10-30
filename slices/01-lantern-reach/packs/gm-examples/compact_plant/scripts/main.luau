--!strict
local G = require("@galaxy")
local U = require("../lib/util")

local function change(ctx:G.Ctx,cite:string,docking:boolean)
 local dock=U.calm(ctx)
 if dock==nil then return end
 local rumor=ctx.rumors:get("rumor.ember_shoals_vein")
 if rumor==nil then
  local coop=ctx.history:sum({kind="core.dispute_tally",faction="faction.wharf_cooperative"})
  local compact=ctx.history:sum({kind="core.dispute_tally",faction="faction.tollway_compact"})
  local lean=ctx.territory:station_owner("station.cinder_hook")=="faction.wharf_cooperative" or coop>compact
  if ctx.store:get("progress","tuned")==lean then return end
  local f=ctx.history:last({kind="core.dispute_tally"}) or ctx.history:last({kind="core.territory_changed"})
  if f==nil then return end
  local why=ctx:cause({summary="Pip checks who supplied the Shoals story",cites={f.id}})
  ctx.beats:expect_docked(dock,why)
  ctx.pacing:set("rumor.ember_shoals.override",if lean then {"false","faction.tollway_compact","a Compact surveyor off the Ninefold"} else "",why)
  ctx.store:set("progress","tuned",lean,why)
  U.say(ctx,why,"Pip's surveyor has a story about the Shoals. Check the claim buoy; its log records who sent it.","npc.pip_calloway","station.brassmouth_exchange",{f.id})
 elseif rumor.planted_by=="faction.tollway_compact" then
  local proof=ctx.history:last({kind="survey_buoy_read",other="gm.compact_plant.buoy"})
  if proof~=nil then
   -- Exposure happens once, wherever the pilot docks next. The reward is Rook's to
   -- offer, so it is offered at Cinder Hook, every docking there until it is claimed.
   local exposed=ctx.store:get("progress","exposed")==true
   local offer=docking and dock=="station.cinder_hook" and ctx.history:last({kind="survey_reward_claimed",other="gm.compact_plant.buoy"})==nil
   if exposed and not offer then return end
   local why=ctx:cause({summary=if exposed then "Rook still holds the Compact proof reward" else "Rook records the Compact survey log",cites={proof.id}})
   ctx.beats:expect_docked(dock,why)
   if not exposed then
    ctx.store:set("progress","exposed",true,why)
    U.fact(ctx,why,"gm.compact_plant.rumor_exposed","station.cinder_hook","npc.rook_adeyemi","The Compact planted the stale Shoals claim")
    U.say(ctx,why,"Rook reads the buoy log: the Compact planted that stale claim. The three iron rocks are still yours to work.","npc.rook_adeyemi","station.cinder_hook",{proof.id})
   end
   if offer then
    ctx.talk:offer({speaker="npc.rook_adeyemi",place="station.cinder_hook",text="Rook: That log is proof. The Cooperative owes you 300 credits for exposing it.",cites={proof.id},choices={{id="gm.compact_plant.claim",text="Someone planted the Shoals rumor"}}},why)
   end
  elseif ctx.store:get("progress","buoy")~=true then
   local why=ctx:cause({summary="Pip marks the survey buoy beside the Shoals claim",cites={cite}})
   ctx.beats:expect_docked(dock,why)
   ctx.kit:buoy("gm.compact_plant.buoy",{region="region.cinder_drift",position={x=6700,y=1900},radius=40,planted_by="faction.tollway_compact",reward={person="npc.rook_adeyemi",choice="gm.compact_plant.claim",credits=300,standing={["faction.wharf_cooperative"]=5,["faction.tollway_compact"]=-3}}},why)
   ctx.store:set("progress","buoy",true,why)
   U.say(ctx,why,"Pip marks a Compact survey buoy at Ember Shoals. Press Space beside it to read the claim log.","npc.pip_calloway","station.brassmouth_exchange",{cite})
  end
 end
end
return G.script({id="main",on={beat_completed=function(ev:G.BeatCompletedEvent,ctx:G.Ctx) change(ctx,ev.fact,false) end,dock=function(ev:G.DockEvent,ctx:G.Ctx) change(ctx,ev.fact,true) end,session_start=function(ev:G.SessionStartEvent,ctx:G.Ctx) change(ctx,ev.fact,true) end}})

