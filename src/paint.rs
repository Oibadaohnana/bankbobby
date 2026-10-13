//! The game's own pictures: every prop and the robber are drawn here as
//! flat-shaded triangles with a colour at each corner, built into a Bevy
//! mesh. No image assets.
//!
//! A vertex's colour is linear RGB with an alpha (shadows are translucent
//! black), and its first UV says whether it is **lit** (0: the scene's
//! light and the player's sight shade it) or **glowing** (1: a screen, a
//! lamp's glass, an LED — its own light, past white for the bloom).

use crate::map::{NORTH, Prop, PropKind, SOUTH, face_dir};
use crate::movement::TILE;
use crate::rng::Rng;
use bevy::asset::RenderAssetUsages;
use bevy::math::{Vec2, vec2};
use bevy::mesh::{Indices, Mesh, PrimitiveTopology};
use std::f32::consts::{PI, TAU};

/// A colour: linear RGB and alpha, and whether it glows.
#[derive(Clone, Copy, Debug)]
pub struct Paint {
    pub rgba: [f32; 4],
    pub glow: bool,
}

fn to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// An sRGB hex colour.
pub const fn hex(rgb: u32) -> Hex {
    Hex(rgb)
}

#[derive(Clone, Copy, Debug)]
pub struct Hex(pub u32);

impl Hex {
    pub fn paint(self) -> Paint {
        self.alpha(1.0)
    }
    pub fn alpha(self, a: f32) -> Paint {
        let r = ((self.0 >> 16) & 0xff) as f32 / 255.0;
        let g = ((self.0 >> 8) & 0xff) as f32 / 255.0;
        let b = (self.0 & 0xff) as f32 / 255.0;
        Paint {
            rgba: [to_linear(r), to_linear(g), to_linear(b), a],
            glow: false,
        }
    }
    /// Glowing, `power` times as bright as the colour.
    pub fn glow(self, power: f32) -> Paint {
        let mut p = self.paint();
        for c in &mut p.rgba[..3] {
            *c *= power;
        }
        p.glow = true;
        p
    }
    /// Lighter (towards white) or darker (towards black) by `amt`.
    pub fn shade(self, amt: f32) -> Hex {
        let ch = |s: u32| -> u32 {
            let c = ((self.0 >> s) & 0xff) as f32;
            let t = if amt < 0.0 { 0.0 } else { 255.0 };
            (c + (t - c) * amt.abs()).round().clamp(0.0, 255.0) as u32
        };
        Hex((ch(16) << 16) | (ch(8) << 8) | ch(0))
    }
}

impl From<Hex> for Paint {
    fn from(h: Hex) -> Paint {
        h.paint()
    }
}

/// Where a prop's own coordinates go in the world: `x` across its front,
/// `y` towards its front, from its body's middle.
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    pub origin: Vec2,
    pub x: Vec2,
    pub y: Vec2,
}

impl Frame {
    pub const IDENTITY: Frame = Frame {
        origin: Vec2::ZERO,
        x: Vec2::X,
        y: Vec2::Y,
    };
    fn at(&self, p: Vec2) -> Vec2 {
        self.origin + self.x * p.x + self.y * p.y
    }
}

/// Triangles with a colour at every corner.
pub struct Painter {
    positions: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    uvs: Vec<[f32; 2]>,
    indices: Vec<u32>,
    pub frame: Frame,
}

impl Default for Painter {
    fn default() -> Self {
        Painter {
            positions: Vec::new(),
            colors: Vec::new(),
            uvs: Vec::new(),
            indices: Vec::new(),
            frame: Frame::IDENTITY,
        }
    }
}

