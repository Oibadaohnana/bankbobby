// The world's one shader: the ground (mode 0) and everything drawn as a
// vertex-coloured mesh over it (mode 1) — the furniture and the robber.
//
// Both are shaded the same way, per pixel:
//
//   * **Light.** Every lamp within reach of the pixel traces a ray to it
//     through the tile grid; what the ray crosses of the opaque boxes (the
//     walls and the tall furniture) is its shadow. Summed with the night's
//     ambient, moonlight outside.
//   * **Sight.** Three rays from the player's eyes — the middle and a
//     little either side, for a soft edge — trace to the pixel the same
//     way. A pixel is seen when they get there, it is within the view range,
//     and, in the dark, close: a body sees `dark` far in the dark and
//     further the more light there is, never past `view` (bims' rule).
//   * What is seen is drawn in its light, never quite black; what is not is
//     drawn under the fog: dim, grey and blue. The whole map is always
//     drawn — the fog hides who is there, not where the walls are.
//
// The grid is three textures, one texel a tile:
//   tiles:  floor kind, wall kind, tint, decoration flags (bytes)
//   occ:    the box inside the tile that stops a ray (0..1 of the tile)
//   extra:  the object the box belongs to (0: a wall), indoor or not
//
// A ray is the grid traversal (Amanatides & Woo): it visits every tile the
// segment crosses, in order, and adds up how far it runs inside each box.
// The object the pixel is on is never its own shadow — a cabinet's whole
// top is seen once its front is, and a wall is seen from the room it walls.

#import bevy_sprite::mesh2d_vertex_output::VertexOutput

