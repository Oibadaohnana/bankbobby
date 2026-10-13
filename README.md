# Bank Bobbery (Rust + Bevy)

A top-down heist on foot, at night, in a bank nobody has built before. This
is the start of a Rust/Bevy remake of `../bankbobbery`. So far it is only
movement and sight. There is no gameplay yet.

- **Movement** is bims_bevy's steered walk, ported number for number
  (`src/movement.rs`). WASD walk, the mouse aims (the body faces the pointer
  at once, and walking away from it strides backwards), Shift sprints
  (1.8×, eased in over 0.1 s), and Space or Alt dodge-rolls (195 units in
  0.38 s, 1.1 s cooldown). Letting go stops you dead. Lone corners are
  rounded, so you slide round door frames instead of snagging on them.
- **The bank is generated** from a seed (`src/map.rs`). It sits on a city
  block: a road with the getaway van at the kerb, a pavement, a car park
  and trees. Inside are a marble lobby with pillars, a queue and the teller
  counter, a corridor with a side door, and back rooms split by a BSP:
  open-plan office, manager, break room, washrooms, server room, security,
  archive and a steel **vault** with one door. A flood fill makes sure
  every room can be reached.
- **Sight and light are ray traced per pixel** (`src/shaders/world.wgsl`).
  Every pixel traces rays through the tile grid to the robber's eyes and to
  each lamp in reach, and the walls and tall furniture (cabinets, shelves,
  racks, pillars, the van, trees) cast exact shadows. Glass lets sight and
  light through. Following bims' rule, you see 8 tiles in the dark, further
  in light, and never more than 14. Anything you can't see is drawn under a
  grey-blue fog.
- **All the graphics are its own.** Floors and walls are painted
  procedurally in the shader. The furniture and the robber (striped shirt,
  beanie, mask, swag bag, crowbar) are vertex-coloured meshes built in
  code (`src/paint.rs`). There are no image assets.

## Run it

```bash
nix-shell            # rust, cargo, the Vulkan/Wayland/X11 runtime libs, weston
cargo run            # a random bank
BB_SEED=1234 cargo run
```

Inside the game: **R** builds a new bank, the **mouse wheel** zooms, and
**F11** toggles full screen.

`cargo test` checks the movement's feel against bims' numbers. It also
checks that on 60 seeds every room is reachable, that the same seed always
gives the same bank, and that no opaque box sits where a body can stand.

## Looking at it without a window

```bash
BB_SEED=6 BB_SMOKE_FRAMES=150 BB_SCREENSHOT=shot.png ./hidden target/debug/bankbobbery
```

`./hidden` runs the game on a headless weston. The `BB_*` variables
(scripted keys, a fixed aim, the spawn tile, the zoom, an unpaced frame-rate
run) are documented at the top of `src/dev.rs`.