impl Painter {
    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }

    fn vertex(&mut self, p: Vec2, paint: Paint) -> u32 {
        let w = self.frame.at(p);
        self.positions.push([w.x, w.y, 0.0]);
        self.colors.push(paint.rgba);
        self.uvs.push([if paint.glow { 1.0 } else { 0.0 }, 0.0]);
        (self.positions.len() - 1) as u32
    }

    /// A convex polygon, as a fan.
    pub fn poly(&mut self, points: &[Vec2], paint: impl Into<Paint>) {
        let paint = paint.into();
        if points.len() < 3 {
            return;
        }
        let first = self.vertex(points[0], paint);
        let mut prev = self.vertex(points[1], paint);
        for &p in &points[2..] {
            let v = self.vertex(p, paint);
            self.indices.extend([first, prev, v]);
            prev = v;
        }
    }

    /// A box by its middle and half-size, turned by `angle`.
    pub fn rect(&mut self, centre: Vec2, half: Vec2, angle: f32, paint: impl Into<Paint>) {
        let (s, c) = angle.sin_cos();
        let ax = vec2(c, s) * half.x;
        let ay = vec2(-s, c) * half.y;
        self.poly(
            &[
                centre - ax - ay,
                centre + ax - ay,
                centre + ax + ay,
                centre - ax + ay,
            ],
            paint,
        );
    }

    /// A box with rounded corners.
    pub fn rounded(&mut self, centre: Vec2, half: Vec2, radius: f32, paint: impl Into<Paint>) {
        let r = radius.min(half.x).min(half.y);
        let mut pts = Vec::with_capacity(24);
        for (k, (sx, sy)) in [(1.0, 1.0), (-1.0, 1.0), (-1.0, -1.0), (1.0, -1.0)]
            .into_iter()
            .enumerate()
        {
            let c = centre + vec2(sx * (half.x - r), sy * (half.y - r));
            for i in 0..=4 {
                let a = (k as f32 + i as f32 / 4.0) * PI / 2.0;
                pts.push(c + vec2(a.cos(), a.sin()) * r);
            }
        }
        self.poly(&pts, paint);
    }

    pub fn ellipse(&mut self, centre: Vec2, radii: Vec2, angle: f32, paint: impl Into<Paint>) {
        let n = ((radii.x.max(radii.y) * 0.9) as usize).clamp(10, 40);
        let (s, c) = angle.sin_cos();
        let pts: Vec<Vec2> = (0..n)
            .map(|i| {
                let a = i as f32 / n as f32 * TAU;
                let p = vec2(a.cos() * radii.x, a.sin() * radii.y);
                centre + vec2(p.x * c - p.y * s, p.x * s + p.y * c)
            })
            .collect();
        self.poly(&pts, paint);
    }

    pub fn circle(&mut self, centre: Vec2, r: f32, paint: impl Into<Paint>) {
        self.ellipse(centre, Vec2::splat(r), 0.0, paint);
    }

    /// A stroke from `a` to `b`, `width` wide, square-ended.
    pub fn line(&mut self, a: Vec2, b: Vec2, width: f32, paint: impl Into<Paint>) {
        let d = b - a;
        let len = d.length();
        if len < 1e-4 {
            return;
        }
        let n = vec2(-d.y, d.x) / len * (width * 0.5);
        self.poly(&[a - n, b - n, b + n, a + n], paint);
    }

    /// A stroke with round ends.
    pub fn capsule(&mut self, a: Vec2, b: Vec2, width: f32, paint: impl Into<Paint>) {
        let paint = paint.into();
        self.line(a, b, width, paint);
        self.circle(a, width * 0.5, paint);
        self.circle(b, width * 0.5, paint);
    }

    /// A soft drop shadow under a box: three widening, fading layers.
    pub fn shadow(&mut self, centre: Vec2, half: Vec2, radius: f32) {
        for (grow, a) in [(5.0, 0.10), (2.5, 0.14), (0.0, 0.18)] {
            self.rounded(
                centre + vec2(3.0, -4.0),
                half + Vec2::splat(grow),
                radius + grow,
                hex(0x000000).alpha(a),
            );
        }
    }

    pub fn build(self) -> Mesh {
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::RENDER_WORLD,
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, self.positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, self.colors);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, self.uvs);
        mesh.insert_indices(Indices::U32(self.indices));
        mesh
    }
}

// --- the props -------------------------------------------------------------

/// The palette.
mod pal {
    use super::{Hex, hex};
    pub const WOOD: Hex = hex(0x8a5a36);
    pub const WOOD_DARK: Hex = hex(0x5e3a22);
    pub const WALNUT: Hex = hex(0x4a2c1c);
    pub const STEEL: Hex = hex(0x8c949c);
    pub const STEEL_DARK: Hex = hex(0x4c535a);
    pub const BLACK: Hex = hex(0x1c1d21);
    pub const PAPER: Hex = hex(0xeeeae0);
    pub const LEAF: Hex = hex(0x3f7a3a);
    pub const LEAF_DARK: Hex = hex(0x2a5428);
    pub const POT: Hex = hex(0xb06a44);
    pub const MARBLE: Hex = hex(0xd8d2c6);
    pub const CASH: Hex = hex(0x6fa060);
    pub const GOLD: Hex = hex(0xe0b03c);
    pub const SCREEN: Hex = hex(0x6fc8ff);
    pub const VELVET: Hex = hex(0x8c1f2a);
    pub const BRASS: Hex = hex(0xc8a050);
    pub const LEATHER: Hex = hex(0x6a3424);
    pub const WHITE: Hex = hex(0xf2f2ee);
    pub const RUG: Hex = hex(0x7a2830);
}

/// Paint every prop into the three layers.
pub fn paint_props(props: &[Prop]) -> [Painter; 3] {
    let mut layers = [Painter::default(), Painter::default(), Painter::default()];
    // Shadows first, so nothing's shadow falls on another's top.
    for p in props {
        if matches!(
            p.kind,
            PropKind::Rug
                | PropKind::VaultDoor
                | PropKind::Stanchion
                | PropKind::StreetLamp
                | PropKind::Tree
        ) {
            continue;
        }
        let b = p.body;
        let low = &mut layers[1];
        low.frame = Frame::IDENTITY;
        let r = if matches!(
            p.kind,
            PropKind::Plant | PropKind::Bush | PropKind::Bin | PropKind::Pillar | PropKind::Hydrant
        ) {
            b.width().min(b.height()) * 0.5
        } else {
            3.0
        };
        low.shadow(b.centre(), (b.max - b.min) * 0.5, r);
    }
    for p in props {
        let layer = match p.kind {
            PropKind::Rug => 0,
            _ => 1,
        };
        paint_prop(p, &mut layers[layer]);
        if matches!(p.kind, PropKind::Tree | PropKind::StreetLamp) {
            paint_high(p, &mut layers[2]);
        }
    }
    layers
}

