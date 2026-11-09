#!/bin/sh
# Rebuild face_fit.blend and every checkpoint image from the Bram lab source.
#   BLENDER=<path to Blender 5.3> sh build_all.sh
# Run from this folder. The Bram lab source is only read.
set -e
B=${BLENDER:-blender}
PY="$B -b --factory-startup --python-exit-code 1 --python"
ROOT=$(cd ../../.. && pwd)
run() { "$B" -b --disable-autoexec face_fit.blend --python-exit-code 1 --python "$@"; }
"$B" -b --disable-autoexec ../blender-bram-lab/bram_bust.blend --python-exit-code 1 \
    --python scripts/make_face_file.py -- "$PWD/face_fit.blend"
run scripts/fit_camera.py
run scripts/project_paint.py -- "$ROOT"
run scripts/fit_geometry.py
run scripts/project_paint.py -- "$ROOT" Fit_Concept
run scripts/build_look.py
run scripts/render_checkpoint.py
run scripts/landmark_views.py -- renders/work A
run scripts/landmark_views.py -- renders/work C Fit_Concept
run scripts/set_arm.py -- B
run scripts/report_budget.py
"$B" -b --factory-startup --disable-autoexec face_fit.blend --python-exit-code 1 --python scripts/clean_for_repo.py
"$B" -b --disable-autoexec face_fit.blend --python-exit-code 1 --python validate_blend.py
# 2D steps (numpy only) run in Blender's own Python through a tiny wrapper
for step in "overlay.py $ROOT concept" \
            "overlay.py $ROOT fit data/camera_fit.json cp1_A renders/work/A_clay_clay_concept.png" \
            "overlay.py $ROOT fit data/camera_fit_C.json cp1_C renders/work/C_clay_clay_concept.png" \
            "make_sheets.py $ROOT cp1"; do
    "$B" -b --factory-startup --python-exit-code 1 --python scripts/run_py.py -- $step
done
