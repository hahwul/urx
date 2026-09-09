#!/usr/bin/env bash
# Space Grotesk is distributed variable-only, and its default instance is 300
# (Light), so ImageMagick renders Light unless a static instance is supplied.
# Regenerate the committed statics with:
#
#   python3 -m venv .venv && .venv/bin/pip install fonttools brotli
#   .venv/bin/python fonts/build-static.sh
set -euo pipefail
python3 - <<'PY'
from fontTools.ttLib import TTFont
from fontTools.varLib import instancer
for w, name in ((700, "Bold"), (500, "Medium")):
    instancer.instantiateVariableFont(TTFont("SpaceGrotesk.ttf"), {"wght": w}).save(f"SpaceGrotesk-{name}.ttf")
    print("wrote", f"SpaceGrotesk-{name}.ttf")
PY