fn frame_of(p: &Prop) -> (Frame, Vec2) {
    let front = face_dir(p.face);
    let x = vec2(front.y, -front.x); // across, to the right looking out of it
    let b = p.body;
    let size = b.max - b.min;
    // Its own size: across, then deep.
    let local = if p.face == NORTH || p.face == SOUTH {
        size
    } else {
        vec2(size.y, size.x)
    };
    (
        Frame {
            origin: b.centre(),
            x,
            y: front,
        },
        local * 0.5,
    )
}

fn paint_prop(p: &Prop, g: &mut Painter) {
    use PropKind::*;
    let (frame, h) = frame_of(p);
    g.frame = frame;
    let mut rng = Rng::new(p.seed as u64);
    let o = Vec2::ZERO;
    match p.kind {
        Desk | BigDesk => {
            let top = if p.kind == BigDesk {
                pal::WALNUT
            } else {
                pal::WOOD
            };
            g.rounded(o, h, 2.0, top.shade(-0.25));
            g.rounded(o + vec2(0.0, 1.0), h - Vec2::splat(2.0), 2.0, top);
            // A monitor at the back, a keyboard, some papers.
            let mx = if p.kind == BigDesk {
                -h.x * 0.45
            } else {
                rng.f32() * 10.0 - 5.0
            };
            g.rect(o + vec2(mx, -h.y + 9.0), vec2(16.0, 3.0), 0.0, pal::BLACK);
            g.rect(
                o + vec2(mx, -h.y + 11.0),
                vec2(14.0, 1.4),
                0.0,
                pal::SCREEN.glow(1.6),
            );
            g.rect(o + vec2(mx, -h.y + 3.0), vec2(4.0, 3.0), 0.0, pal::BLACK);
            g.rect(o + vec2(mx, 5.0), vec2(11.0, 3.5), 0.0, hex(0x2a2c30));
            for _ in 0..rng.range(1, 4) {
                let at =
                    vec2(rng.f32() * 2.0 - 1.0, rng.f32() * 2.0 - 1.0) * (h - Vec2::splat(10.0));
                g.rect(o + at, vec2(6.0, 8.0), rng.f32() - 0.5, pal::PAPER);
            }
            if p.kind == BigDesk {
                g.rect(o + vec2(h.x * 0.5, 2.0), vec2(9.0, 2.0), 0.0, pal::BRASS);
                g.circle(o + vec2(h.x * 0.75, -6.0), 5.0, hex(0x2a6a9a));
            }
        }
        Chair => {
            let seat = hex(0x2c2f36);
            g.circle(o, 11.0, seat.shade(-0.3));
            g.rounded(o + vec2(0.0, 1.0), vec2(9.0, 8.0), 4.0, seat.shade(0.1));
            g.rounded(o + vec2(0.0, -9.0), vec2(10.0, 3.0), 2.0, seat);
        }
        Cabinet => {
            g.rect(o, h, 0.0, pal::STEEL_DARK);
            g.rect(o, h - Vec2::splat(2.0), 0.0, pal::STEEL);
            for k in 0..3 {
                let y = -h.y + 5.0 + k as f32 * (h.y * 2.0 - 10.0) / 2.0;
                g.line(
                    o + vec2(-h.x + 3.0, y),
                    o + vec2(h.x - 3.0, y),
                    1.0,
                    pal::STEEL_DARK,
                );
            }
            g.rect(o + vec2(0.0, h.y - 3.0), vec2(5.0, 1.2), 0.0, pal::BLACK);
        }
        Bookshelf | Lockers => {
            let (frame_c, inner) = if p.kind == Bookshelf {
                (pal::WOOD_DARK, pal::WALNUT)
            } else {
                (pal::STEEL_DARK, pal::STEEL)
            };
            g.rect(o, h, 0.0, frame_c);
            g.rect(o, h - Vec2::splat(2.5), 0.0, inner);
            if p.kind == Bookshelf {
                let mut x = -h.x + 3.0;
                while x < h.x - 4.0 {
                    let w = 2.0 + rng.f32() * 3.0;
                    let c = rng.pick(&[0x8c2a2a, 0x2a4c8c, 0x2a7a4a, 0xc89a3a, 0x5a3a7a, 0xd0c8b0]);
                    g.rect(
                        o + vec2(x + w * 0.5, 1.0),
                        vec2(w * 0.5, h.y - 4.0 - rng.f32() * 3.0),
                        0.0,
                        hex(c),
                    );
                    x += w + 0.6;
                }
            } else {
                // Deposit boxes: a grid of little doors with keyholes.
                let cols = ((h.x * 2.0) / 12.0) as i32;
                for i in 0..cols {
                    let x = -h.x + 6.0 + i as f32 * (h.x * 2.0 - 12.0) / (cols - 1).max(1) as f32;
                    g.rect(
                        o + vec2(x, 0.0),
                        vec2(4.8, h.y - 3.0),
                        0.0,
                        pal::STEEL.shade(0.15),
                    );
                    g.circle(o + vec2(x, h.y - 5.0), 1.2, pal::BRASS);
                }
            }
        }
        Plant => {
            g.circle(o, 12.0, pal::POT.shade(-0.3));
            g.circle(o, 10.0, pal::POT);
            for i in 0..7 {
                let a = i as f32 / 7.0 * TAU + rng.f32();
                let d = 8.0 + rng.f32() * 6.0;
                g.ellipse(
                    o + vec2(a.cos(), a.sin()) * d * 0.6,
                    vec2(d, 4.5),
                    a,
                    pal::LEAF.shade(rng.f32() * 0.3 - 0.15),
                );
            }
            g.circle(o, 5.0, pal::LEAF_DARK);
        }
        Pillar => {
            // A square plinth, the round column on it.
            g.rect(o, h, 0.0, pal::MARBLE.shade(-0.5));
            g.rect(o, h - Vec2::splat(2.0), 0.0, pal::MARBLE.shade(-0.2));
            g.circle(o, h.x + 1.0, pal::MARBLE.shade(-0.35));
            g.circle(o, h.x - 2.0, pal::MARBLE);
            g.circle(o + vec2(-4.0, 4.0), h.x * 0.5, pal::MARBLE.shade(0.3));
        }
        Bench => {
            g.rounded(o, h, 4.0, pal::WOOD_DARK);
            let mut x = -h.x + 4.0;
            while x < h.x - 2.0 {
                g.rect(o + vec2(x + 2.0, 0.0), vec2(2.4, h.y - 2.0), 0.0, pal::WOOD);
                x += 6.0;
            }
            g.rect(o + vec2(-h.x + 3.0, 0.0), vec2(2.0, h.y), 0.0, pal::BLACK);
            g.rect(o + vec2(h.x - 3.0, 0.0), vec2(2.0, h.y), 0.0, pal::BLACK);
        }
        Counter => {
            g.rect(o, h, 0.0, pal::WALNUT);
            g.rect(o + vec2(0.0, 3.0), h - vec2(0.0, 6.0), 0.0, pal::MARBLE);
            g.rect(o + vec2(0.0, h.y - 2.0), vec2(h.x, 2.0), 0.0, pal::BRASS);
            // A glass screen along it, and a till every three tiles.
            g.rect(
                o + vec2(0.0, -2.0),
                vec2(h.x, 1.0),
                0.0,
                hex(0xa8d8e8).alpha(0.6),
            );
            let n = (h.x * 2.0 / TILE).round() as i32;
            for k in 0..n {
                if k % 3 != 1 {
                    continue;
                }
                let x = -h.x + (k as f32 + 0.5) * TILE;
                g.rect(o + vec2(x, -8.0), vec2(10.0, 5.0), 0.0, pal::BLACK);
                g.rect(
                    o + vec2(x, -6.0),
                    vec2(8.0, 1.5),
                    0.0,
                    pal::SCREEN.glow(1.4),
                );
                g.rect(o + vec2(x + 14.0, -9.0), vec2(6.0, 4.0), 0.0, hex(0x3a3c40));
            }
        }
        Atm => {
            g.rect(o, h, 0.0, hex(0x2a3a5a));
            g.rect(o + vec2(0.0, 2.0), h - Vec2::splat(3.0), 0.0, hex(0x3a5a8a));
            g.rect(
                o + vec2(0.0, h.y - 7.0),
                vec2(9.0, 3.0),
                0.0,
                pal::SCREEN.glow(2.0),
            );
            g.rect(
                o + vec2(0.0, h.y - 2.0),
                vec2(10.0, 1.0),
                0.0,
                hex(0x6aff8a).glow(2.0),
            );
        }
        Cooler => {
            g.rect(o, h, 0.0, hex(0xd8dcdc));
            g.circle(o, h.x * 0.8, hex(0x6ab0e0).alpha(0.9));
            g.circle(o + vec2(-2.0, 2.0), h.x * 0.3, hex(0xbfe8ff));
        }
        Rack => {
            g.rect(o, h, 0.0, hex(0x18191c));
            g.rect(o, h - Vec2::splat(2.5), 0.0, hex(0x26282c));
            let rows = ((h.x * 2.0) / 7.0) as i32;
            for i in 0..rows {
                let x = -h.x + 4.0 + i as f32 * 7.0;
                g.line(
                    o + vec2(x, -h.y + 3.0),
                    o + vec2(x, h.y - 3.0),
                    0.8,
                    hex(0x0e0f10),
                );
                let led = rng.pick(&[0x40ff70, 0x40ff70, 0xffb040, 0x50a0ff]);
                g.rect(
                    o + vec2(x + 2.0, h.y - 4.0),
                    vec2(1.1, 1.1),
                    0.0,
                    hex(led).glow(3.0),
                );
                if rng.chance(0.5) {
                    g.rect(
                        o + vec2(x + 2.0, h.y - 8.0),
                        vec2(1.1, 1.1),
                        0.0,
                        hex(0x40ff70).glow(3.0),
                    );
                }
            }
        }
        Table => {
            g.rounded(o, h, 6.0, pal::WOOD.shade(-0.3));
            g.rounded(
                o + vec2(0.0, 1.0),
                h - Vec2::splat(2.0),
                6.0,
                pal::WOOD.shade(0.1),
            );
            g.circle(o + vec2(-6.0, 4.0), 4.0, pal::WHITE);
            g.circle(o + vec2(-6.0, 4.0), 2.6, hex(0x5a3a22));
            g.rect(o + vec2(8.0, -6.0), vec2(7.0, 5.0), 0.3, pal::PAPER);
        }
        Fridge | Vending => {
            if p.kind == Fridge {
                g.rect(o, h, 0.0, hex(0xb8bcbe));
                g.rect(o, h - Vec2::splat(2.0), 0.0, hex(0xe6e8e8));
                g.line(
                    o + vec2(-h.x + 2.0, -2.0),
                    o + vec2(h.x - 2.0, -2.0),
                    1.0,
                    hex(0x9a9ea0),
                );
                g.rect(
                    o + vec2(h.x - 5.0, h.y - 3.0),
                    vec2(3.0, 1.0),
                    0.0,
                    pal::STEEL_DARK,
                );
            } else {
                g.rect(o, h, 0.0, hex(0x8c1a1a));
                g.rect(
                    o + vec2(-3.0, 2.0),
                    vec2(h.x - 7.0, h.y - 4.0),
                    0.0,
                    hex(0xffe8c0).glow(1.6),
                );
                for k in 0..4 {
                    let c = rng.pick(&[0xe03030, 0x30a0e0, 0xe0c030, 0x30c060]);
                    g.rect(
                        o + vec2(-h.x + 8.0 + k as f32 * 6.0, 2.0),
                        vec2(2.0, 2.0),
                        0.0,
                        hex(c),
                    );
                }
            }
        }
        SinkCounter => {
            g.rect(o, h, 0.0, hex(0xc8c4bc));
            let n = (h.x * 2.0 / TILE).round().max(1.0) as i32;
            for k in 0..n {
                let x = -h.x + (k as f32 + 0.5) * TILE;
                g.ellipse(o + vec2(x, 0.0), vec2(10.0, 7.0), 0.0, hex(0xeeeeee));
                g.ellipse(o + vec2(x, 0.0), vec2(7.0, 4.5), 0.0, hex(0xb8c4cc));
                g.rect(o + vec2(x, -h.y + 3.0), vec2(1.5, 3.0), 0.0, pal::STEEL);
            }
        }
        Toilet => {
            // The stall's partitions either side, then the bowl.
            let half = TILE * 0.5;
            g.frame.origin = p.block.centre();
            let back = -half;
            g.rect(vec2(-half + 1.5, 0.0), vec2(1.5, half), 0.0, hex(0x9aa4a8));
            g.rect(vec2(half - 1.5, 0.0), vec2(1.5, half), 0.0, hex(0x9aa4a8));
            g.rect(
                vec2(0.0, back + 4.0),
                vec2(9.0, 4.0),
                0.0,
                pal::WHITE.shade(-0.1),
            );
            g.ellipse(vec2(0.0, back + 16.0), vec2(8.0, 10.0), 0.0, pal::WHITE);
            g.ellipse(vec2(0.0, back + 17.0), vec2(5.0, 6.5), 0.0, hex(0xa8c8d8));
        }
        Pallet => {
            g.rect(o, h, 0.0, hex(0x9a7a4a));
            for (i, j) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                let at = vec2(i, j) * (h * 0.48);
                g.rect(o + at, h * 0.46, 0.0, pal::CASH.shade(-0.2));
                g.rect(o + at + vec2(0.0, 1.0), h * 0.42, 0.0, pal::CASH);
                g.rect(o + at, vec2(h.x * 0.1, h.y * 0.42), 0.0, hex(0xe8e0c8));
            }
        }
        Gold => {
            for (i, at) in [vec2(-6.0, -5.0), vec2(6.0, -5.0), vec2(0.0, 5.0)]
                .iter()
                .enumerate()
            {
                let c = if i == 2 {
                    pal::GOLD.shade(0.2)
                } else {
                    pal::GOLD
                };
                g.rect(o + *at, vec2(7.0, 4.0), 0.0, pal::GOLD.shade(-0.35));
                g.rect(o + *at + vec2(0.0, 0.5), vec2(5.5, 2.8), 0.0, c.glow(1.15));
            }
        }
        Safe => {
            g.rect(o, h, 0.0, hex(0x2c3034));
            g.rect(o, h - Vec2::splat(2.5), 0.0, hex(0x40464c));
            g.circle(o + vec2(0.0, 1.0), 7.0, pal::STEEL);
            g.circle(o + vec2(0.0, 1.0), 4.0, hex(0x2c3034));
            g.line(o + vec2(-2.0, 1.0), o + vec2(2.0, 1.0), 1.2, pal::BRASS);
        }
        MonitorDesk => {
            g.rect(o, h, 0.0, hex(0x2a2c30));
            let n = ((h.x * 2.0) / 22.0) as i32;
            for k in 0..n {
                let x = -h.x + 11.0 + k as f32 * 22.0;
                g.rect(o + vec2(x, -h.y + 7.0), vec2(9.5, 4.0), 0.0, pal::BLACK);
                let c = rng.pick(&[0x7ad0ff, 0x9affb0, 0x7ad0ff, 0xd0d0ff]);
                g.rect(
                    o + vec2(x, -h.y + 9.0),
                    vec2(8.0, 2.0),
                    0.0,
                    hex(c).glow(1.4),
                );
            }
            g.rect(
                o + vec2(0.0, h.y - 6.0),
                vec2(14.0, 3.0),
                0.0,
                hex(0x18191c),
            );
        }
        Sofa => {
            g.rounded(o, h, 6.0, pal::LEATHER.shade(-0.3));
            g.rounded(
                o + vec2(0.0, 3.0),
                vec2(h.x - 7.0, h.y - 6.0),
                4.0,
                pal::LEATHER,
            );
            g.line(
                o + vec2(0.0, -h.y + 6.0),
                o + vec2(0.0, h.y - 3.0),
                0.8,
                pal::LEATHER.shade(-0.4),
            );
        }
        Copier => {
            g.rect(o, h, 0.0, hex(0xb4b8b8));
            g.rect(
                o + vec2(0.0, -3.0),
                vec2(h.x - 3.0, h.y - 7.0),
                0.0,
                hex(0xdadcdc),
            );
            g.rect(
                o + vec2(h.x - 6.0, h.y - 4.0),
                vec2(3.0, 2.0),
                0.0,
                hex(0x60ff90).glow(2.0),
            );
        }
        Rug => {
            g.frame.origin = p.block.centre();
            let hb = (p.block.max - p.block.min) * 0.5 - Vec2::splat(6.0);
            g.rect(Vec2::ZERO, hb, 0.0, pal::RUG.shade(-0.2));
            g.rect(Vec2::ZERO, hb - Vec2::splat(5.0), 0.0, pal::RUG);
            g.rect(
                Vec2::ZERO,
                hb - Vec2::splat(16.0),
                0.0,
                pal::BRASS.shade(-0.3),
            );
            g.rect(
                Vec2::ZERO,
                hb - Vec2::splat(19.0),
                0.0,
                pal::RUG.shade(0.08),
            );
        }
        Stanchion => {
            g.frame.origin = p.block.centre();
            let post = vec2(-TILE * 0.5, 0.0);
            g.line(post, post + vec2(TILE, 0.0), 2.4, pal::VELVET);
            g.circle(post, 4.5, pal::BRASS.shade(-0.3));
            g.circle(post, 3.0, pal::BRASS);
        }
        VaultDoor => {
            // Swung open into the vault against its wall: a thick round door.
            g.frame = Frame {
                origin: p.block.centre(),
                ..Frame::IDENTITY
            };
            let into = -face_dir(p.face); // into the vault
            let across = vec2(into.y, -into.x);
            let hinge = across * TILE + into * 8.0;
            let centre = hinge - across * 6.0 + into * 40.0;
            g.ellipse(centre, vec2(36.0, 9.0), into.y.atan2(into.x), hex(0x6a7078));
            g.ellipse(centre, vec2(33.0, 6.5), into.y.atan2(into.x), hex(0x9aa2aa));
            g.rect(hinge, vec2(5.0, 5.0), 0.0, hex(0x3a3e44));
            // The bolts of the frame along the threshold.
            for k in -3..=3 {
                g.circle(across * (k as f32 * 13.0) - into * 20.0, 2.4, pal::STEEL);
            }
        }
        Tree => {
            g.circle(o, 9.0, hex(0x4a3424));
            g.circle(o, 6.0, hex(0x5e4430));
        }
        Bush => {
            for i in 0..6 {
                let a = i as f32 / 6.0 * TAU + rng.f32();
                g.circle(
                    o + vec2(a.cos(), a.sin()) * 10.0,
                    12.0,
                    pal::LEAF_DARK.shade(rng.f32() * 0.2),
                );
            }
            g.circle(o, 12.0, pal::LEAF);
        }
        Car => paint_car(g, h, &mut rng, false),
        Van => paint_car(g, h, &mut rng, true),
        StreetLamp => {
            g.circle(o, 7.0, hex(0x2a2c30));
            g.circle(o, 4.5, hex(0x40444a));
        }
        Hydrant => {
            g.circle(o, 9.0, hex(0x9a1818));
            g.circle(o, 6.0, hex(0xd02a24));
            g.rect(o + vec2(0.0, 0.0), vec2(11.0, 2.5), 0.0, hex(0xb02020));
            g.circle(o, 2.5, pal::BRASS);
        }
        Bin => {
            g.circle(o, 11.0, hex(0x2a3a2a));
            g.circle(o, 9.0, hex(0x3a4c3a));
            g.circle(o, 5.0, hex(0x1a1c1a));
        }
    }
}

