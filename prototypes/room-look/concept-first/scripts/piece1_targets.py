"""Which blockout objects get baked, and which count as foreground (shared by piece1, matte, relight)."""
TARGETS = ["wall_back", "pillar_teal", "arches", "board_frame", "lamp_shade", "lamp_arm_a", "lamp_arm_b",
           "wall_left", "porthole_tube", "porthole_bezel"] + \
          [f"board_slot_{i}{j}" for i in range(6) for j in range(3)]
FOREGROUND = ("bram_", "plant_", "chair_", "book", "sign", "mug", "rack", "case_", "desk_")
# piece 5: the whole room goes through the same bake (everything the concept camera sees but the floor)
ROOM = ["desk_top", "desk_front", "book", "sign", "mug", "rack", "case_low", "case_high", "plant_pot",
        "plant_leaves", "bram_head", "bram_cap", "bram_torso", "bram_arm_l_up", "bram_arm_l_lo",
        "bram_arm_r_up", "bram_arm_r_lo", "chair_back"]
TARGETS = TARGETS + ROOM
