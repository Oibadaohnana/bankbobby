//! The bank, generated from a seed.
//!
//! A city block at night: a road along the south with the getaway van at
//! the kerb, a pavement, the bank, a car park behind it and grass with trees
//! down the sides. The bank is laid out in bands, front to back:
//!
//! - the **lobby** across the front (marble, pillars, a queue, the teller
//!   counter with a gap at one end and the tellers' wood floor behind it),
//!   with an office or a washroom in either front corner;
//! - a **corridor** right across the building, a side door at one end;
//! - the **back rooms**, split up by a BSP: an open-plan office, the
//!   manager's office, a break room, washrooms, a server room, security,
//!   an archive — and the **vault**, steel-walled, one door.
//!
//! Everything is on a grid of [`TILE`]-sized tiles. A wall is a whole tile;
//! furniture is a box inside its tiles. What the generator hands on:
//!
//! - the tiles (floor, wall, a tint, decoration flags), drawn by the ground
//!   shader;
//! - the props, drawn as meshes, each with the box it blocks a body with
//!   and the box it blocks sight with;
//! - the lights;
//! - [`Map::solids`] for the movement, and [`Map::occluders`] for the
//!   ray tracer: per tile the box that stops a ray, and which object it is.
//!
//! Every room is reachable from the street: the walls are checked with a
//! flood fill and a door punched where a room was shut off, and furniture
//! that walls a door in is taken out again.

