--!strict
local G = require("@galaxy")
local U = require("../lib/util")

local function visit(ctx:G.Ctx,cite:string)
 if U.calm(ctx)~="station.cinder_hook" or not ctx.mechanics:is_on("cap.loops") or ctx.counters:get("rocks_depleted")<10 then return end
 if ctx.store:get("progress","taught")==true then
  if ctx.store:get("progress","third")~=true and ctx.history:count({kind="tagged",ship="ship.player"})>=3 then
   local tag=ctx.history:last({kind="tagged",ship="ship.player"})
   if tag~=nil then
    local why=ctx:cause({summary="Juno notices the pilot's third rock lamp",cites={tag.id}})
    ctx.beats:expect_docked("station.cinder_hook",why)
    ctx.store:set("progress","third",true,why)
    U.say(ctx,why,"Three lamps of your own. The Cinder Belt is beginning to look like a place you know.","npc.juno_brask","station.cinder_hook",{tag.id})
   end
  end
  return
 end
 local demonstration=ctx.history:last({kind="tagged",ship="gm.rock_tagging.magpie"})
 if ctx.space:craft("gm.rock_tagging.magpie")==nil and demonstration==nil then
  local why=ctx:cause({summary="Juno launches Magpie for a rock-lamp demonstration",cites={cite}})
  ctx.beats:expect_docked("station.cinder_hook",why)
  ctx.space:put_craft({id="gm.rock_tagging.magpie",name="Magpie",npc="npc.juno_brask",hull="hull.wren",station="station.cinder_hook"},why)
  U.say(ctx,why,"Watch Magpie light a Cinder Belt rock before it mines. Come back and I will teach you the lamps.","npc.juno_brask","station.cinder_hook",{cite})
  return
 end
 if demonstration==nil then return end
 cite=demonstration.id
 local why=ctx:cause({summary="Juno offers to teach rock lamps at Cinder Hook",cites={cite}})
 ctx.beats:expect_docked("station.cinder_hook",why)
 ctx.talk:offer({speaker="npc.juno_brask",place="station.cinder_hook",cites={cite},text="A lamp marks a rock for your loops. Shall I show you how?",choices={{id="gm.rock_tagging.learn",text="Teach me rock lamps"}}},why)
 ctx.map:note({speaker="npc.juno_brask",place="station.cinder_hook",cites={cite},text="Juno teaches rock lamps at Cinder Hook."},why)
end
return G.script({id="main",on={dock=function(ev:G.DockEvent,ctx:G.Ctx) visit(ctx,ev.fact) end,session_start=function(ev:G.SessionStartEvent,ctx:G.Ctx) visit(ctx,ev.fact) end,
 reply=function(ev:G.ReplyEvent,ctx:G.Ctx)
  if ev.npc~="npc.juno_brask" or ev.choice~="gm.rock_tagging.learn" or U.calm(ctx)~="station.cinder_hook" or not ctx.mechanics:is_on("cap.loops") or ctx.counters:get("rocks_depleted")<10 or ctx.store:get("progress","taught")==true or ctx.history:last({kind="tagged",ship="gm.rock_tagging.magpie"})==nil then return end
  local why=ctx:cause({summary="Juno teaches rock tagging and sends Magpie to demonstrate",cites={ev.fact}})
  ctx.beats:expect_docked("station.cinder_hook",why)
  ctx.mechanics:teach({mechanic="mech.rock_tagging",teacher="npc.juno_brask",slots={"slot.key_t","slot.right_click","slot.hud_verb"}},why)
  ctx.store:set("progress","taught",true,why)
  U.say(ctx,why,"Press T beside a rock to tend its lamp. Six lamps fit the kit; your tagged field orders choose the richest lit rock.","npc.juno_brask","station.cinder_hook",{ev.fact})
 end}})
