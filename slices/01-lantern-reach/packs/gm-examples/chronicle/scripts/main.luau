--!strict
local G = require("@galaxy")
local U = require("../lib/util")

-- What each kind of change reads as in the chronicle, by the kind's last segment.
-- Station growth reads its stage: stage 1 is the first greenhouse pane (Odo's "First
-- pane's up"), later stages are rings.
local kinds: {[string]:string} = {
 shortage_started="the ice shortage began",shortage_tuned="Odo traced the ice buyers",shortage_resolved="the ice shortage ended",territory_changed="the depot changed hands",border_moved="Hesper redrew the border",rumor_resolved="the Shoals story was settled",rumor_exposed="the planted claim was exposed",canon_opened="Nell recorded a fragment revelation",station_founded="a new station opened",mechanic_taught="a teacher shared a new verb",trait_earned="you picked up a new habit",prestige="you brought home a new hull",tender_acquired="a tender joined you",gm_change_applied="a game-master change arrived",
}
local function title(f:G.Fact,suffix:string):string?
 if suffix=="stage_built" then
  return if (f.amount or 0)<=1 then "the first greenhouse pane went up" else "a new greenhouse ring lit up"
 end
 return kinds[suffix]
end
-- Who writes each: the engine (bare kinds), core, or an example pack, each only in its
-- own namespace (the pack's whole ID).
local sources: {string} = {
 "trait_earned","prestige","tender_acquired","gm_change_applied",
 "core.stage_built","core.shortage_started","core.shortage_resolved","core.territory_changed","core.rumor_resolved","core.canon_opened","core.mechanic_taught",
 "gm.kettle_growth.stage_built","gm.shortage_tuning.shortage_tuned","gm.territory_map.border_moved","gm.compact_plant.rumor_exposed",
 "gm.ancients_bearing.canon_opened","gm.sixth_station.station_founded","gm.rock_tagging.mechanic_taught",
}
local function collect(ctx:G.Ctx)
 local dock=U.calm(ctx)
 if dock==nil then return end
 local saved=ctx.store:get("progress","cursor")
 local cursor=if type(saved)=="string" then saved else nil
 -- Filter at the history index, so thousands of mining ticks cannot hide news.
 -- Keep a bounded chronological page; later calm moments drain any backlog.
 local facts:{G.Fact}={}
 for _,kind in ipairs(sources) do
  for _,f in ipairs(ctx.history:find({kind=kind,after=cursor,limit=50})) do
   table.insert(facts,f)
  end
 end
 table.sort(facts,function(a:G.Fact,b:G.Fact):boolean
  return (tonumber(string.match(a.id,"%d+")) or 0)<(tonumber(string.match(b.id,"%d+")) or 0)
 end)
 while #facts>20 do table.remove(facts) end
 if #facts==0 then return end
 local why=ctx:cause({summary="Ilse adds witnessed changes to the Reach chronicle",cites={facts[#facts].id}})
 ctx.beats:expect_docked(dock,why)
 for _,f in ipairs(facts) do
  local suffix=string.match(f.kind,"([a-z_]+)$") or f.kind
  local what=title(f,suffix)
  if what~=nil then
   -- Tagged with where it happened, and with a person only if they're there: an
   -- entry about Kettle Dock is never "Ilse Marrow · Kettle Dock".
   local place=f.subjects.station or dock
   local npc=if f.subjects.npc~=nil then ctx.npcs:get(f.subjects.npc) else nil
   local person=if npc~=nil and npc.location==place then npc.id else nil
   local station=ctx.stations:get(place)
   local text="At "..(if station~=nil then station.name else "the Reach")..", "..what.."."
   ctx.chronicle:write({text=text,speaker=person,place=place,cites={f.id}},why)
   ctx.store:set("progress","last",text,why)
   ctx.store:set("progress","cite",f.id,why)
   ctx.store:set("progress","place",place,why)
  end
 end
 ctx.store:set("progress","cursor",facts[#facts].id,why)
end
return G.script({id="main",on={dock=function(_ev:G.DockEvent,ctx:G.Ctx) collect(ctx) end,session_start=function(_ev:G.SessionStartEvent,ctx:G.Ctx) collect(ctx) end,
 reply=function(ev:G.ReplyEvent,ctx:G.Ctx)
  if ev.npc~="npc.ilse_marrow" or ev.choice~="choice.what_changed" or U.calm(ctx)~="station.lantern_wharf" then return end
  local text=ctx.store:get("progress","last")
  local cite=ctx.store:get("progress","cite")
  -- Only what changed here or nearby: an entry from the Wharf's own region.
  local place=ctx.store:get("progress","place")
  local at=if type(place)=="string" then ctx.stations:get(place) else nil
  local wharf=ctx.stations:get("station.lantern_wharf")
  local near=at~=nil and wharf~=nil and at.region==wharf.region
  if near and type(text)=="string" and type(cite)=="string" and ctx.store:get("progress","recalled")~=cite and not ctx.talk:cited(cite) then
   local why=ctx:cause({summary="Ilse recalls the chronicle",cites={cite}})
   ctx.beats:expect_docked("station.lantern_wharf",why)
   ctx.store:set("progress","recalled",cite,why)
   ctx.talk:offer({text=text,speaker="npc.ilse_marrow",place="station.lantern_wharf",cites={cite}},why)
  end
 end}})