#[cfg(test)]
use crate::movement::BODY_MARGIN;
use crate::movement::{Aabb, TILE};
use crate::rng::Rng;
use bevy::math::{Vec2, vec2};
use std::collections::VecDeque;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Floor {
    Grass = 0,
    Asphalt = 1,
    Sidewalk = 2,
    Parking = 3,
    Marble = 4,
    Carpet = 5,
    Wood = 6,
    WashroomTile = 7,
    Concrete = 8,
    Steel = 9,
    Lino = 10,
    Raised = 11,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Wall {
    None = 0,
    Brick = 1,
    Drywall = 2,
    Vault = 3,
    Glass = 4,
}

/// Decoration flags on a tile, read by the ground shader.
pub mod deco {
    /// A doorway: a threshold strip.
    pub const DOOR: u8 = 1;
    /// The road's centre line along the tile's bottom edge.
    pub const LANE: u8 = 2;
    /// A parking bay's line along the tile's left edge.
    pub const BAY: u8 = 4;
    /// A zebra crossing.
    pub const ZEBRA: u8 = 8;
    /// The vault's doorway.
    pub const VAULT_DOOR: u8 = 16;
    /// A runner rug.
    pub const RUG: u8 = 32;
}

#[derive(Clone, Copy, Debug)]
pub struct Tile {
    pub floor: Floor,
    pub wall: Wall,
    /// A colour variant: a carpet's dye, a marble's shade.
    pub tint: u8,
    pub deco: u8,
    /// The room it is in, `NO_ROOM` outside or in a wall.
    pub room: u16,
    pub indoor: bool,
}

pub const NO_ROOM: u16 = u16::MAX;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoomKind {
    Lobby,
    Corridor,
    OpenOffice,
    Office,
    Manager,
    Break,
    Washroom,
    Server,
    Security,
    Archive,
    Vault,
}

#[derive(Clone, Copy, Debug)]
pub struct Room {
    pub kind: RoomKind,
    /// Its floor tiles, inclusive.
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl Room {
    fn w(&self) -> i32 {
        self.x1 - self.x0 + 1
    }
    fn h(&self) -> i32 {
        self.y1 - self.y0 + 1
    }
    fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x0 && x <= self.x1 && y >= self.y0 && y <= self.y1
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Light {
    pub pos: Vec2,
    pub radius: f32,
    pub color: [f32; 3],
    pub intensity: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PropKind {
    Desk,
    BigDesk,
    Chair,
    Cabinet,
    Bookshelf,
    Plant,
    Pillar,
    Bench,
    Counter,
    Atm,
    Cooler,
    Rack,
    Table,
    Fridge,
    Vending,
    SinkCounter,
    Toilet,
    Lockers,
    Pallet,
    Gold,
    Safe,
    MonitorDesk,
    Sofa,
    Copier,
    Rug,
    Stanchion,
    VaultDoor,
    Tree,
    Car,
    Van,
    StreetLamp,
    Hydrant,
    Bin,
    Bush,
}

/// Which way a prop's front faces: north (+y), east, south, west.
pub type Face = u8;
pub const NORTH: Face = 0;
pub const EAST: Face = 1;
pub const SOUTH: Face = 2;
pub const WEST: Face = 3;

#[derive(Clone, Copy, Debug)]
pub struct Prop {
    pub kind: PropKind,
    /// The tiles it stands on, in world units.
    pub block: Aabb,
    /// The thing itself inside them.
    pub body: Aabb,
    pub face: Face,
    pub seed: u32,
    pub solid: bool,
    pub opaque: bool,
    /// The room it was placed in, for taking it out again.
    room: u16,
}

pub fn face_dir(face: Face) -> Vec2 {
    match face {
        NORTH => vec2(0.0, 1.0),
        EAST => vec2(1.0, 0.0),
        SOUTH => vec2(0.0, -1.0),
        _ => vec2(-1.0, 0.0),
    }
}

/// How a kind sits in its block: backed against the wall behind it or
/// centred, how far in from the ends, how deep, and whether it stops a
/// body and a ray.
struct Spec {
    backed: bool,
    inset: f32,
    depth: f32,
    solid: bool,
    opaque: bool,
}

fn spec(kind: PropKind) -> Spec {
    use PropKind::*;
    let s = |backed, inset, depth, solid, opaque| Spec {
        backed,
        inset,
        depth,
        solid,
        opaque,
    };
    match kind {
        Desk => s(false, 3.0, 34.0, true, false),
        BigDesk => s(false, 2.0, 40.0, true, false),
        Chair => s(false, 14.0, 24.0, false, false),
        Cabinet => s(true, 7.0, 32.0, true, true),
        Bookshelf => s(true, 2.0, 22.0, true, true),
        Plant => s(false, 12.0, 28.0, true, false),
        Pillar => s(false, 6.0, 40.0, true, true),
        Bench => s(false, 4.0, 22.0, true, false),
        Counter => s(false, 0.0, 30.0, true, false),
        Atm => s(true, 9.0, 26.0, true, true),
        Cooler => s(true, 14.0, 22.0, true, false),
        Rack => s(false, 5.0, 34.0, true, true),
        Table => s(false, 12.0, 80.0, true, false),
        Fridge => s(true, 6.0, 36.0, true, true),
        Vending => s(true, 4.0, 34.0, true, true),
        SinkCounter => s(true, 0.0, 26.0, true, false),
        Toilet => s(true, 14.0, 30.0, true, false),
        Lockers => s(true, 0.0, 18.0, true, true),
        Pallet => s(false, 6.0, 40.0, true, false),
        Gold => s(false, 12.0, 26.0, true, false),
        Safe => s(true, 8.0, 36.0, true, true),
        MonitorDesk => s(true, 2.0, 30.0, true, false),
        Sofa => s(true, 4.0, 30.0, true, false),
        Copier => s(true, 8.0, 30.0, true, false),
        Rug | Stanchion | VaultDoor => s(false, 0.0, TILE, false, false),
        Tree => s(false, 18.0, 16.0, true, true),
        Car => s(false, 8.0, 5.0 * TILE - 14.0, true, false),
        Van => s(false, 6.0, 6.0 * TILE - 10.0, true, true),
        StreetLamp => s(false, 21.0, 10.0, true, false),
        Hydrant => s(false, 17.0, 18.0, true, false),
        Bin => s(false, 14.0, 24.0, true, false),
        Bush => s(false, 4.0, 40.0, true, false),
    }
}

pub struct Map {
    pub w: i32,
    pub h: i32,
    pub tiles: Vec<Tile>,
    pub rooms: Vec<Room>,
    pub props: Vec<Prop>,
    pub lights: Vec<Light>,
    pub spawn: Vec2,
    /// The building's tiles, walls included.
    pub building: (i32, i32, i32, i32),
}

/// Per tile, what stops a ray: a box inside the tile in 0..=255 of the
/// tile each way (min x, min y, max x, max y; empty when max ≤ min), and
/// the object it belongs to (0 for a wall, which is its own tile).
pub struct Occluders {
    pub boxes: Vec<[u8; 4]>,
    pub objects: Vec<u8>,
}

impl Map {
    pub fn idx(&self, x: i32, y: i32) -> usize {
        (y * self.w + x) as usize
    }

    pub fn inside(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && x < self.w && y < self.h
    }

    pub fn at(&self, x: i32, y: i32) -> Tile {
        self.tiles[self.idx(x, y)]
    }

    fn at_mut(&mut self, x: i32, y: i32) -> &mut Tile {
        let i = self.idx(x, y);
        &mut self.tiles[i]
    }

    pub fn is_wall(&self, x: i32, y: i32) -> bool {
        self.inside(x, y) && self.at(x, y).wall != Wall::None
    }

    pub fn bounds(&self) -> Aabb {
        Aabb::new(Vec2::ZERO, vec2(self.w as f32, self.h as f32) * TILE)
    }

    /// Everything a body bumps into: wall tiles, then the props.
    pub fn solids(&self) -> Vec<Aabb> {
        let mut out = Vec::new();
        for y in 0..self.h {
            for x in 0..self.w {
                if self.at(x, y).wall != Wall::None {
                    out.push(tile_box(x, y));
                }
            }
        }
        out.extend(self.props.iter().filter(|p| p.solid).map(|p| p.body));
        out
    }

    pub fn occluders(&self) -> Occluders {
        let n = (self.w * self.h) as usize;
        let mut boxes = vec![[0u8; 4]; n];
        let mut objects = vec![0u8; n];
        for y in 0..self.h {
            for x in 0..self.w {
                let wall = self.at(x, y).wall;
                if wall != Wall::None && wall != Wall::Glass {
                    boxes[self.idx(x, y)] = [0, 0, 255, 255];
                }
            }
        }
        let mut id = 0u8;
        for p in self.props.iter().filter(|p| p.opaque) {
            id = if id >= 254 { 1 } else { id + 1 };
            let b = p.body;
            let (x0, y0) = tile_of(b.min);
            let (x1, y1) = tile_of(b.max - Vec2::splat(0.01));
            for y in y0.max(0)..=y1.min(self.h - 1) {
                for x in x0.max(0)..=x1.min(self.w - 1) {
                    let t = tile_box(x, y);
                    let lo = (b.min.max(t.min) - t.min) / TILE;
                    let hi = (b.max.min(t.max) - t.min) / TILE;
                    if hi.x <= lo.x || hi.y <= lo.y {
                        continue;
                    }
                    let q = |v: f32, up: bool| -> u8 {
                        let s = v * 255.0;
                        (if up { s.ceil() } else { s.floor() }).clamp(0.0, 255.0) as u8
                    };
                    let i = self.idx(x, y);
                    let old = boxes[i];
                    let new = [q(lo.x, false), q(lo.y, false), q(hi.x, true), q(hi.y, true)];
                    boxes[i] = if old[2] > old[0] {
                        [
                            old[0].min(new[0]),
                            old[1].min(new[1]),
                            old[2].max(new[2]),
                            old[3].max(new[3]),
                        ]
                    } else {
                        new
                    };
                    objects[i] = id;
                }
            }
        }
        Occluders { boxes, objects }
    }
}

pub fn tile_box(x: i32, y: i32) -> Aabb {
    let min = vec2(x as f32, y as f32) * TILE;
    Aabb::new(min, min + Vec2::splat(TILE))
}

pub fn tile_of(p: Vec2) -> (i32, i32) {
    ((p.x / TILE).floor() as i32, (p.y / TILE).floor() as i32)
}

pub fn tile_centre(x: i32, y: i32) -> Vec2 {
    (vec2(x as f32, y as f32) + Vec2::splat(0.5)) * TILE
}

// --- lights ----------------------------------------------------------------

const WARM: [f32; 3] = [1.0, 0.82, 0.58];
const FLUORESCENT: [f32; 3] = [0.82, 0.92, 1.0];
const SODIUM: [f32; 3] = [1.0, 0.62, 0.28];
const SERVER_BLUE: [f32; 3] = [0.45, 0.7, 1.0];
const VAULT_RED: [f32; 3] = [1.0, 0.32, 0.22];

// --- the generator ---------------------------------------------------------

struct Gen {
    map: Map,
    rng: Rng,
    /// Tiles a prop stands on (solid ones only).
    used: Vec<bool>,
    /// Tiles kept clear in front of doors.
    clear: Vec<bool>,
}

pub fn generate(seed: u64) -> Map {
    let mut rng = Rng::new(seed);
    let bw = rng.range(46, 61);
    let bh = rng.range(34, 43);
    let side = rng.range(7, 10);
    let road = 6;
    let pavement = 3;
    let park = rng.range(11, 14);
    let w = bw + 2 * side;
    let h = road + pavement + bh + park;
    let bx0 = side;
    let by0 = road + pavement;
    let bx1 = bx0 + bw - 1;
    let by1 = by0 + bh - 1;
    let n = (w * h) as usize;
    let map = Map {
        w,
        h,
        tiles: vec![
            Tile {
                floor: Floor::Grass,
                wall: Wall::None,
                tint: 0,
                deco: 0,
                room: NO_ROOM,
                indoor: false,
            };
            n
        ],
        rooms: Vec::new(),
        props: Vec::new(),
        lights: Vec::new(),
        spawn: Vec2::ZERO,
        building: (bx0, by0, bx1, by1),
    };
    let mut g = Gen {
        map,
        rng,
        used: vec![false; n],
        clear: vec![false; n],
    };
    g.outside(road, pavement);
    let entrance = g.building();
    g.fix_walls();
    g.mark_clear();
    g.furnish();
    g.street(road, entrance);
    g.fix_props();
    for i in 0..n {
        let r = g.rng.u32();
        if g.map.tiles[i].tint == 0 {
            g.map.tiles[i].tint = (r & 0xff) as u8;
        }
    }
    g.map
}

impl Gen {
    fn set_floor(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, floor: Floor, tint: u8) {
        for y in y0..=y1 {
            for x in x0..=x1 {
                if self.map.inside(x, y) {
                    let t = self.map.at_mut(x, y);
                    t.floor = floor;
                    t.tint = tint;
                }
            }
        }
    }

    fn set_wall(&mut self, x: i32, y: i32, wall: Wall) {
        if self.map.inside(x, y) {
            let t = self.map.at_mut(x, y);
            t.wall = wall;
            t.deco &= !(deco::DOOR | deco::VAULT_DOOR);
        }
    }

    fn door(&mut self, x: i32, y: i32, floor: Floor, tint: u8) {
        let t = self.map.at_mut(x, y);
        t.wall = Wall::None;
        t.floor = floor;
        t.tint = tint;
        t.deco |= deco::DOOR;
    }

    fn add_room(&mut self, kind: RoomKind, x0: i32, y0: i32, x1: i32, y1: i32) -> u16 {
        let id = self.map.rooms.len() as u16;
        self.map.rooms.push(Room {
            kind,
            x0,
            y0,
            x1,
            y1,
        });
        for y in y0..=y1 {
            for x in x0..=x1 {
                self.map.at_mut(x, y).room = id;
            }
        }
        id
    }

    fn light(&mut self, x: f32, y: f32, radius_tiles: f32, color: [f32; 3], intensity: f32) {
        self.map.lights.push(Light {
            pos: vec2(x, y) * TILE,
            radius: radius_tiles * TILE,
            color,
            intensity,
        });
    }

    /// The road, the pavement, the car park and the grass.
    fn outside(&mut self, road: i32, pavement: i32) {
        let (bx0, _, bx1, by1) = self.map.building;
        let (w, h) = (self.map.w, self.map.h);
        self.set_floor(0, 0, w - 1, road - 1, Floor::Asphalt, 0);
        for x in 0..w {
            self.map.at_mut(x, road / 2).deco |= deco::LANE;
        }
        let zebra = if self.rng.chance(0.5) {
            bx0 - 4
        } else {
            bx1 + 2
        };
        for y in 0..road {
            for x in zebra..zebra + 3 {
                if self.map.inside(x, y) {
                    self.map.at_mut(x, y).deco |= deco::ZEBRA;
                }
            }
        }
        self.set_floor(0, road, w - 1, road + pavement - 1, Floor::Sidewalk, 0);
        // A pavement round the building.
        self.set_floor(
            bx0 - 2,
            road + pavement,
            bx0 - 1,
            by1 + 2,
            Floor::Sidewalk,
            0,
        );
        self.set_floor(
            bx1 + 1,
            road + pavement,
            bx1 + 2,
            by1 + 2,
            Floor::Sidewalk,
            0,
        );
        self.set_floor(bx0 - 2, by1 + 1, bx1 + 2, by1 + 2, Floor::Sidewalk, 0);
        // The car park behind it, two rows of bays.
        let p0 = by1 + 3;
        let p1 = h - 2;
        self.set_floor(bx0 - 4, p0, bx1 + 4, p1, Floor::Parking, 0);
        let rows = [(p0 + 1, p0 + 5), (p1 - 4, p1)];
        for (r0, r1) in rows {
            if r1 - r0 < 4 || r0 < p0 {
                continue;
            }
            let mut x = bx0 - 2;
            while x <= bx1 + 2 {
                for y in r0..=r1 {
                    self.map.at_mut(x, y).deco |= deco::BAY;
                }
                x += 3;
            }
        }
    }

    /// The building's shell and its rooms. Returns the entrance's middle
    /// tile x.
    fn building(&mut self) -> i32 {
        let (bx0, by0, bx1, by1) = self.map.building;
        for y in by0..=by1 {
            for x in bx0..=bx1 {
                let t = self.map.at_mut(x, y);
                t.floor = Floor::Concrete;
                t.indoor = true;
                t.deco = 0;
                if x == bx0 || x == bx1 || y == by0 || y == by1 {
                    t.wall = Wall::Brick;
                }
            }
        }
        let (ix0, iy0, ix1, iy1) = (bx0 + 1, by0 + 1, bx1 - 1, by1 - 1);
        let ih = iy1 - iy0 + 1;
        let depth = ((ih as f32 * 0.37) as i32).clamp(11, 15);
        let ly = iy0 + depth; // the lobby's back wall
        let (cy0, cy1) = (ly + 1, ly + 3); // the corridor
        let ry = cy1 + 1; // the back rooms' front wall
        for x in ix0..=ix1 {
            self.set_wall(x, ly, Wall::Drywall);
            self.set_wall(x, ry, Wall::Drywall);
        }

        // The front corners: nothing, an office, or an office over a washroom.
        let corner = |g: &mut Gen, left: bool| -> i32 {
            if !g.rng.chance(0.75) {
                return 0;
            }
            let cw = g.rng.range(7, 11);
            let wx = if left { ix0 + cw } else { ix1 - cw };
            for y in iy0..ly {
                g.set_wall(wx, y, Wall::Drywall);
            }
            let (rx0, rx1) = if left { (ix0, wx - 1) } else { (wx + 1, ix1) };
            let split = depth >= 12 && g.rng.chance(0.6);
            let door_x = wx;
            let tint = g.rng.u32() as u8;
            if split {
                let my = iy0 + depth / 2;
                for x in rx0..=rx1 {
                    g.set_wall(x, my, Wall::Drywall);
                }
                g.set_floor(rx0, iy0, rx1, my - 1, Floor::Carpet, tint);
                g.add_room(RoomKind::Office, rx0, iy0, rx1, my - 1);
                g.set_floor(rx0, my + 1, rx1, ly - 1, Floor::WashroomTile, 0);
                g.add_room(RoomKind::Washroom, rx0, my + 1, rx1, ly - 1);
                let d0 = g.rng.range(iy0 + 1, my - 1);
                g.door(door_x, d0, Floor::Carpet, tint);
                let d1 = g.rng.range(my + 1, ly - 1);
                g.door(door_x, d1, Floor::WashroomTile, 0);
            } else {
                g.set_floor(rx0, iy0, rx1, ly - 1, Floor::Carpet, tint);
                g.add_room(RoomKind::Office, rx0, iy0, rx1, ly - 1);
                let d = g.rng.range(iy0 + 2, ly - 5);
                g.door(door_x, d, Floor::Carpet, tint);
            }
            cw + 1
        };
        let sl = corner(self, true);
        let sr = corner(self, false);
        let (lx0, lx1) = (ix0 + sl, ix1 - sr);

        // The lobby: marble out front, the counter, wood behind it.
        self.set_floor(lx0, iy0, lx1, ly - 1, Floor::Marble, 0);
        let lobby = self.add_room(RoomKind::Lobby, lx0, iy0, lx1, ly - 1);
        let cr = ly - 4;
        self.set_floor(lx0, cr + 1, lx1, ly - 1, Floor::Wood, 0);
        let entrance = (lx0 + lx1) / 2;
        for x in entrance - 1..=entrance + 1 {
            self.door(x, by0, Floor::Marble, 0);
            self.map.at_mut(x, by0).indoor = true;
        }
        // The front's windows: glass in runs of three, brick between.
        for x in bx0 + 2..bx1 - 1 {
            if (x - entrance).abs() <= 2 || self.map.at(x, by0).wall != Wall::Brick {
                continue;
            }
            if (x - bx0) % 4 != 0 {
                self.set_wall(x, by0, Wall::Glass);
            }
        }
        // The counter, a gap at one end.
        let gap_left = self.rng.chance(0.5);
        let (c0, c1) = if gap_left {
            (lx0 + 2, lx1)
        } else {
            (lx0, lx1 - 2)
        };
        self.place_span(PropKind::Counter, c0, cr, c1 - c0 + 1, 1, SOUTH, lobby);
        let mut x = c0 + 1;
        while x < c1 {
            self.place(PropKind::Chair, x, cr + 1, 1, 1, SOUTH, lobby);
            x += 3;
        }
        // The lobby's doors to the corridor, from the tellers' side.
        let n_doors = if lx1 - lx0 > 20 { 2 } else { 1 };
        for k in 0..n_doors {
            let span = (lx1 - lx0 - 3) / n_doors;
            let x = lx0 + 2 + k * span + self.rng.range(0, span.max(1) - 1);
            self.door(x, ly, Floor::Wood, 0);
            self.door(x + 1, ly, Floor::Wood, 0);
        }

        // The corridor, a side door at one end.
        let tint = self.rng.u32() as u8;
        self.set_floor(ix0, cy0, ix1, cy1, Floor::Lino, tint);
        self.add_room(RoomKind::Corridor, ix0, cy0, ix1, cy1);
        let side_x = if self.rng.chance(0.5) { bx0 } else { bx1 };
        self.door(side_x, cy0 + 1, Floor::Lino, tint);
        self.map.at_mut(side_x, cy0 + 1).indoor = true;
        let mut x = ix0 + 3;
        while x < ix1 {
            self.light(x as f32 + 0.5, cy0 as f32 + 1.5, 6.0, FLUORESCENT, 0.9);
            x += 7;
        }

        // The back rooms.
        let mut leaves = Vec::new();
        let mut splits = Vec::new();
        self.bsp(ix0, ry + 1, ix1, iy1, &mut leaves, &mut splits);
        self.assign_back_rooms(leaves, splits, ry);
        // Windows in the back and side walls of the back rooms.
        for x in bx0 + 2..bx1 - 1 {
            if self.map.at(x, by1 - 1).wall == Wall::None
                && self.map.at(x, by1 - 1).floor != Floor::Steel
                && (x - bx0) % 5 < 2
            {
                self.set_wall(x, by1, Wall::Glass);
            }
        }
        // Lobby lights: a warm grid.
        let mut y = iy0 as f32 + 2.5;
        while y < cr as f32 {
            let mut x = lx0 as f32 + 2.5;
            while x < lx1 as f32 {
                self.light(x, y, 7.5, WARM, 0.95);
                x += 6.0;
            }
            y += 5.0;
        }
        let mut x = lx0 as f32 + 3.0;
        while x < lx1 as f32 {
            self.light(x, (ly - 2) as f32 + 0.5, 5.0, WARM, 0.6);
            x += 7.0;
        }
        entrance
    }

    /// Split the back rooms' band into rooms.
    fn bsp(
        &mut self,
        x0: i32,
        y0: i32,
        x1: i32,
        y1: i32,
        leaves: &mut Vec<(i32, i32, i32, i32)>,
        splits: &mut Vec<(bool, i32, i32, i32)>,
    ) {
        let (w, h) = (x1 - x0 + 1, y1 - y0 + 1);
        const MIN: i32 = 4;
        let wide = w > 13 || (w > 9 && self.rng.chance(0.4));
        let tall = h > 10 || (h > 8 && self.rng.chance(0.3));
        let can_v = w > 2 * MIN;
        let can_h = h > 2 * MIN;
        let vertical = if wide && can_v && (!tall || !can_h || w >= h) {
            true
        } else if tall && can_h {
            false
        } else {
            leaves.push((x0, y0, x1, y1));
            return;
        };
        if vertical {
            let cut = x0 + self.rng.range(MIN, w - MIN);
            for y in y0..=y1 {
                self.set_wall(cut, y, Wall::Drywall);
            }
            splits.push((true, cut, y0, y1));
            self.bsp(x0, y0, cut - 1, y1, leaves, splits);
            self.bsp(cut + 1, y0, x1, y1, leaves, splits);
        } else {
            let cut = y0 + self.rng.range(MIN, h - MIN);
            for x in x0..=x1 {
                self.set_wall(x, cut, Wall::Drywall);
            }
            splits.push((false, cut, x0, x1));
            self.bsp(x0, y0, x1, cut - 1, leaves, splits);
            self.bsp(x0, cut + 1, x1, y1, leaves, splits);
        }
    }

    fn assign_back_rooms(
        &mut self,
        leaves: Vec<(i32, i32, i32, i32)>,
        splits: Vec<(bool, i32, i32, i32)>,
        ry: i32,
    ) {
        // The vault: the biggest leaf that will take it, deepest first.
        let mut order: Vec<usize> = (0..leaves.len()).collect();
        order.sort_by_key(|&i| {
            let (x0, y0, x1, y1) = leaves[i];
            let a = (x1 - x0 + 1) * (y1 - y0 + 1);
            (-(a.min(80)), -(y0))
        });
        let vault = order
            .iter()
            .copied()
            .find(|&i| {
                let (x0, y0, x1, y1) = leaves[i];
                x1 - x0 + 1 >= 6 && y1 - y0 + 1 >= 5
            })
            .unwrap_or(order[0]);
        let mut rest: Vec<usize> = order.into_iter().filter(|&i| i != vault).collect();
        // The rest by size: the biggest is open-plan, then the manager, the
        // break room; the smallest are washrooms, servers, security, archive.
        rest.sort_by_key(|&i| {
            let (x0, y0, x1, y1) = leaves[i];
            -((x1 - x0 + 1) * (y1 - y0 + 1))
        });
        let mut kinds = vec![RoomKind::Office; rest.len()];
        let mut big = vec![RoomKind::OpenOffice, RoomKind::Manager, RoomKind::Break];
        let mut small = vec![
            RoomKind::Washroom,
            RoomKind::Server,
            RoomKind::Security,
            RoomKind::Archive,
        ];
        self.rng.shuffle(&mut small[1..]);
        let (mut lo, mut hi) = (0usize, rest.len());
        while lo < hi && !big.is_empty() {
            kinds[lo] = big.remove(0);
            lo += 1;
        }
        while hi > lo && !small.is_empty() {
            hi -= 1;
            kinds[hi] = small.remove(0);
        }

        // Doors between the BSP's halves, where the wall has room for one.
        for &(vertical, cut, a0, a1) in &splits {
            let mut spots = Vec::new();
            for k in a0 + 1..a1 {
                let (x, y) = if vertical { (cut, k) } else { (k, cut) };
                let (ax, ay, bx, by) = if vertical {
                    (x - 1, y, x + 1, y)
                } else {
                    (x, y - 1, x, y + 1)
                };
                if !self.map.is_wall(ax, ay) && !self.map.is_wall(bx, by) {
                    spots.push((x, y));
                }
            }
            if !spots.is_empty() && self.rng.chance(0.55) {
                let (x, y) = self.rng.pick(&spots);
                self.door(x, y, Floor::Concrete, 0);
            }
        }

        let vault_leaf = leaves[vault];
        let mut ids = Vec::new();
        for (k, &i) in rest.iter().enumerate() {
            let (x0, y0, x1, y1) = leaves[i];
            let kind = kinds[k];
            let tint = self.rng.u32() as u8;
            let floor = match kind {
                RoomKind::OpenOffice | RoomKind::Office | RoomKind::Security => Floor::Carpet,
                RoomKind::Manager => Floor::Wood,
                RoomKind::Break => Floor::Lino,
                RoomKind::Washroom => Floor::WashroomTile,
                RoomKind::Server => Floor::Raised,
                _ => Floor::Concrete,
            };
            self.set_floor(x0, y0, x1, y1, floor, tint);
            let id = self.add_room(kind, x0, y0, x1, y1);
            ids.push(id);
            // Onto the corridor, where it fronts it.
            if y0 == ry + 1 {
                let x = self.rng.range(x0 + 1, x1);
                self.door(x, ry, floor, tint);
                // An office's front is glass, half the time.
                if matches!(
                    kind,
                    RoomKind::Office | RoomKind::Manager | RoomKind::OpenOffice
                ) && self.rng.chance(0.5)
                {
                    for gx in x0..=x1 {
                        if (gx - x).abs() > 1 && self.map.at(gx, ry).wall == Wall::Drywall {
                            self.set_wall(gx, ry, Wall::Glass);
                        }
                    }
                }
            }
            // Doors take the floor of the room they open into.
            for y in y0 - 1..=y1 + 1 {
                for x in x0 - 1..=x1 + 1 {
                    if self.map.inside(x, y)
                        && self.map.at(x, y).deco & deco::DOOR != 0
                        && self.map.at(x, y).floor == Floor::Concrete
                    {
                        let t = self.map.at_mut(x, y);
                        t.floor = floor;
                        t.tint = tint;
                    }
                }
            }
            self.room_lights(kind, x0, y0, x1, y1);
        }

        // The vault: steel all round, every door in its walls shut, one
        // door of its own.
        let (x0, y0, x1, y1) = vault_leaf;
        self.set_floor(x0, y0, x1, y1, Floor::Steel, 0);
        self.add_room(RoomKind::Vault, x0, y0, x1, y1);
        let (bx0, _, bx1, by1) = self.map.building;
        for y in y0 - 1..=y1 + 1 {
            for x in x0 - 1..=x1 + 1 {
                let edge = x == x0 - 1 || x == x1 + 1 || y == y0 - 1 || y == y1 + 1;
                if !edge {
                    continue;
                }
                let exterior = x == bx0 || x == bx1 || y == by1;
                if exterior {
                    self.set_wall(x, y, Wall::Brick);
                } else {
                    self.set_wall(x, y, Wall::Vault);
                }
            }
        }
        // Its door: onto the corridor if it fronts it, else the longest
        // stretch of wall shared with a room.
        let mut spots: Vec<(i32, i32, i32, i32)> = Vec::new(); // door tile, outward step
        for x in x0 + 1..x1 - 1 {
            spots.push((x, y0 - 1, 0, -1));
            spots.push((x, y1 + 1, 0, 1));
        }
        for y in y0 + 1..y1 - 1 {
            spots.push((x0 - 1, y, -1, 0));
            spots.push((x1 + 1, y, 1, 0));
        }
        let open = |m: &Map, x: i32, y: i32| m.inside(x, y) && m.at(x, y).wall == Wall::None;
        let mut good: Vec<(i32, i32, i32, i32)> = spots
            .iter()
            .copied()
            .filter(|&(x, y, dx, dy)| {
                let (px, py) = (-dy, dx); // along the wall
                open(&self.map, x + dx, y + dy)
                    && open(&self.map, x + px + dx, y + py + dy)
                    && self.map.at(x + dx, y + dy).indoor
                    && self.map.at(x + px, y + py).wall == Wall::Vault
            })
            .collect();
        good.sort_by_key(|&(x, y, dx, dy)| {
            let onto_corridor = self.map.at(x + dx, y + dy).room != NO_ROOM
                && self.map.rooms[self.map.at(x + dx, y + dy).room as usize].kind
                    == RoomKind::Corridor;
            (
                !onto_corridor,
                (x - (x0 + x1) / 2).abs() + (y - (y0 + y1) / 2).abs(),
            )
        });
        if let Some(&(x, y, dx, dy)) = good.first() {
            let (px, py) = (-dy, dx);
            for (tx, ty) in [(x, y), (x + px, y + py)] {
                self.door(tx, ty, Floor::Steel, 0);
                self.map.at_mut(tx, ty).deco |= deco::VAULT_DOOR;
            }
            let vault_id = (self.map.rooms.len() - 1) as u16;
            // The door itself, swung open against the outside wall.
            let face = match (dx, dy) {
                (0, 1) => NORTH,
                (1, 0) => EAST,
                (0, -1) => SOUTH,
                _ => WEST,
            };
            let (ox, oy) = (x.min(x + px), y.min(y + py));
            let (w, h) = if px != 0 { (2, 1) } else { (1, 2) };
            self.map.props.push(Prop {
                kind: PropKind::VaultDoor,
                block: Aabb::new(
                    vec2(ox as f32, oy as f32) * TILE,
                    vec2((ox + w) as f32, (oy + h) as f32) * TILE,
                ),
                body: Aabb::new(
                    vec2(ox as f32, oy as f32) * TILE,
                    vec2((ox + w) as f32, (oy + h) as f32) * TILE,
                ),
                face,
                seed: 0,
                solid: false,
                opaque: false,
                room: vault_id,
            });
        }
        self.room_lights(RoomKind::Vault, x0, y0, x1, y1);
    }

    fn room_lights(&mut self, kind: RoomKind, x0: i32, y0: i32, x1: i32, y1: i32) {
        let (color, intensity, reach, on) = match kind {
            RoomKind::Vault => (VAULT_RED, 0.75, 6.5, 1.0),
            RoomKind::Server => (SERVER_BLUE, 0.7, 5.5, 1.0),
            RoomKind::Washroom => (FLUORESCENT, 0.8, 5.0, 0.7),
            RoomKind::Manager => (WARM, 0.85, 6.0, 0.6),
            RoomKind::Break => (FLUORESCENT, 0.85, 6.0, 0.6),
            RoomKind::Security => (FLUORESCENT, 0.7, 5.5, 1.0),
            _ => (FLUORESCENT, 0.8, 6.0, 0.45),
        };
        if !self.rng.chance(on) {
            return;
        }
        let (w, h) = (x1 - x0 + 1, y1 - y0 + 1);
        let nx = ((w as f32) / 5.5).ceil().max(1.0) as i32;
        let ny = ((h as f32) / 5.5).ceil().max(1.0) as i32;
        for j in 0..ny {
            for i in 0..nx {
                let x = x0 as f32 + w as f32 * (i as f32 + 0.5) / nx as f32;
                let y = y0 as f32 + h as f32 * (j as f32 + 0.5) / ny as f32;
                self.light(x, y, reach, color, intensity);
            }
        }
    }

    /// Flood from the street through every open tile; punch a door into
    /// any room left shut off, and do it again until none is.
    fn fix_walls(&mut self) {
        for _ in 0..64 {
            let reached = self.flood(false);
            let mut shut: Vec<usize> = Vec::new();
            for (i, r) in self.map.rooms.iter().enumerate() {
                let any = (r.y0..=r.y1).any(|y| (r.x0..=r.x1).any(|x| reached[self.map.idx(x, y)]));
                if !any {
                    shut.push(i);
                }
            }
            if shut.is_empty() {
                return;
            }
            // The first shut room that borders a reached one: a room shut
            // in behind another is opened once that one is.
            let mut found = None;
            for &k in &shut {
                let room = self.map.rooms[k];
                let spots = self.door_spots(room, &reached);
                if !spots.is_empty() {
                    found = Some((room, spots));
                    break;
                }
            }
            let Some((room, spots)) = found else {
                return;
            };
            let (x, y) = self.rng.pick(&spots);
            let inner = self.map.at(room.x0, room.y0);
            self.door(x, y, inner.floor, inner.tint);
        }
    }

    /// Wall tiles a door could be punched through from `room` into a
    /// reached tile.
    fn door_spots(&self, room: Room, reached: &[bool]) -> Vec<(i32, i32)> {
        let mut spots = Vec::new();
        for y in room.y0 - 1..=room.y1 + 1 {
            for x in room.x0 - 1..=room.x1 + 1 {
                if !self.map.inside(x, y) {
                    continue;
                }
                let wall = self.map.at(x, y).wall;
                if !matches!(wall, Wall::Drywall | Wall::Glass)
                    && !(room.kind == RoomKind::Vault && wall == Wall::Vault)
                {
                    continue;
                }
                for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let (ix, iy) = (x - dx, y - dy);
                    let (ox, oy) = (x + dx, y + dy);
                    if room.contains(ix, iy)
                        && self.map.inside(ox, oy)
                        && reached[self.map.idx(ox, oy)]
                    {
                        spots.push((x, y));
                    }
                }
            }
        }
        spots
    }

    /// The tiles reachable from the street, through doors and, with
    /// `props`, round the furniture.
    fn flood(&self, props: bool) -> Vec<bool> {
        let m = &self.map;
        let mut seen = vec![false; (m.w * m.h) as usize];
        let mut queue = VecDeque::new();
        // The pavement's far end, or where the player starts once there is one.
        let start = if m.spawn == Vec2::ZERO {
            (1, 7)
        } else {
            tile_of(m.spawn)
        };
        seen[m.idx(start.0, start.1)] = true;
        queue.push_back(start);
        while let Some((x, y)) = queue.pop_front() {
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let (nx, ny) = (x + dx, y + dy);
                if !m.inside(nx, ny) {
                    continue;
                }
                let i = m.idx(nx, ny);
                if seen[i] || m.at(nx, ny).wall != Wall::None || (props && self.used[i]) {
                    continue;
                }
                seen[i] = true;
                queue.push_back((nx, ny));
            }
        }
        seen
    }

    /// The tiles in front of every door, kept clear of furniture.
    fn mark_clear(&mut self) {
        for y in 0..self.map.h {
            for x in 0..self.map.w {
                if self.map.at(x, y).deco & deco::DOOR == 0 {
                    continue;
                }
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        if self.map.inside(x + dx, y + dy) {
                            let i = self.map.idx(x + dx, y + dy);
                            self.clear[i] = true;
                        }
                    }
                }
                // Two in, straight through.
                let horizontal_wall = self.map.is_wall(x - 1, y) || self.map.is_wall(x + 1, y);
                let steps: [(i32, i32); 2] = if horizontal_wall {
                    [(0, 2), (0, -2)]
                } else {
                    [(2, 0), (-2, 0)]
                };
                for (dx, dy) in steps {
                    if self.map.inside(x + dx, y + dy) {
                        let i = self.map.idx(x + dx, y + dy);
                        self.clear[i] = true;
                    }
                }
            }
        }
    }

    /// Take furniture out of any room where it walls a door in.
    fn fix_props(&mut self) {
        for _ in 0..200 {
            let reached = self.flood(true);
            // A door nobody reaches, beside a room partly reached: the
            // furniture in that room is what walls it in.
            let partly = |r: u16| {
                let room = self.map.rooms[r as usize];
                (room.y0..=room.y1)
                    .any(|y| (room.x0..=room.x1).any(|x| reached[self.map.idx(x, y)]))
            };
            let mut stuck_room = None;
            'find: for y in 0..self.map.h {
                for x in 0..self.map.w {
                    let t = self.map.at(x, y);
                    if t.deco & deco::DOOR != 0 && !reached[self.map.idx(x, y)] {
                        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                            if self.map.inside(x + dx, y + dy) {
                                let r = self.map.at(x + dx, y + dy).room;
                                if r != NO_ROOM && partly(r) {
                                    stuck_room = Some(r);
                                    break 'find;
                                }
                            }
                        }
                    }
                }
            }
            let Some(room) = stuck_room else { return };
            let Some(k) = self
                .map
                .props
                .iter()
                .rposition(|p| p.room == room && p.solid)
            else {
                return;
            };
            let p = self.map.props.remove(k);
            let (x0, y0) = tile_of(p.block.min);
            let (x1, y1) = tile_of(p.block.max - Vec2::splat(0.01));
            for y in y0..=y1 {
                for x in x0..=x1 {
                    if self.map.inside(x, y) {
                        let i = self.map.idx(x, y);
                        self.used[i] = false;
                    }
                }
            }
            // Something else on those tiles keeps them used.
            for q in self.map.props.clone() {
                if q.solid {
                    self.mark_used(q.block);
                }
            }
        }
    }

    fn mark_used(&mut self, block: Aabb) {
        let (x0, y0) = tile_of(block.min);
        let (x1, y1) = tile_of(block.max - Vec2::splat(0.01));
        for y in y0..=y1 {
            for x in x0..=x1 {
                if self.map.inside(x, y) {
                    let i = self.map.idx(x, y);
                    self.used[i] = true;
                }
            }
        }
    }

    // --- placing things ----------------------------------------------------

    /// Whether a block of tiles is free floor of one room.
    fn free(&self, x: i32, y: i32, w: i32, h: i32, room: u16, keep_clear: bool) -> bool {
        for ty in y..y + h {
            for tx in x..x + w {
                if !self.map.inside(tx, ty) {
                    return false;
                }
                let i = self.map.idx(tx, ty);
                let t = self.map.tiles[i];
                if t.wall != Wall::None
                    || self.used[i]
                    || (keep_clear && self.clear[i])
                    || (room != NO_ROOM && t.room != room)
                    || t.deco & deco::DOOR != 0
                {
                    return false;
                }
            }
        }
        true
    }

    /// A prop on the tiles `(x, y)`..`(x + w, y + h)` facing `face`, if
    /// they are free. Decoration goes anywhere.
    #[allow(clippy::too_many_arguments)]
    fn place(
        &mut self,
        kind: PropKind,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        face: Face,
        room: u16,
    ) -> bool {
        let s = spec(kind);
        if s.solid && !self.free(x, y, w, h, room, true) {
            return false;
        }
        self.put(kind, x, y, w, h, face, room);
        true
    }

    /// As [`place`](Self::place), but along a span the caller has worked
    /// out, door clearances or not (the counter).
    #[allow(clippy::too_many_arguments)]
    fn place_span(
        &mut self,
        kind: PropKind,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        face: Face,
        room: u16,
    ) {
        self.put(kind, x, y, w, h, face, room);
    }

    #[allow(clippy::too_many_arguments)]
    fn put(&mut self, kind: PropKind, x: i32, y: i32, w: i32, h: i32, face: Face, room: u16) {
        let s = spec(kind);
        let block = Aabb::new(
            vec2(x as f32, y as f32) * TILE,
            vec2((x + w) as f32, (y + h) as f32) * TILE,
        );
        let body = body_in(block, face, &s);
        if s.solid {
            self.mark_used(block);
        }
        let seed = self.rng.u32();
        self.map.props.push(Prop {
            kind,
            block,
            body,
            face,
            seed,
            solid: s.solid,
            opaque: s.opaque,
            room,
        });
    }

    /// Up to `n` of a kind against the room's walls, `along` tiles long,
    /// facing into the room.
    fn along_walls(&mut self, kind: PropKind, room: u16, along: i32, n: usize) -> usize {
        let r = self.map.rooms[room as usize];
        let mut spots = Vec::new();
        for x in r.x0..=r.x1 - along + 1 {
            if (x..x + along).all(|tx| self.map.is_wall(tx, r.y1 + 1)) {
                spots.push((x, r.y1, along, 1, SOUTH));
            }
            if (x..x + along).all(|tx| self.map.is_wall(tx, r.y0 - 1)) {
                spots.push((x, r.y0, along, 1, NORTH));
            }
        }
        for y in r.y0..=r.y1 - along + 1 {
            if (y..y + along).all(|ty| self.map.is_wall(r.x0 - 1, ty)) {
                spots.push((r.x0, y, 1, along, EAST));
            }
            if (y..y + along).all(|ty| self.map.is_wall(r.x1 + 1, ty)) {
                spots.push((r.x1, y, 1, along, WEST));
            }
        }
        self.rng.shuffle(&mut spots);
        let mut placed = 0;
        for (x, y, w, h, face) in spots {
            if placed >= n {
                break;
            }
            if self.place(kind, x, y, w, h, face, room) {
                placed += 1;
            }
        }
        placed
    }

    /// One in each corner that is free, up to `n`.
    fn in_corners(&mut self, kind: PropKind, room: u16, n: usize) {
        let r = self.map.rooms[room as usize];
        let mut corners = [
            (r.x0, r.y0, NORTH),
            (r.x1, r.y0, NORTH),
            (r.x0, r.y1, SOUTH),
            (r.x1, r.y1, SOUTH),
        ];
        self.rng.shuffle(&mut corners);
        let mut placed = 0;
        for (x, y, face) in corners {
            if placed >= n {
                break;
            }
            if self.place(kind, x, y, 1, 1, face, room) {
                placed += 1;
            }
        }
    }

    fn furnish(&mut self) {
        for id in 0..self.map.rooms.len() as u16 {
            let r = self.map.rooms[id as usize];
            match r.kind {
                RoomKind::Lobby => self.furnish_lobby(id),
                RoomKind::Corridor => {
                    self.along_walls(PropKind::Plant, id, 1, (r.w() / 12) as usize);
                    self.along_walls(PropKind::Cooler, id, 1, 1);
                    self.along_walls(PropKind::Bin, id, 1, 1);
                }
                RoomKind::OpenOffice => self.furnish_open_office(id),
                RoomKind::Office => self.furnish_office(id),
                RoomKind::Manager => self.furnish_manager(id),
                RoomKind::Break => self.furnish_break(id),
                RoomKind::Washroom => self.furnish_washroom(id),
                RoomKind::Server => self.furnish_server(id),
                RoomKind::Security => {
                    self.along_walls(PropKind::MonitorDesk, id, 3, 1);
                    self.along_walls(PropKind::Lockers, id, 2, 2);
                    self.along_walls(PropKind::Cabinet, id, 1, 1);
                }
                RoomKind::Archive => {
                    self.along_walls(PropKind::Bookshelf, id, 2, 8);
                    self.along_walls(PropKind::Cabinet, id, 1, 6);
                }
                RoomKind::Vault => self.furnish_vault(id),
            }
        }
    }

    fn furnish_lobby(&mut self, id: u16) {
        let r = self.map.rooms[id as usize];
        let cr = r.y1 - 3; // the counter's row
        // Pillars in two rows across the customers' side.
        let rows: Vec<i32> = if cr - r.y0 >= 8 {
            vec![r.y0 + 2, cr - 3]
        } else {
            vec![r.y0 + 2]
        };
        for y in rows {
            let mut x = r.x0 + 3;
            while x <= r.x1 - 3 {
                self.place(PropKind::Pillar, x, y, 1, 1, NORTH, id);
                x += 6;
            }
        }
        // A runner from the door to the counter.
        let (_, by0, _, _) = self.map.building;
        let mid = {
            let mut m = r.x0;
            for x in r.x0..=r.x1 {
                if self.map.at(x, by0).deco & deco::DOOR != 0 {
                    m = x;
                    break;
                }
            }
            m + 1
        };
        for y in r.y0..cr {
            for x in mid - 1..=mid + 1 {
                self.map.at_mut(x, y).deco |= deco::RUG;
            }
        }
        // The queue's ropes, in front of the counter.
        let qx = if mid - r.x0 > r.x1 - mid {
            mid - 6
        } else {
            mid + 3
        };
        for k in 0..4 {
            for y in [cr - 1, cr - 2] {
                self.place(PropKind::Stanchion, qx + k, y, 1, 1, EAST, id);
            }
        }
        // Benches and plants along the sides, ATMs on a wall.
        self.along_walls(PropKind::Atm, id, 1, 2);
        self.along_walls(PropKind::Bench, id, 2, 2);
        self.in_corners(PropKind::Plant, id, 2);
        // Behind the counter: cabinets and a safe against the back wall.
        let back = r.y1;
        let mut x = r.x0 + 1;
        while x < r.x1 {
            let kind = if self.rng.chance(0.15) {
                PropKind::Safe
            } else if self.rng.chance(0.5) {
                PropKind::Cabinet
            } else {
                PropKind::Copier
            };
            self.place(kind, x, back, 1, 1, SOUTH, id);
            x += self.rng.range(2, 5);
        }
    }

    fn furnish_open_office(&mut self, id: u16) {
        let r = self.map.rooms[id as usize];
        let mut y = r.y0 + 1;
        while y < r.y1 - 1 {
            let mut x = r.x0 + 1;
            while x < r.x1 - 1 {
                if self.place(PropKind::Desk, x, y, 2, 1, NORTH, id) {
                    self.place(PropKind::Chair, x, y - 1, 1, 1, NORTH, id);
                }
                x += 3;
            }
            y += 3;
        }
        self.in_corners(PropKind::Plant, id, 2);
        self.along_walls(PropKind::Copier, id, 1, 1);
        self.along_walls(PropKind::Cabinet, id, 1, 2);
    }

    fn furnish_office(&mut self, id: u16) {
        let r = self.map.rooms[id as usize];
        let cx = (r.x0 + r.x1) / 2;
        let cy = (r.y0 + r.y1) / 2;
        if self.place(PropKind::Desk, cx - 1, cy + 1, 2, 1, SOUTH, id) {
            self.place(PropKind::Chair, cx - 1, cy + 2, 1, 1, SOUTH, id);
        }
        self.along_walls(PropKind::Bookshelf, id, 2, 1);
        self.along_walls(PropKind::Cabinet, id, 1, 2);
        self.in_corners(PropKind::Plant, id, 1);
    }

    fn furnish_manager(&mut self, id: u16) {
        let r = self.map.rooms[id as usize];
        let cx = (r.x0 + r.x1) / 2;
        let cy = (r.y0 + r.y1) / 2;
        self.place(PropKind::Rug, cx - 2, cy - 1, 4, 3, NORTH, id);
        if self.place(PropKind::BigDesk, cx - 1, cy + 1, 3, 1, SOUTH, id) {
            self.place(PropKind::Chair, cx, cy + 2, 1, 1, SOUTH, id);
        }
        self.along_walls(PropKind::Bookshelf, id, 2, 2);
        self.along_walls(PropKind::Sofa, id, 2, 1);
        self.along_walls(PropKind::Safe, id, 1, 1);
        self.in_corners(PropKind::Plant, id, 2);
    }

    fn furnish_break(&mut self, id: u16) {
        let r = self.map.rooms[id as usize];
        let cx = (r.x0 + r.x1) / 2;
        let cy = (r.y0 + r.y1) / 2;
        if self.place(PropKind::Table, cx - 1, cy - 1, 2, 2, NORTH, id) {
            for (x, y, f) in [
                (cx - 2, cy - 1, EAST),
                (cx - 2, cy, EAST),
                (cx + 1, cy - 1, WEST),
                (cx + 1, cy, WEST),
            ] {
                self.place(PropKind::Chair, x, y, 1, 1, f, id);
            }
        }
        self.along_walls(PropKind::SinkCounter, id, 2, 1);
        self.along_walls(PropKind::Fridge, id, 1, 1);
        self.along_walls(PropKind::Vending, id, 1, 1);
        self.along_walls(PropKind::Cooler, id, 1, 1);
    }

    fn furnish_washroom(&mut self, id: u16) {
        self.along_walls(PropKind::Toilet, id, 1, 3);
        self.along_walls(PropKind::SinkCounter, id, 2, 1);
        self.along_walls(PropKind::Bin, id, 1, 1);
    }

    fn furnish_server(&mut self, id: u16) {
        let r = self.map.rooms[id as usize];
        let mut x = r.x0 + 1;
        while x < r.x1 {
            let mut y = r.y0 + 1;
            while y < r.y1 - 1 {
                self.place(PropKind::Rack, x, y, 1, 2, EAST, id);
                y += 3;
            }
            x += 2;
        }
    }

    fn furnish_vault(&mut self, id: u16) {
        let r = self.map.rooms[id as usize];
        // Deposit boxes round the walls, in runs of two.
        let n = (r.w() + r.h()) as usize;
        self.along_walls(PropKind::Lockers, id, 2, n);
        // Cash and gold in the middle, a gangway between.
        let mut y = r.y0 + 2;
        while y <= r.y1 - 2 {
            let mut x = r.x0 + 2;
            while x <= r.x1 - 2 {
                let kind = match self.rng.range(0, 4) {
                    0 => PropKind::Gold,
                    1 => PropKind::Safe,
                    _ => PropKind::Pallet,
                };
                self.place(kind, x, y, 1, 1, NORTH, id);
                x += 2;
            }
            y += 2;
        }
    }

    /// The street furniture: lamps, trees, benches, bins, the van at the
    /// kerb and cars in the car park. Also where the player starts.
    fn street(&mut self, road: i32, entrance: i32) {
        let (bx0, by0, bx1, by1) = self.map.building;
        let (w, h) = (self.map.w, self.map.h);
        // Street lamps along the kerb.
        let mut x = 3;
        while x < w - 2 {
            if self.place(PropKind::StreetLamp, x, road, 1, 1, NORTH, NO_ROOM) {
                self.light(x as f32 + 0.5, road as f32 + 0.6, 8.5, SODIUM, 1.0);
            }
            x += 10;
        }
        // Lamps in the car park.
        let mut x = bx0 + 2;
        while x < bx1 {
            let y = by1 + 3;
            if self.place(PropKind::StreetLamp, x, y, 1, 1, NORTH, NO_ROOM) {
                self.light(x as f32 + 0.5, y as f32 + 0.5, 8.0, SODIUM, 0.85);
            }
            x += 13;
        }
        // A lamp over the side door.
        for y in by0..=by1 {
            for xx in [bx0, bx1] {
                if self.map.at(xx, y).deco & deco::DOOR != 0 {
                    let out = if xx == bx0 { -0.4 } else { 1.4 };
                    self.light(xx as f32 + out, y as f32 + 0.5, 5.0, WARM, 0.8);
                }
            }
        }
        self.light(entrance as f32 + 0.5, by0 as f32 - 0.6, 6.0, WARM, 0.9);
        // Trees and bushes on the grass at the sides.
        for side in [0, 1] {
            let (gx0, gx1) = if side == 0 {
                (1, bx0 - 4)
            } else {
                (bx1 + 4, w - 2)
            };
            let mut y = by0 + 1;
            while y < h - 2 {
                let x = self.rng.range(gx0, gx1 + 1);
                if self.map.at(x, y).floor == Floor::Grass {
                    let kind = if self.rng.chance(0.65) {
                        PropKind::Tree
                    } else {
                        PropKind::Bush
                    };
                    self.place(kind, x, y, 1, 1, NORTH, NO_ROOM);
                }
                y += self.rng.range(3, 6);
            }
        }
        // On the pavement: benches, bins, a hydrant.
        let pave = road + 1;
        for _ in 0..6 {
            let x = self.rng.range(2, w - 3);
            if (x - entrance).abs() < 4 {
                continue;
            }
            let kind = self
                .rng
                .pick(&[PropKind::Bench, PropKind::Bin, PropKind::Hydrant]);
            let len = if kind == PropKind::Bench { 2 } else { 1 };
            self.place(kind, x, pave + 1, len, 1, SOUTH, NO_ROOM);
        }
        // The van, at the kerb a little along from the door.
        let vx = entrance + if self.rng.chance(0.5) { 3 } else { -9 };
        self.place(PropKind::Van, vx, road - 2, 6, 2, EAST, NO_ROOM);
        // Cars in the bays.
        let p0 = by1 + 3;
        for (r0, r1) in [(p0 + 1, p0 + 5), (h - 6, h - 2)] {
            let mut x = bx0 - 2;
            while x + 3 <= bx1 + 2 {
                if self.rng.chance(0.45) {
                    let face = if r0 == p0 + 1 { NORTH } else { SOUTH };
                    if r1 - r0 >= 4 {
                        self.place(PropKind::Car, x, r0, 3, 5, face, NO_ROOM);
                    }
                }
                x += 3;
            }
        }
        // The player: on the pavement in front of the door.
        self.map.spawn = tile_centre(entrance, road + 1);
    }
}

