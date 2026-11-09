# Run one of the numpy-only lab scripts inside Blender's Python, with its arguments:
#   blender -b --factory-startup --python scripts/run_py.py -- overlay.py <args...>
import os
import runpy
import sys

args = sys.argv[sys.argv.index("--") + 1:]
here = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, here)
sys.argv = [os.path.join(here, args[0])] + args[1:]
runpy.run_path(sys.argv[0], run_name="__main__")
