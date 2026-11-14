# Rebuild every operator:
#   tools/blender/characters/operators/*.toml -> assets/models/characters/*.glb
# Extra arguments pass through, e.g. --only auger, --preview DIR, --keep-blend DIR
#
# First time only: --setup installs MPFB 2.0.18 and the MakeHuman CC0 system
# assets into your Blender user configuration (downloads ~330 MB).
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..\..")
$blender = if ($env:HARROW_BLENDER) { $env:HARROW_BLENDER } else { "blender" }
if ($args.Count -gt 0 -and $args[0] -eq "--setup") {
    & $blender --background --factory-startup `
        --python tools/blender/characters/setup_mpfb.py
    exit $LASTEXITCODE
}
& $blender --background --factory-startup `
    --python tools/blender/build_characters.py -- --out assets/models/characters @args
exit $LASTEXITCODE
