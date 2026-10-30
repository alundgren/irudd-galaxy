--!strict
local G=require("@galaxy")
local U=require("../lib/util")
local recipe=require("../lib/recipe")
local function found(ctx:G.Ctx,cite:string)
 local dock=U.calm(ctx)
 if dock==nil or not U.done(ctx,"beat.b13_first_tender") or ctx.counters:get("dockings")<50 then return end
 local resolved=0
 for _,id in ipairs({"beat.b09_hook_dispute","beat.b10_rumor","beat.b12_ice_shortage"}) do if U.done(ctx,id) then resolved+=1 end end
 if resolved<2 or ctx.beats:is_active("beat.b12_ice_shortage") then return end
 local anchor:G.Fact?=nil
 for _,f in ipairs(ctx.history:find({kind="mine",ship="ship.player",ware="ware.ice",limit=50})) do
  local rock=f.subjects.rock
  if rock~=nil and string.sub(rock,1,18)=="rock.shallows.ice." then anchor=f end
 end
 if anchor==nil then return end
 if ctx.store:get("progress","founded")~=true then
  local why=ctx:cause({summary="Wynn builds Meltwater Halt beside the pilot's ice workings",cites={anchor.id,cite}})
  ctx.beats:expect_docked(dock,why)
  ctx.kit:station({id="gm.sixth_station.halt",name="Meltwater Halt",region="region.lantern_shallows",position={x=300,y=850},dock_radius=70,owner="faction.wharf_cooperative",services={"market","dockmaster"},build={stage=1,parts={}}},recipe,{id="gm.sixth_station.wynn",name="Wynn Halloran",role="dockmaster",location="gm.sixth_station.halt",faction="faction.wharf_cooperative"},{["ware.ice"]={base_price=900,price=900,min_price=540,max_price=1260,stock=60,target=100,production=2,role="sells"},["ware.refined_plate"]={base_price=4000,price=4000,min_price=2400,max_price=5600,stock=40,target=40,role="buys"},["ware.rations"]={base_price=2000,price=2000,min_price=1200,max_price=2800,stock=40,target=40,role="buys"}},why)
  ctx.store:set("progress","founded",true,why)
  U.fact(ctx,why,"gm.sixth_station.station_founded","station.lantern_wharf","npc.ilse_marrow","Wynn built Meltwater Halt near the pilot's Rimewater workings")
  U.say(ctx,why,"Ilse: Wynn has opened Meltwater Halt by your ice workings. Its tanks and lamp frame came from the station kit.","npc.ilse_marrow","station.lantern_wharf",{anchor.id,cite})
 elseif dock=="gm.sixth_station.halt" and ctx.store:get("progress","stocked")~=true then
  local why=ctx:cause({summary="Wynn opens the Halt's ice tanks and trading hatch",cites={anchor.id,cite}})
  ctx.beats:expect_docked(dock,why)
  ctx.store:set("progress","stocked",true,why)
  U.say(ctx,why,"Wynn: I remember those Rimewater rocks you cut. The ice tanks are open; bring plate or rations when you pass.","gm.sixth_station.wynn","gm.sixth_station.halt",{anchor.id,cite})
  U.say(ctx,why,"Wynn: Slow work, sound tanks. Your ice-cutting showed us where a halt belonged.","gm.sixth_station.wynn","gm.sixth_station.halt",{anchor.id,cite})
  U.say(ctx,why,"Wynn: I count rocks by name. I keep the story of your Rimewater workings beside the market book.","gm.sixth_station.wynn","gm.sixth_station.halt",{anchor.id,cite})
 end
end
return G.script({id="main",on={dock=function(ev:G.DockEvent,ctx:G.Ctx) found(ctx,ev.fact) end,session_start=function(ev:G.SessionStartEvent,ctx:G.Ctx) found(ctx,ev.fact) end}})

