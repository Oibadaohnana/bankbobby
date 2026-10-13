# Bank Bobbery (Rust + Bevy)

A top-down heist on foot, at night, in a bank nobody has built before. This
is the start of a Rust/Bevy remake of `../bankbobbery`. So far it is only
movement and light. There is no gameplay yet.

- **Movement** is bims_bevy's steered walk, ported number for number
  (`src/movement.rs`). WASD walk, the mouse aims (the body faces the pointer
  at once, and walking away from it strides backwards), Shift sprints
  (1.8×, eased in over 0.1 s), and Space or Alt dodge-rolls (195 units in
  0.38 s, 1.1 s cooldown). C crouches and stands: a little over half speed
  and no sprint, folded forward with his knees out in front. A roll goes
  head over heels. T switches the flashlight on and off. Letting go stops you
  dead. Lone corners are rounded, so you slide round door frames instead of
  snagging on them.
- **The bank is generated** from a seed (`src/map.rs`). It sits on a city
  block: a road with the getaway van at the kerb, a pavement, a car park
  and trees. Inside are a marble lobby with pillars, a queue and the teller
  counter, a corridor with a side door, and back rooms split by a BSP:
  open-plan office, manager, break room, washrooms, server room, security,
  archive and a steel **vault** with one door. A flood fill makes sure
  every room can be reached.
- **Light is ray traced per pixel** (`src/shaders/world.wgsl`). Every
  pixel traces rays through the tile grid to each lamp in reach, and the
  walls and tall furniture (cabinets, shelves, racks, pillars, the van,
  trees) cast exact shadows away from that lamp. The robber's
  **flashlight** is a light like any other: a cone from his hand, 15 tiles
  long, with a soft edge, throwing shadows away from him. With it off he
  throws none. Glass lets light through.
- **Sight is ray traced too.** You see only what the robber could see:
  walls and tall furniture block it, glass doesn't. You see 12 tiles into a
  room with its lights on (and outside), 5 into a dark one. Inside the
  flashlight's beam you see the full 12 tiles, as in a lit room. Anything
  out of sight is drawn under a grey-blue fog.
- **Half-height cover.** Desks, tables, the counter and sofas are half
  height. Crouched behind one you are in cover (the HUD says so), and you
  see only 2 tiles past it, though walls and tall things beyond still show
  over it. Nothing is looking for you yet; the cover is for the guards to
  come.
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

Inside the game: **T** switches the flashlight on and off, **R** builds a
new bank, the **mouse wheel** zooms (out to at most 14 tiles above and
below the robber), and **F11** toggles full screen.

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
