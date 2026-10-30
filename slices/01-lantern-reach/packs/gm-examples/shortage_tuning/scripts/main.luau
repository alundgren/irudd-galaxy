--!strict
local G = require("@galaxy")
local U = require("../lib/util")

-- Reconstruct FIFO ice lots, preserving provenance through ship transfers.
-- Purchased ice never counts as the pilot's Rimewater ice.
type Lots = {items: {{n:number, rime:boolean}}, first:number}
local function tune(ctx: G.Ctx)
 local dock = U.calm(ctx)
 if dock == nil or ctx.beats:state("beat.b12_ice_shortage") ~= nil then return end
 local inventory: {[string]:Lots} = {}
 local function cargo(ship:string):Lots
  if inventory[ship]==nil then inventory[ship]={items={},first=1} end
  return inventory[ship]
 end
 local totals: {[string]:number} = {}
 local cites: {string} = {}
 local cursor: string? = nil
 while true do
  local facts = ctx.history:find({ware="ware.ice",after=cursor,limit=50})
  if #facts == 0 then break end
  for _,f in ipairs(facts) do
   local n = f.amount or 0
   local ship=f.subjects.ship or "unknown"
   local hold=cargo(ship)
   if f.kind == "mine" or f.kind == "buy" then
    local rock = f.subjects.rock or ""
    table.insert(hold.items,{n=n,rime=ship=="ship.player" and f.kind=="mine" and string.sub(rock,1,18)=="rock.shallows.ice."})
   elseif f.kind == "sell" or f.kind == "deliver" or f.kind=="transfer" then
    local rime=0
    local target=if f.kind=="transfer" and f.subjects.other~=nil then cargo(f.subjects.other) else nil
    while n>0 and hold.first<=#hold.items do
     local lot=hold.items[hold.first]
     local take=math.min(n,lot.n)
     if lot.rime then rime+=take end
     if target~=nil then table.insert(target.items,{n=take,rime=lot.rime}) end
     lot.n-=take; n-=take
     if lot.n==0 then hold.first+=1 end
    end
    if target~=nil and n>0 then table.insert(target.items,{n=n,rime=false}) end
    local station=f.subjects.station
    if ship=="ship.player" and f.kind=="sell" and station~=nil and rime>0 then
     totals[station]=(totals[station] or 0)+rime
     table.insert(cites,f.id)
     if #cites>40 then table.remove(cites,1) end
    end
   end
   cursor=f.id
  end
 end
 if #cites==0 then return end
 local ranked={"station.lantern_wharf","station.cinder_hook","station.glass_drift_observatory"}
 local rank: {[string]:number} = {}
 for i,s in ipairs(ranked) do rank[s]=i end
 table.sort(ranked,function(a:string,b:string): boolean
  local av,bv=totals[a] or 0,totals[b] or 0
  return if av==bv then rank[a]<rank[b] else av>bv
 end)
 local stations={"station.kettle_dock",ranked[1],ranked[2]}
 local sold=0
 for _,s in ipairs(stations) do sold+=totals[s] or 0 end
 local quota=math.clamp(math.floor(sold/20+0.5)*10,30,60)
 local signature=table.concat(stations,",")..tostring(quota)
 if ctx.store:get("progress","signature")==signature then return end
 local why=ctx:cause({summary="Odo traces the shortage to the pilot's Rimewater ice",cites=cites})
 ctx.beats:expect_docked(dock,why)
 ctx.pacing:set("shortage.rimewater_ice.stations",stations,why)
 ctx.pacing:set("shortage.rimewater_ice.way_a_quota",quota,why)
 ctx.store:set("progress","signature",signature,why)
 U.say(ctx,why,`Odo counted your Rimewater ice: Kettle and its two busiest buyers need {quota} relief ice. Brassmouth still sells it cheaply.`,"npc.odo_fenn","station.kettle_dock",cites)
 U.fact(ctx,why,"gm.shortage_tuned","station.kettle_dock","npc.odo_fenn","Odo traced Rimewater ice to its buyers")
end
return G.script({id="main",on={sell=function(ev:G.SellEvent,ctx:G.Ctx) tune(ctx) end,dock=function(_ev:G.DockEvent,ctx:G.Ctx) tune(ctx) end,session_start=function(_ev:G.SessionStartEvent,ctx:G.Ctx) tune(ctx) end}})