struct Params {
    // x, y: the player's eyes; z: the view range; w: the dark range.
    eye: vec4<f32>,
    // x, y: the map in tiles; z: a tile in world units; w: the clock.
    map: vec4<f32>,
    // x: the mode (0 ground, 1 mesh); y: the lamps; z, w: unused.
    misc: vec4<f32>,
    // Two a lamp: (x, y, reach, power), (r, g, b, 0).
    lights: array<vec4<f32>, 256>,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> P: Params;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var tiles: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var occ: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var extra: texture_2d<f32>;

const FLOOR_GRASS: u32 = 0u;
const FLOOR_ASPHALT: u32 = 1u;
const FLOOR_SIDEWALK: u32 = 2u;
const FLOOR_PARKING: u32 = 3u;
const FLOOR_MARBLE: u32 = 4u;
const FLOOR_CARPET: u32 = 5u;
const FLOOR_WOOD: u32 = 6u;
const FLOOR_WASHROOM: u32 = 7u;
const FLOOR_CONCRETE: u32 = 8u;
const FLOOR_VAULT: u32 = 9u;
const FLOOR_LINO: u32 = 10u;
const FLOOR_RAISED: u32 = 11u;

const WALL_NONE: u32 = 0u;
const WALL_BRICK: u32 = 1u;
const WALL_DRYWALL: u32 = 2u;
const WALL_VAULT: u32 = 3u;
const WALL_GLASS: u32 = 4u;

const DECO_DOOR: u32 = 1u;
const DECO_LANE: u32 = 2u;
const DECO_BAY: u32 = 4u;
const DECO_ZEBRA: u32 = 8u;
const DECO_VAULT_DOOR: u32 = 16u;
const DECO_RUG: u32 = 32u;

// Half the gap between the eyes, in world units: the soft edge's width.
const EYE_SPREAD: f32 = 7.0;
// How much a run through an opaque box takes to stop a ray fully, units.
const STOP_SOFT: f32 = 3.0;

// --- the grid ---------------------------------------------------------------

fn in_map(c: vec2<i32>) -> bool {
    return c.x >= 0 && c.y >= 0 && f32(c.x) < P.map.x && f32(c.y) < P.map.y;
}

fn tile_at(c: vec2<i32>) -> vec4<u32> {
    if !in_map(c) {
        return vec4<u32>(FLOOR_GRASS, WALL_NONE, 0u, 0u);
    }
    return vec4<u32>(round(textureLoad(tiles, c, 0) * 255.0));
}

fn box_at(c: vec2<i32>) -> vec4<f32> {
    if !in_map(c) {
        return vec4<f32>(0.0);
    }
    return textureLoad(occ, c, 0);
}

fn object_at(c: vec2<i32>) -> u32 {
    if !in_map(c) {
        return 0u;
    }
    return u32(round(textureLoad(extra, c, 0).r * 255.0));
}

fn indoor_at(c: vec2<i32>) -> bool {
    if !in_map(c) {
        return false;
    }
    return textureLoad(extra, c, 0).g > 0.5;
}

fn is_wall(c: vec2<i32>) -> bool {
    let t = tile_at(c);
    return t.y != WALL_NONE;
}

// What a point is standing on, for the skip: its tile, whether it is inside
// that tile's box, and the box's object.
struct Spot {
    cell: vec2<i32>,
    in_box: bool,
    object: u32,
    // On a wall's top (see `wall_face`).
    wall: bool,
};

fn spot_of(w: vec2<f32>) -> Spot {
    let tile = P.map.z;
    let cell = vec2<i32>(floor(w / tile));
    let b = box_at(cell);
    let f = w / tile - vec2<f32>(cell);
    var s: Spot;
    s.cell = cell;
    s.in_box = b.z > b.x && f.x >= b.x && f.x <= b.z && f.y >= b.y && f.y <= b.w;
    s.object = select(0u, object_at(cell), s.in_box);
    s.wall = s.in_box && s.object == 0u;
    return s;
}

// How far the segment a → b runs inside opaque boxes (x) and through glass
// (y), in world units — the boxes `src` and `dst` stand in left out.
fn trace(a: vec2<f32>, b: vec2<f32>, src: Spot, dst: Spot) -> vec2<f32> {
    let tile = P.map.z;
    let p = a / tile;
    let q = b / tile;
    let d = q - p;
    let len = length(d);
    if len < 1e-5 {
        return vec2<f32>(0.0);
    }
    var cell = vec2<i32>(floor(p));
    let dir = vec2<i32>(sign(d));
    let big = 1e30;
    let inv = vec2<f32>(
        select(big, 1.0 / d.x, abs(d.x) > 1e-9),
        select(big, 1.0 / d.y, abs(d.y) > 1e-9),
    );
    var tmax = vec2<f32>(
        select(big, (f32(cell.x) + select(0.0, 1.0, d.x > 0.0) - p.x) * inv.x, abs(d.x) > 1e-9),
        select(big, (f32(cell.y) + select(0.0, 1.0, d.y > 0.0) - p.y) * inv.y, abs(d.y) > 1e-9),
    );
    let delta = abs(inv);
    var t = 0.0;
    var opaque = 0.0;
    var glass = 0.0;
    for (var i = 0; i < 128; i++) {
        let t1 = min(min(tmax.x, tmax.y), 1.0);
        if in_map(cell) {
            let o = textureLoad(occ, cell, 0);
            if o.z > o.x {
                let obj = object_at(cell);
                let skip = (all(cell == dst.cell) && dst.in_box)
                    || (dst.object != 0u && obj == dst.object)
                    || (all(cell == src.cell) && src.in_box)
                    || (src.object != 0u && obj == src.object);
                if !skip {
                    let bmin = vec2<f32>(cell) + o.xy;
                    let bmax = vec2<f32>(cell) + o.zw;
                    let s0 = (bmin - p) * inv;
                    let s1 = (bmax - p) * inv;
                    let tn = max(min(s0.x, s1.x), min(s0.y, s1.y));
                    let tf = min(max(s0.x, s1.x), max(s0.y, s1.y));
                    opaque += max(0.0, min(tf, t1) - max(tn, t));
                }
            }
            let kind = u32(round(textureLoad(tiles, cell, 0).g * 255.0));
            if kind == WALL_GLASS && !all(cell == dst.cell) {
                glass += t1 - t;
            }
        }
        if t1 >= 1.0 {
            break;
        }
        if tmax.x < tmax.y {
            cell.x += dir.x;
            t = tmax.x;
            tmax.x += delta.x;
        } else {
            cell.y += dir.y;
            t = tmax.y;
            tmax.y += delta.y;
        }
    }
    return vec2<f32>(opaque, glass) * len * tile;
}

fn clear_share(run: f32) -> f32 {
    return 1.0 - smoothstep(0.0, STOP_SOFT, run);
}

// --- light and sight ----------------------------------------------------------

fn light_at(w: vec2<f32>, dst: Spot, indoor: bool) -> vec3<f32> {
    var light = select(vec3<f32>(0.050, 0.062, 0.105), vec3<f32>(0.018, 0.020, 0.030), indoor);
    let n = i32(P.misc.y);
    for (var i = 0; i < n; i++) {
        let lp = P.lights[2 * i];
        let lc = P.lights[2 * i + 1];
        let d = distance(lp.xy, w);
        if d >= lp.z {
            continue;
        }
        let x = d / lp.z;
        let fall = (1.0 - x * x) * (1.0 - x * x) / (1.0 + 6.0 * x * x);
        let run = trace(lp.xy, w, spot_of(lp.xy), dst);
        light += lc.rgb * (lp.w * fall * clear_share(run.x));
    }
    return light;
}

// How much of the pixel the player sees, nought to one.
fn seen_at(w: vec2<f32>, dst: Spot, light: vec3<f32>) -> f32 {
    let eye = P.eye.xy;
    let off = w - eye;
    let dist = length(off);
    let tile = P.map.z;
    if dist < 20.0 {
        return 1.0;
    }
    // Bims' reach: `dark` in the dark, further the more light there is,
    // never past `view`.
    let level = clamp(dot(light, vec3<f32>(0.3, 0.59, 0.11)) * 1.6, 0.0, 0.95);
    let reach = min(P.eye.w / (1.0 - level), P.eye.z);
    let ranged = 1.0 - smoothstep(reach - 0.8 * tile, reach, dist);
    if ranged <= 0.0 {
        return 0.0;
    }
    let side = vec2<f32>(-off.y, off.x) / dist * EYE_SPREAD;
    let src = spot_of(eye);
    var seen = 0.0;
    for (var k = -1; k <= 1; k++) {
        let e = eye + side * f32(k);
        let run = trace(e, w, src, dst);
        seen += clear_share(run.x) * select(1.0, 0.85, run.y > 1.0);
    }
    return seen / 3.0 * ranged;
}

fn luma(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

// A wall's top is lit and seen as the floor in front of its nearest open
// face is: a ray to the top itself would graze along the wall's other tiles
// and leave saw teeth of shadow on it. A wall between two rooms is lit by
// each on its own half, an outside wall's outer half by the street.
fn wall_face(w: vec2<f32>, cell: vec2<i32>) -> vec2<f32> {
    let tile = P.map.z;
    let f = w / tile - vec2<f32>(cell);
    let lo = vec2<f32>(cell) * tile;
    let hi = lo + vec2<f32>(tile);
    var best = w;
    var near = 1e9;
    if !is_wall(cell - vec2<i32>(1, 0)) && f.x < near {
        near = f.x;
        best = vec2<f32>(lo.x - 1.0, w.y);
    }
    if !is_wall(cell + vec2<i32>(1, 0)) && 1.0 - f.x < near {
        near = 1.0 - f.x;
        best = vec2<f32>(hi.x + 1.0, w.y);
    }
    if !is_wall(cell - vec2<i32>(0, 1)) && f.y < near {
        near = f.y;
        best = vec2<f32>(w.x, lo.y - 1.0);
    }
    if !is_wall(cell + vec2<i32>(0, 1)) && 1.0 - f.y < near {
        near = 1.0 - f.y;
        best = vec2<f32>(w.x, hi.y + 1.0);
    }
    // Walled in on all four sides: an inside corner's open diagonal.
    if near > 1e8 {
        for (var dy = -1; dy <= 1; dy += 2) {
            for (var dx = -1; dx <= 1; dx += 2) {
                let c = cell + vec2<i32>(dx, dy);
                let corner = vec2<f32>(select(lo.x, hi.x, dx > 0), select(lo.y, hi.y, dy > 0));
                let d = distance(w, corner) / tile;
                if !is_wall(c) && d < near {
                    near = d;
                    best = corner + vec2<f32>(f32(dx), f32(dy));
                }
            }
        }
    }
    return best;
}

fn shade(albedo: vec3<f32>, glow: bool, ground: bool, at: vec2<f32>) -> vec3<f32> {
    var w = at;
    var dst = spot_of(w);
    if dst.wall {
        w = wall_face(w, dst.cell);
        dst = spot_of(w);
    } else if ground {
        // The floor under a thing is not the thing: a round pillar's
        // square box shades the floor at its corners.
        dst.in_box = false;
        dst.object = 0u;
    }
    let indoor = indoor_at(dst.cell);
    if glow {
        let seen = seen_at(w, dst, vec3<f32>(1.0));
        return albedo * mix(0.35, 1.0, seen);
    }
    let light = light_at(w, dst, indoor);
    let seen = seen_at(w, dst, light);
    // Seen: in its light, never quite black — eyes used to the dark.
    let lit = albedo * (light + vec3<f32>(0.075, 0.08, 0.11));
    // Unseen: dim, mostly grey, a little blue.
    let dim = albedo * light;
    let fog = mix(dim, vec3<f32>(luma(dim)), 0.85) * 0.2 + albedo * vec3<f32>(0.004, 0.006, 0.014);
    return mix(fog, lit, seen);
}

// --- the ground's pictures -----------------------------------------------------

fn hash_u(x: u32) -> u32 {
    var h = x * 747796405u + 2891336453u;
    h = ((h >> ((h >> 28u) + 4u)) ^ h) * 277803737u;
    return (h >> 22u) ^ h;
}

fn hash2(c: vec2<i32>) -> f32 {
    let h = hash_u(u32(c.x) * 1597334677u ^ hash_u(u32(c.y)));
    return f32(h & 0xffffffu) / 16777215.0;
}

fn noise(p: vec2<f32>) -> f32 {
    let i = vec2<i32>(floor(p));
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash2(i);
    let b = hash2(i + vec2<i32>(1, 0));
    let c = hash2(i + vec2<i32>(0, 1));
    let d = hash2(i + vec2<i32>(1, 1));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

fn fbm(p: vec2<f32>) -> f32 {
    return noise(p) * 0.55 + noise(p * 2.03 + 7.1) * 0.3 + noise(p * 4.1 + 3.3) * 0.15;
}

fn srgb(c: vec3<f32>) -> vec3<f32> {
    return pow(c, vec3<f32>(2.2));
}

// How close to a grid line a point is, at `size` spacing: 0 on the line.
fn grid_line(p: vec2<f32>, size: f32) -> f32 {
    let g = abs(fract(p / size + 0.5) - 0.5) * size;
    return min(g.x, g.y);
}

const CARPETS = array<vec3<f32>, 6>(
    vec3<f32>(0.24, 0.28, 0.36),
    vec3<f32>(0.22, 0.32, 0.26),
    vec3<f32>(0.38, 0.20, 0.22),
    vec3<f32>(0.42, 0.37, 0.30),
    vec3<f32>(0.28, 0.29, 0.31),
    vec3<f32>(0.18, 0.32, 0.34),
);

fn floor_colour(kind: u32, tint: u32, deco: u32, cell: vec2<i32>, f: vec2<f32>, w: vec2<f32>) -> vec3<f32> {
    let tile = P.map.z;
    let edge = min(min(f.x, 1.0 - f.x), min(f.y, 1.0 - f.y));
    var c = vec3<f32>(0.5);
    switch kind {
        case FLOOR_GRASS: {
            let n = fbm(w / 40.0);
            let blades = noise(w / 2.2);
            c = mix(vec3<f32>(0.13, 0.24, 0.10), vec3<f32>(0.24, 0.36, 0.15), n) * (0.85 + 0.3 * blades);
        }
        case FLOOR_ASPHALT, FLOOR_PARKING: {
            let base = select(0.16, 0.19, kind == FLOOR_PARKING);
            c = vec3<f32>(base, base, base * 1.04) * (0.8 + 0.35 * fbm(w / 6.0)) + vec3<f32>(0.06) * step(0.93, noise(w / 1.3));
            if (deco & DECO_LANE) != 0u && f.y < 0.07 && fract(w.x / (tile * 3.0)) < 0.55 {
                c = vec3<f32>(0.80, 0.68, 0.26);
            }
            // A zebra: white bars along the road, stepped over crossing it.
            if (deco & DECO_ZEBRA) != 0u && fract(w.y / 26.0) < 0.5 {
                let wear = 0.75 + 0.25 * fbm(w / 9.0);
                c = mix(c, vec3<f32>(0.74, 0.74, 0.70), wear);
            }
            if (deco & DECO_BAY) != 0u && f.x < 0.05 {
                c = vec3<f32>(0.72, 0.72, 0.68);
            }
            // The kerb, where the road meets the pavement.
            let up = tile_at(cell + vec2<i32>(0, 1));
            if kind == FLOOR_ASPHALT && up.x == FLOOR_SIDEWALK && f.y > 0.88 {
                c = vec3<f32>(0.55, 0.55, 0.53) * (0.9 + 0.2 * noise(w / 3.0));
            }
        }
        case FLOOR_SIDEWALK: {
            let slab = hash2(cell);
            c = vec3<f32>(0.46, 0.46, 0.44) * (0.9 + 0.15 * slab) * (0.85 + 0.25 * fbm(w / 9.0));
            if edge < 0.025 {
                c *= 0.6;
            }
        }
        case FLOOR_MARBLE: {
            let check = (cell.x + cell.y) % 2 == 0;
            let vein = abs(sin(w.x * 0.015 + w.y * 0.01 + fbm(w / 45.0) * 7.0));
            if check {
                c = vec3<f32>(0.80, 0.77, 0.71) * (0.95 + 0.08 * fbm(w / 20.0));
                c = mix(c * 0.84, c, smoothstep(0.0, 0.08, vein));
            } else {
                c = vec3<f32>(0.50, 0.50, 0.47) * (0.92 + 0.12 * fbm(w / 20.0));
                c = mix(vec3<f32>(0.64, 0.63, 0.60), c, smoothstep(0.0, 0.05, vein));
            }
            if edge < 0.012 {
                c *= 0.7;
            }
            if (deco & DECO_RUG) != 0u {
                let l = (tile_at(cell - vec2<i32>(1, 0)).w & DECO_RUG) != 0u;
                let r = (tile_at(cell + vec2<i32>(1, 0)).w & DECO_RUG) != 0u;
                c = vec3<f32>(0.42, 0.08, 0.10) * (0.85 + 0.2 * noise(w / 1.5));
                let pattern = abs(fract((w.y + abs(fract(w.x / 26.0) - 0.5) * 26.0) / 26.0) - 0.5);
                c = mix(c, vec3<f32>(0.52, 0.14, 0.12), step(pattern, 0.08));
                if (!l && f.x < 0.14) || (!r && f.x > 0.86) {
                    c = vec3<f32>(0.62, 0.48, 0.20);
                }
                if (!l && f.x < 0.06) || (!r && f.x > 0.94) {
                    c = vec3<f32>(0.30, 0.05, 0.06);
                }
            }
        }
        case FLOOR_CARPET: {
            var carpets = CARPETS;
            c = carpets[tint % 6u];
            let check = (cell.x + cell.y) % 2 == 0;
            let grain = select(sin(w.x * 1.1), sin(w.y * 1.1), check);
            c *= 0.88 + 0.16 * noise(w / 1.2) + 0.03 * grain;
            if edge < 0.01 {
                c *= 0.85;
            }
        }
        case FLOOR_WOOD: {
            let h = tile / 4.0;
            let row = i32(floor(w.y / h));
            let off = hash2(vec2<i32>(row, 7)) * tile * 2.0;
            let plank = i32(floor((w.x + off) / (tile * 2.0)));
            let tone = hash2(vec2<i32>(plank, row));
            c = vec3<f32>(0.46, 0.29, 0.16) * (0.8 + 0.35 * tone);
            c *= 0.9 + 0.12 * sin(w.y * 0.9 + noise(w / vec2<f32>(40.0, 3.0)) * 5.0);
            let along = fract((w.x + off) / (tile * 2.0));
            if fract(w.y / h) < 0.05 || along < 0.01 {
                c *= 0.55;
            }
        }
        case FLOOR_WASHROOM: {
            c = vec3<f32>(0.80, 0.82, 0.83) * (0.95 + 0.06 * hash2(vec2<i32>(floor(w / (tile / 4.0)))));
            if grid_line(w, tile / 4.0) < 0.8 {
                c = vec3<f32>(0.55, 0.56, 0.56);
            }
        }
        case FLOOR_CONCRETE: {
            c = vec3<f32>(0.40, 0.40, 0.39) * (0.8 + 0.3 * fbm(w / 14.0));
            if grid_line(w, tile * 3.0) < 0.8 {
                c *= 0.75;
            }
        }
        case FLOOR_VAULT: {
            c = vec3<f32>(0.36, 0.39, 0.42) * (0.92 + 0.1 * fbm(w / 10.0));
            // Diamond plate: little raised bars, alternating.
            let s = 9.0;
            let g = w / s;
            let k = vec2<i32>(floor(g));
            let fp = fract(g) - 0.5;
            let turn = select(1.0, -1.0, (k.x + k.y) % 2 == 0);
            let r = vec2<f32>(fp.x + fp.y * turn, fp.y - fp.x * turn) * 0.7071;
            let bar = length(vec2<f32>(r.x * 0.45, r.y * 2.2));
            c *= 1.0 + 0.25 * (1.0 - smoothstep(0.12, 0.2, bar));
            if edge < 0.02 {
                c *= 0.6;
            }
            let corner = length(abs(f - 0.5) - vec2<f32>(0.42));
            if corner < 0.035 {
                c = vec3<f32>(0.55, 0.58, 0.62);
            }
        }
        case FLOOR_LINO: {
            let q = vec2<i32>(floor(w / (tile * 0.5)));
            let check = (q.x + q.y) % 2 == 0;
            let other = select(vec3<f32>(0.22, 0.38, 0.40), vec3<f32>(0.45, 0.18, 0.16), tint % 2u == 0u);
            c = select(other, vec3<f32>(0.78, 0.76, 0.70), check) * (0.92 + 0.1 * noise(w / 5.0));
        }
        case FLOOR_RAISED: {
            c = vec3<f32>(0.56, 0.58, 0.60);
            let holes = grid_line(w + tile * 0.0625, tile / 8.0);
            if holes > 2.4 && edge > 0.08 {
                c *= 0.8;
            }
            if edge < 0.02 {
                c *= 0.55;
            }
        }
        default: {}
    }
    // A threshold across a doorway.
    if (deco & DECO_DOOR) != 0u {
        let across_x = is_wall(cell - vec2<i32>(1, 0)) || is_wall(cell + vec2<i32>(1, 0));
        let m = select(f.x, f.y, across_x);
        if abs(m - 0.5) < 0.1 {
            c = vec3<f32>(0.45, 0.45, 0.46) * (0.9 + 0.2 * noise(w / 2.0));
            if (deco & DECO_VAULT_DOOR) != 0u {
                let stripe = fract((w.x + w.y) / 12.0);
                c = select(vec3<f32>(0.08), vec3<f32>(0.80, 0.62, 0.10), stripe < 0.5);
            }
        }
    }
    return c;
}

fn wall_colour(kind: u32, cell: vec2<i32>, f: vec2<f32>, w: vec2<f32>, under: vec3<f32>) -> vec3<f32> {
    let tile = P.map.z;
    let wl = is_wall(cell - vec2<i32>(1, 0));
    let wr = is_wall(cell + vec2<i32>(1, 0));
    let wd = is_wall(cell - vec2<i32>(0, 1));
    let wu = is_wall(cell + vec2<i32>(0, 1));
    // How far to an edge that faces a floor, in tiles.
    var e = 1.0;
    if !wl { e = min(e, f.x); }
    if !wr { e = min(e, 1.0 - f.x); }
    if !wd { e = min(e, f.y); }
    if !wu { e = min(e, 1.0 - f.y); }
    var c = vec3<f32>(0.5);
    switch kind {
        case WALL_BRICK: {
            let row = floor(w.y / 9.0);
            let bx = (w.x + row * 9.0) / 18.0;
            let brick = vec2<i32>(i32(floor(bx)), i32(row));
            c = vec3<f32>(0.36, 0.17, 0.12) * (0.8 + 0.35 * hash2(brick));
            if fract(w.y / 9.0) < 0.12 || fract(bx) < 0.06 {
                c = vec3<f32>(0.22, 0.20, 0.18);
            }
            if e < 0.18 {
                c = vec3<f32>(0.42, 0.40, 0.37) * (0.9 + 0.15 * noise(w / 3.0));
            }
        }
        case WALL_DRYWALL: {
            c = vec3<f32>(0.50, 0.48, 0.45) * (0.95 + 0.08 * noise(w / 6.0));
        }
        case WALL_VAULT: {
            c = vec3<f32>(0.32, 0.36, 0.40) * (0.9 + 0.12 * fbm(w / 12.0));
            let stripe = abs(fract((w.x + w.y) / 30.0) - 0.5);
            c *= 0.92 + 0.12 * step(stripe, 0.25);
            let rivet = length(fract(w / (tile / 3.0)) - 0.5);
            if rivet < 0.08 {
                c = vec3<f32>(0.58, 0.62, 0.66);
            }
        }
        case WALL_GLASS: {
            // The floor under it, a cold sheen, and the frame.
            let horizontal = wl || wr;
            let m = select(f.x, f.y, horizontal);
            let along = select(f.y, f.x, horizontal);
            c = mix(under, vec3<f32>(0.55, 0.74, 0.82), 0.45);
            c += vec3<f32>(0.08) * step(0.85, fract((along + m) * 2.0));
            if abs(m - 0.5) > 0.36 {
                c = vec3<f32>(0.30, 0.32, 0.34);
            }
            if along < 0.04 || along > 0.96 {
                c = vec3<f32>(0.26, 0.28, 0.30);
            }
            return c;
        }
        default: {}
    }
    // A dark line round the top where it meets the floor, and a bevel.
    if e < 0.05 {
        c *= 0.35;
    } else if e < 0.14 {
        c *= 0.78;
    }
    return c;
}

@fragment
fn fragment(mesh: VertexOutput) -> @location(0) vec4<f32> {
    let w = mesh.world_position.xy;
    if P.misc.x < 0.5 {
        let tile = P.map.z;
        let cell = vec2<i32>(floor(w / tile));
        let f = w / tile - vec2<f32>(cell);
        let t = tile_at(cell);
        var albedo = srgb(floor_colour(t.x, t.z, t.w, cell, f, w));
        if t.y != WALL_NONE {
            albedo = srgb(wall_colour(t.y, cell, f, w, pow(albedo, vec3<f32>(1.0 / 2.2))));
        }
        return vec4<f32>(shade(albedo, false, true, w), 1.0);
    }
#ifdef VERTEX_COLORS
    let colour = mesh.color;
#else
    let colour = vec4<f32>(1.0, 0.0, 1.0, 1.0);
#endif
    let glow = mesh.uv.x > 0.5;
    return vec4<f32>(shade(colour.rgb, glow, false, w), colour.a);
}
