# Working on Bank Bobbery (Rust)

A single-crate Bevy 0.19 game (same Bevy as `../bims2_bevy`, so builds can
run `--offline` from the shared cargo cache). `README.md` covers what the
game is. This file covers how to work on it.

## Code style

- Keep the code clean and short: the fewest lines that do the job clearly.
  No speculative helpers, layers or options.
- Minimal comments: only what a future reader (you) could not get from the
  code itself, such as a non-obvious reason or a trap. No narration, no
  restating the code, no long doc headers.
- Lightweight tests: a few fast ones that pin what matters (the movement's
  numbers, the map's reachability). No broad or slow test suites.

## Build, test, look

- Work inside `nix-shell shell.nix`, which provides cargo, the runtime libs
  and weston. `cargo build --offline`, `cargo test --offline`,
  `cargo clippy --offline --all-targets` (keep it warning-free), and
  `rustfmt --edition 2024 src/*.rs`.
- `cargo build` proves nothing about the picture, so look at a screenshot:
  `BB_SEED=6 BB_SMOKE_FRAMES=150 BB_SCREENSHOT=shot.png ./hidden target/debug/bankbobbery`.
  Smoke frames are paced at 60 fps. The shader compiles during the first
  second or so, so a screenshot taken before about frame 90 may be missing
  layers.
- To check the frame rate, add `BB_UNPACED=1` and read the "smoke run: …
  fps" line. On the RX 7800 XT, in a dev build, it is about 800 fps at
  1400×900 (720 with the flashlight off, 760 with it on in a dark room).
- `SEED=n cargo test print_map -- --ignored --nocapture` prints a map as
  ASCII (`#` brick, `=` drywall, `V` vault, `:` glass, `D` door, `o`
  furniture, letters for rooms).

## Where things are

| file | what it does |
| --- | --- |
| `src/movement.rs` | bims' steered walk: `Body`, `Steer`, `Solids`, `Aabb::push_out_round`. Pure arithmetic, stepped at 60 Hz. |
| `src/map.rs` | the generator: tiles, rooms, props, lamps; `solids()` for movement, `occluders()` for the ray tracer |
| `src/paint.rs` | `Painter` (vertex-coloured triangles) and every prop's and the robber's picture |
| `src/render.rs` | `WorldMaterial`, the three grid textures, spawning a map, writing the robber, the flashlight and the lamps each frame |
| `src/shaders/world.wgsl` | the per-pixel ray tracing (lamp and flashlight shadows), the procedural floors and walls |
| `src/player.rs` | input → `Controls` (PreUpdate), the body's fixed step (FixedUpdate), the picture blended between steps, camera, zoom |
| `src/dev.rs` | the `BB_*` switches for headless runs |

## Things easy to get wrong

- **Movement numbers are bims'.** `MARCH_SPEED` 216, `ACCEL` 330 × 4
  under the keys, instant stop, `SPRINT` 1.8, the roll 195 / 0.38 s / 1.1 s,
  `BODY_MARGIN` 23, `CORNER_ROUND` 1.5 × margin, `TILE` 52. The tests in
  `movement.rs` pin the feel. Change them only on purpose.
- **An opaque box must lie inside a solid one.** The flashlight's rays start
  at the robber's hand, so a body inside an opaque box would light nothing.
  Make every opaque prop solid; a test enforces this.
- **Sight** (`seen`): three rays from the eyes, blocked by walls and tall
  furniture, within 12 tiles in a lit room or outside and 5 in a dark one
  (`Map::lit`, in `extra.a`). Per room, not per pixel: by the light on each
  spot, far lamp pools showed as islands. Inside the flashlight's cone (`beam`) sight
  is the full 12 however dark it is. The flashlight's light (`torch_at`)
  casts its own shadows. Crouched (`misc.w`), the first half-height object
  a sight ray crosses hides what lies more than 2 tiles past it
  (`trace(.., low)`, the `low_boxes` texture, ids in `extra.b`). Which
  props are half height is `PropKind::is_low`.
- **What skips what in a trace.** A ray ignores the box of the object its
  target pixel is on (a cabinet's whole top is seen once its front is) and
  the box its source stands in (a lamp over furniture). For a pixel on a
  wall top, the trace is redone from the floor just past the wall's
  nearest open face or open diagonal (`wall_face`, chessboard distance, so
  each side's half-tile band meets the next square). Without that, rays
  graze along neighbouring wall tiles and draw saw-tooth shadows on the
  walls. Wall-top traces ignore furniture (`Spot.top`): it is lower than
  the wall, and that point often lies inside a shelf against it. Floor
  pixels under an object never skip it (`ground`), which is why a round
  pillar's square box darkens its plinth's corners.
- **Reserved WGSL words.** `from`, `target`, `filter`, `set`, `type` and
  `meta` are reserved, so naga refuses them as identifiers.
- **The tile textures are `Rgba8Unorm`, not sRGB.** They hold bytes (kinds,
  flags, box bounds) and are read with `textureLoad` × 255. Colours in the
  shader are written in sRGB and converted with `srgb()`. Vertex colours
  are linear (`paint::Hex::paint`).
- **Glowing vertices** (UV x = 1) ignore light altogether, so keep them for
  things that are lights: the street lamps' heads and the flashlight's lens.
  Screens, LEDs and the like are ordinary paint, or they shine in a dark
  room. Glow can go past white for the bloom.
- **Everything spawned for a map carries `WorldPart`.** R despawns all of
  them and builds the next map.