/// What stands over the player: a tree's crown, a lamp's head.
fn paint_high(p: &Prop, g: &mut Painter) {
    let mut rng = Rng::new(p.seed as u64 ^ 0xabc);
    g.frame = Frame {
        origin: p.block.centre(),
        ..Frame::IDENTITY
    };
    match p.kind {
        PropKind::Tree => {
            let r = 38.0 + rng.f32() * 12.0;
            g.circle(vec2(6.0, -8.0), r, hex(0x000000).alpha(0.22));
            for i in 0..9 {
                let a = i as f32 / 9.0 * TAU + rng.f32() * 0.5;
                let d = r * (0.45 + rng.f32() * 0.2);
                g.circle(
                    vec2(a.cos(), a.sin()) * d,
                    r * 0.5,
                    pal::LEAF_DARK.shade(rng.f32() * 0.1),
                );
            }
            g.circle(Vec2::ZERO, r * 0.7, pal::LEAF);
            for _ in 0..5 {
                let at = vec2(rng.f32() - 0.5, rng.f32() - 0.5) * r * 0.9;
                g.circle(at, r * 0.25, pal::LEAF.shade(0.12));
            }
        }
        PropKind::StreetLamp => {
            // An arm out over the pavement and the head.
            let head = vec2(0.0, -12.0);
            g.line(Vec2::ZERO, head, 4.0, hex(0x2a2c30));
            g.rounded(head, vec2(12.0, 6.0), 3.0, hex(0x30343a));
            g.rounded(head, vec2(9.0, 3.5), 2.0, hex(0xffc070).glow(4.0));
        }
        _ => {}
    }
}

