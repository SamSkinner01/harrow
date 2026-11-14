# Rebuild the fortress kit: tools/blender/kit/*.py -> assets/models/kit/*.glb
# Extra arguments pass through, e.g. --only stone_wall_2m, or --preview out.png
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..\..")
$blender = if ($env:HARROW_BLENDER) { $env:HARROW_BLENDER } else { "blender" }
& $blender --background --factory-startup `
    --python tools/blender/build_kit.py -- --out assets/models/kit @args
exit $LASTEXITCODE
