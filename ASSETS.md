# External assets

Every asset not made from scratch in this repo. Only CC0 or clearly permissive
licences. Add a row before committing the asset.

| Asset | Source URL | Licence | Used in |
|---|---|---|---|
| Rustic Stone Wall (2k; diff, nor_gl, arm) by Dimitrios Savva | https://polyhaven.com/a/rustic_stone_wall | CC0 | `assets/textures/stone/`: structural walls, posts, parapets, stairs |
| Patterned Clay Plaster (2k) by Amal Kumar | https://polyhaven.com/a/patterned_clay_plaster | CC0 | `assets/textures/plaster/`: soft walls |
| Mixed Stone Tiles (2k) by Dimitrios Savva | https://polyhaven.com/a/mixed_stone_tiles | CC0 | `assets/textures/flagstone/`: stone floors, courtyard |
| Coast Sand 01 (2k) by Rob Tuytel | https://polyhaven.com/a/coast_sand_01 | CC0 | `assets/textures/sand/`: desert ground |
| Weathered Plank Siding (1k) by Dimitrios Savva | https://polyhaven.com/a/weathered_plank_siding | CC0 | `assets/textures/wood/`: crates, barricades, wooden floors, railings |
| Rusty Metal 02 (1k) by Rob Tuytel | https://polyhaven.com/a/rusty_metal_02 | CC0 | `assets/textures/metal/`: hatch frames |
| Hessian 230 (1k) by colormass, Rico Cilliers | https://polyhaven.com/a/hessian_230 | CC0 | `assets/textures/burlap/`: sandbags |

Textures are fetched by `tools/scripts/fetch_textures.py`.

Sounds: the footstep sound is synthesised in code (`src/player/footsteps.rs`);
no external audio yet.

## Tools (not shipped with the game)

| Tool | Licence | Notes |
|---|---|---|
| Blender 5.2.4 LTS | GNU GPL (source 2.0-or-later, bundled libs 3.0-compatible) | Files we create with it are ours. |
| MPFB 2.0.18 *(M5)* | GPL-3.0-or-later (add-on code) | Generated bodies use MakeHuman's CC0 base assets; only use add-on asset packs that are CC0. |