/// Where a kind stands inside its block.
fn body_in(block: Aabb, face: Face, s: &Spec) -> Aabb {
    let along_x = face == NORTH || face == SOUTH;
    let (mut x0, mut x1, mut y0, mut y1) = (block.min.x, block.max.x, block.min.y, block.max.y);
    if along_x {
        x0 += s.inset;
        x1 -= s.inset;
        let d = s.depth.min(y1 - y0);
        if s.backed {
            if face == NORTH {
                y1 = y0 + d;
            } else {
                y0 = y1 - d;
            }
        } else {
            let m = (y0 + y1) * 0.5;
            y0 = m - d * 0.5;
            y1 = m + d * 0.5;
        }
    } else {
        y0 += s.inset;
        y1 -= s.inset;
        let d = s.depth.min(x1 - x0);
        if s.backed {
            if face == EAST {
                x1 = x0 + d;
            } else {
                x0 = x1 - d;
            }
        } else {
            let m = (x0 + x1) * 0.5;
            x0 = m - d * 0.5;
            x1 = m + d * 0.5;
        }
    }
    Aabb::new(vec2(x0, y0), vec2(x1, y1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_room_and_every_door_is_reachable_on_many_seeds() {
        for seed in 0..60 {
            let map = generate(seed);
            let g = Gen {
                used: {
                    let mut used = vec![false; (map.w * map.h) as usize];
                    for p in map.props.iter().filter(|p| p.solid) {
                        let (x0, y0) = tile_of(p.block.min);
                        let (x1, y1) = tile_of(p.block.max - Vec2::splat(0.01));
                        for y in y0..=y1 {
                            for x in x0..=x1 {
                                if map.inside(x, y) {
                                    used[map.idx(x, y)] = true;
                                }
                            }
                        }
                    }
                    used
                },
                clear: Vec::new(),
                rng: Rng::new(0),
                map,
            };
            let reached = g.flood(true);
            for (i, r) in g.map.rooms.iter().enumerate() {
                let any = (r.y0..=r.y1).any(|y| (r.x0..=r.x1).any(|x| reached[g.map.idx(x, y)]));
                assert!(any, "seed {seed}: room {i} ({:?}) shut off", r.kind);
            }
            assert!(
                g.map.rooms.iter().any(|r| r.kind == RoomKind::Vault),
                "seed {seed}: no vault"
            );
        }
    }

    #[test]
    fn the_spawn_is_clear_and_the_same_seed_is_the_same_bank() {
        let a = generate(7);
        let b = generate(7);
        assert_eq!(a.w, b.w);
        assert_eq!(a.props.len(), b.props.len());
        assert_eq!(a.spawn, b.spawn);
        for s in a.solids() {
            assert!(!s.expand(BODY_MARGIN - 1.0).contains(a.spawn), "{s:?}");
        }
    }

    #[test]
    fn an_opaque_box_never_holds_a_place_a_body_can_stand() {
        // The ray tracer starts every ray at the body: a body inside an
        // opaque box would see nothing.
        for seed in 0..20 {
            let map = generate(seed);
            for p in map.props.iter().filter(|p| p.opaque) {
                assert!(p.solid, "{:?}", p.kind);
            }
        }
    }
}

#[cfg(test)]
pub fn ascii(map: &Map) -> String {
    let mut used = vec![false; (map.w * map.h) as usize];
    for p in map.props.iter().filter(|p| p.solid) {
        let (x0, y0) = tile_of(p.block.min);
        let (x1, y1) = tile_of(p.block.max - Vec2::splat(0.01));
        for y in y0..=y1 {
            for x in x0..=x1 {
                if map.inside(x, y) {
                    used[map.idx(x, y)] = true;
                }
            }
        }
    }
    let mut s = String::new();
    for y in (0..map.h).rev() {
        for x in 0..map.w {
            let t = map.at(x, y);
            let c = match t.wall {
                Wall::Brick => '#',
                Wall::Drywall => '=',
                Wall::Vault => 'V',
                Wall::Glass => ':',
                Wall::None if t.deco & deco::DOOR != 0 => 'D',
                Wall::None if used[map.idx(x, y)] => 'o',
                Wall::None if t.room != NO_ROOM => (b'a' + (t.room % 26) as u8) as char,
                _ => '.',
            };
            s.push(c);
        }
        s.push('\n');
    }
    s
}

#[cfg(test)]
#[test]
#[ignore]
fn print_map() {
    let seed = std::env::var("SEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(6);
    let m = generate(seed);
    println!("{}", ascii(&m));
    for (i, r) in m.rooms.iter().enumerate() {
        println!(
            "{} {:?} {:?}",
            (b'a' + (i % 26) as u8) as char,
            r.kind,
            (r.x0, r.y0, r.x1, r.y1)
        );
    }
}
