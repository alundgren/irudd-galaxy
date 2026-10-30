--!strict
-- Every stable ID the journey's scripts name, in one place. The data still uses some
-- provisional IDs from the foundation (content/08-contract.md lists the renames:
-- `faction.wharf_cooperative` for the spec's `faction.wharf_coop`, `ware.refined_plate`
-- for `ware.plate`, `field.shallows.ice` for Rimewater ...); when the data is renamed,
-- this file is the only script change.

local ids = {
	-- The player
	PLAYER = "ship.player",

	-- Stations
	WHARF = "station.lantern_wharf",
	KETTLE = "station.kettle_dock",
	HOOK = "station.cinder_hook",
	GDO = "station.glass_drift_observatory",
	BRASS = "station.brassmouth_exchange",

	-- Regions
	SHALLOWS = "region.lantern_shallows",
	CINDER = "region.cinder_drift",
	NARROWS = "region.brassmouth_narrows",

	-- Factions
	COOP = "faction.wharf_cooperative",
	COMPACT = "faction.tollway_compact",
	LAMP = "faction.lamplighter_society",

	-- Wares
	ICE = "ware.ice",
	ORE = "ware.iron_ore",
	PLATE = "ware.refined_plate",
	GLASSITE = "ware.glassite",
	RATIONS = "ware.rations",
	FRAGMENT = "ware.ancients_fragment",

	-- Fields (and the prefix of their rocks)
	RIMEWATER = "field.shallows.ice",
	RIMEWATER_ROCKS = "rock.shallows.ice.",
	TINROOT = "field.shallows.iron",
	CINDER_BELT = "field.cinder.iron",
	GLASS_DRIFT = "field.cinder.glassite",
	GLASS_DRIFT_ROCKS = "rock.cinder.glassite.",
	HOOKSIDE = "field.hookside",
	EMBER_SHOALS = "field.ember_shoals",
	EMBER_SHOALS_ROCKS = "rock.ember_shoals.",

	-- People
	ILSE = "npc.ilse_marrow",
	ORRIN = "npc.orrin_vale",
	ODO = "npc.odo_fenn",
	TAMSIN = "npc.tamsin_oddway",
	ROOK = "npc.rook_adeyemi",
	JUNO = "npc.juno_brask",
	SAFFRON = "npc.saffron_vey",
	PIP = "npc.pip_calloway",
	NELL = "npc.nell_quillon",
	HESPER = "npc.hesper_lune",

	-- Craft
	LITTLE_PAIL = "craft.little_pail",
	THIMBLE = "craft.thimble",
	LITTLE_MERCY = "craft.little_mercy",
	NINEFOLD = "craft.ninefold",
	EVEN_KEEL = "craft.even_keel",
	QUIET_LAMP = "craft.quiet_lamp",

	-- Hulls
	WREN = "hull.wren",
	TENDER = "hull.tender",

	-- Ledgers (delivery tallies)
	GREENHOUSE = "ledger.kettle_greenhouse",
	CONSIGNMENT = "ledger.hook_consignment",
	ICE_RELIEF = "ledger.ice_relief",
	CONDENSER = "ledger.kettle_condenser",
	LENS_TOWER = "ledger.observatory_lens",

	-- Situations, rumors, mechanics
	DISPUTE = "dispute.hook",
	SHORTAGE = "shortage.rimewater_ice",
	RUMOR = "rumor.ember_shoals_vein",
	TENDER_MECH = "mech.tender",

	-- Answers the player gives at the dock (`reply` events)
	SIDE_COOP = "choice.side_coop",
	SIDE_COMPACT = "choice.side_compact",
	SIDE_NONE = "choice.side_none",
	REST = "choice.rimewater_rest",
	ASK_ORRIN = "choice.ask_orrin",
	FRAGMENT_LEAVE = "choice.fragment_leave",
	FRAGMENT_KEEP = "choice.fragment_keep",
	WHAT_CHANGED = "choice.what_changed",

	-- Beats
	B01 = "beat.b01_undock",
	B02 = "beat.b02_mine_by_hand",
	B03 = "beat.b03_first_delivery",
	B04 = "beat.b04_first_sale",
	B05 = "beat.b05_pause",
	B06 = "beat.b06_nav_chip",
	B07 = "beat.b07_cinder_drift",
	B08 = "beat.b08_chasing_a_gap",
	B09 = "beat.b09_hook_dispute",
	B10 = "beat.b10_rumor",
	B11 = "beat.b11_orders",
	B12 = "beat.b12_ice_shortage",
	B13 = "beat.b13_first_tender",
	B14 = "beat.b14_pilot_changes",
	B15 = "beat.b15_fragment",
	B16 = "beat.b16_hullwrights_question",
	B17 = "beat.b17_coming_home",
}

return ids