fn paint_car(g: &mut Painter, h: Vec2, rng: &mut Rng, van: bool) {
    let colour = if van {
        hex(0xe8e6e0)
    } else {
        hex(rng.pick(&[0x8a1c1c, 0x1c3a6a, 0x2a2a2e, 0xc8c8c4, 0x3a5a3a, 0xb07a2a]))
    };
    let o = Vec2::ZERO;
    g.rounded(o, h, 10.0, colour.shade(-0.4));
    g.rounded(o, h - Vec2::splat(2.0), 9.0, colour);
    // Wheels peek out at the sides.
    for (sx, sy) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
        g.rect(
            o + vec2(sx * (h.x - 1.0), sy * h.y * 0.62),
            vec2(3.0, 11.0),
            0.0,
            pal::BLACK,
        );
    }
    if van {
        // Cab at the front, a long roof, the company name for nobody.
        g.rounded(
            o + vec2(0.0, h.y - 34.0),
            vec2(h.x - 6.0, 16.0),
            6.0,
            hex(0x2a3440),
        );
        g.rounded(
            o + vec2(0.0, -24.0),
            vec2(h.x - 5.0, h.y - 46.0),
            4.0,
            colour.shade(0.12),
        );
        g.rect(
            o + vec2(0.0, -24.0),
            vec2(h.x - 14.0, 2.0),
            0.0,
            hex(0x3a6a9a),
        );
        for k in 0..4 {
            g.line(
                o + vec2(-h.x + 10.0, -h.y + 30.0 + k as f32 * 36.0),
                o + vec2(h.x - 10.0, -h.y + 30.0 + k as f32 * 36.0),
                1.0,
                colour.shade(-0.15),
            );
        }
    } else {
        g.rounded(
            o + vec2(0.0, h.y * 0.32),
            vec2(h.x - 7.0, 14.0),
            6.0,
            hex(0x2a3440),
        );
        g.rounded(
            o + vec2(0.0, -h.y * 0.12),
            vec2(h.x - 6.0, h.y * 0.32),
            6.0,
            colour.shade(0.1),
        );
        g.rounded(
            o + vec2(0.0, -h.y * 0.55),
            vec2(h.x - 8.0, 10.0),
            5.0,
            hex(0x2a3440),
        );
    }
    // Headlamps and tail-lamps, dark.
    g.rect(
        o + vec2(-h.x + 9.0, h.y - 3.0),
        vec2(6.0, 2.0),
        0.0,
        hex(0xe8e8d8),
    );
    g.rect(
        o + vec2(h.x - 9.0, h.y - 3.0),
        vec2(6.0, 2.0),
        0.0,
        hex(0xe8e8d8),
    );
    g.rect(
        o + vec2(-h.x + 8.0, -h.y + 3.0),
        vec2(5.0, 2.0),
        0.0,
        hex(0x8a1010),
    );
    g.rect(
        o + vec2(h.x - 8.0, -h.y + 3.0),
        vec2(5.0, 2.0),
        0.0,
        hex(0x8a1010),
    );
}

