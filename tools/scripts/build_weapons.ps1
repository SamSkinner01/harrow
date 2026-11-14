# Rebuild the weapon blockouts: tools/blender/weapons/*.py -> assets/models/weapons/*.glb
# Extra arguments pass through, e.g. --only tamarisk, or --preview out.png
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..\..")
$blender = if ($env:HARROW_BLENDER) { $env:HARROW_BLENDER } else { "blender" }
& $blender --background --factory-startup `
    --python tools/blender/weapons/build_weapons.py -- --out assets/models/weapons @args
exit $LASTEXITCODE