// --- the robber ------------------------------------------------------------

/// The robber's parts, each its own mesh so the picture can move them: in
/// the body's own frame, `+x` forward (where it faces) and `+y` its left.
pub struct RobberParts {
    pub shadow: Mesh,
    pub foot: Mesh,
    pub bag: Mesh,
    pub torso: Mesh,
    pub arm: Mesh,
    pub crowbar: Mesh,
    pub head: Mesh,
    pub streak: Mesh,
}

const SKIN: Hex = hex(0xd8a47a);
const BEANIE: Hex = hex(0x1c1c22);
const STRIPE_DARK: Hex = hex(0x1d1e24);
const STRIPE_LIGHT: Hex = hex(0xe6e0d2);
const BURLAP: Hex = hex(0xb08a54);

pub fn robber() -> RobberParts {
    let build = |f: &dyn Fn(&mut Painter)| {
        let mut g = Painter::default();
        f(&mut g);
        g.build()
    };
    RobberParts {
        shadow: build(&|g| {
            g.circle(vec2(-2.0, -2.0), 24.0, hex(0x000000).alpha(0.12));
            g.circle(vec2(-2.0, -2.0), 20.0, hex(0x000000).alpha(0.18));
        }),
        foot: build(&|g| {
            g.ellipse(Vec2::ZERO, vec2(8.5, 5.5), 0.0, hex(0x16120f));
            g.ellipse(vec2(2.0, 0.0), vec2(5.0, 3.6), 0.0, hex(0x2c2420));
        }),
        bag: build(&|g| {
            g.circle(Vec2::ZERO, 12.5, BURLAP.shade(-0.4));
            g.circle(Vec2::ZERO, 11.0, BURLAP);
            g.circle(vec2(-2.5, 2.5), 5.0, BURLAP.shade(0.15));
            // A dollar sign, of sorts, and the tie.
            g.line(vec2(-4.0, -4.0), vec2(4.0, 4.0), 2.2, hex(0x2a5a2a));
            g.line(vec2(-1.0, -6.0), vec2(-1.0, 6.0), 1.2, hex(0x2a5a2a));
            g.circle(vec2(9.0, 0.0), 3.0, BURLAP.shade(-0.3));
            g.rect(vec2(6.0, 6.0), vec2(4.0, 2.2), 0.4, hex(0x7ab06a));
        }),
        torso: build(&|g| striped_ellipse(g, vec2(11.5, 17.0))),
        arm: build(&|g| {
            g.capsule(Vec2::ZERO, vec2(14.0, 0.0), 8.5, STRIPE_DARK);
            g.line(vec2(4.0, 0.0), vec2(7.0, 0.0), 8.0, STRIPE_LIGHT);
            g.circle(vec2(15.0, 0.0), 4.8, hex(0x111114));
        }),
        crowbar: build(&|g| {
            g.line(vec2(0.0, 0.0), vec2(26.0, 0.0), 3.0, hex(0x9a2a1e));
            g.line(vec2(26.0, 0.0), vec2(30.0, 4.0), 3.0, hex(0x9a2a1e));
            g.line(vec2(-3.0, 0.0), vec2(0.0, 0.0), 3.0, hex(0x6a6e74));
        }),
        head: build(&|g| {
            g.circle(Vec2::ZERO, 10.5, SKIN.shade(-0.25));
            g.circle(vec2(0.5, 0.0), 9.8, SKIN);
            // The mask over the eyes, the beanie over the rest.
            g.ellipse(vec2(6.0, 0.0), vec2(3.5, 8.6), 0.0, hex(0x0e0e10));
            g.circle(vec2(-1.8, 0.0), 9.6, BEANIE);
            g.ellipse(vec2(3.6, 0.0), vec2(2.0, 9.0), 0.0, BEANIE.shade(0.15));
            g.line(vec2(-8.0, 0.0), vec2(2.0, 0.0), 1.0, BEANIE.shade(0.1));
            g.circle(vec2(-4.0, 0.0), 3.8, hex(0x8c1f2a));
            g.circle(vec2(-4.6, 0.6), 1.8, hex(0xb03040));
        }),
        streak: build(&|g| {
            g.line(
                vec2(-16.0, 0.0),
                vec2(0.0, 0.0),
                2.0,
                hex(0xffffff).alpha(0.22),
            );
        }),
    }
}

/// The torso: an ellipse in bands, black and white, across the shoulders.
fn striped_ellipse(g: &mut Painter, r: Vec2) {
    g.ellipse(
        Vec2::ZERO,
        r + Vec2::splat(1.2),
        0.0,
        STRIPE_DARK.shade(-0.4),
    );
    let bands = 7;
    for b in 0..bands {
        let x0 = -r.x + 2.0 * r.x * b as f32 / bands as f32;
        let x1 = -r.x + 2.0 * r.x * (b + 1) as f32 / bands as f32;
        let colour = if b % 2 == 1 {
            STRIPE_LIGHT
        } else {
            STRIPE_DARK
        };
        let steps = 6;
        let mut top = Vec::new();
        for i in 0..=steps {
            let x = x0 + (x1 - x0) * i as f32 / steps as f32;
            let y = r.y * (1.0 - (x / r.x).powi(2)).max(0.0).sqrt();
            top.push(vec2(x, y));
        }
        let mut pts = top.clone();
        for p in top.iter().rev() {
            pts.push(vec2(p.x, -p.y));
        }
        g.poly(&pts, colour);
    }
    // The collar at the front.
    g.ellipse(vec2(r.x - 3.0, 0.0), vec2(2.5, 6.0), 0.0, STRIPE_DARK);
}
