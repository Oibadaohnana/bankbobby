//! How a body moves under its player's keys — bims' steering, ported.
//!
//! Everything here is `bims::character`'s steered walk (task 144, task 150
//! and the October 2026 changes) with the routes, the errands and the fight
//! taken away: WASD walk the body, the pointer turns it at once, Shift
//! sprints, the dodge key rolls. Same numbers, same units (a tile is 52),
//! same order of operations, so it feels the way a Bim does:
//!
//! - the keys get the body to its pace at `STEER_ACCEL` × `ACCEL`, and any
//!   slowing under the keys — letting go included — is at once;
//! - it faces the pointer at once, and walks whichever way the keys say:
//!   away from the pointer it strides backwards at the same pace;
//! - a sprint is `SPRINT` × the walk, eased in over `SPRINT_EASE`, and
//!   turns the body to the way it runs at `SPRINT_TURN`;
//! - a roll goes `ROLL_DISTANCE` in `ROLL_TIME` the way the keys walk (else
//!   the way they last did, else the heading) and outranks everything;
//! - collision is a push out of every solid grown by `BODY_MARGIN`, with a
//!   lone corner rounded to `CORNER_ROUND` so a body slides round a door
//!   frame instead of stopping on it, at its whole pace when the corner
//!   turned it less than 60°.
//!
//! No Bevy in here beyond `Vec2`: it is plain arithmetic, stepped at a
//! fixed rate, and the tests at the bottom pin the feel.

use bevy::math::{Vec2, vec2};
use std::f32::consts::{PI, TAU};

/// A tile's side in world units (bims' `room::TILE`).
pub const TILE: f32 = 52.0;
/// The one march, in world units a second (bims' `balance::MARCH_SPEED`).
pub const MARCH_SPEED: f32 = 216.0;
/// Units a second squared, speeding up (bims' `ACCEL`).
const ACCEL: f32 = 330.0;
/// How much quicker the keys get a body to its pace than `ACCEL`.
const STEER_ACCEL: f32 = 4.0;
/// How far the body's centre is kept clear of walls and furniture.
pub const BODY_MARGIN: f32 = 23.0;
/// A lone corner's rounding: half again the body, so it clips a corner by
/// some five units and slides round it.
const CORNER_ROUND: f32 = 1.5 * BODY_MARGIN;
/// The cosine a slide round a corner may turn the walk by and keep its pace.
const SLIDE_KEEPS_PACE: f32 = 0.5;
/// How sharply a sprinting body turns to the way it runs.
const SPRINT_TURN: f32 = 16.0;
/// Sprint: a multiple of the walk, and the seconds it eases in over.
pub const SPRINT: f32 = 1.8;
const SPRINT_EASE: f32 = 0.1;
/// The dodge roll: its length, its time and the wait from one to the next.
pub const ROLL_DISTANCE: f32 = 195.0;
pub const ROLL_TIME: f32 = 0.38;
pub const ROLL_COOLDOWN: f32 = 1.1;
/// The fixed step the body is moved at.
pub const STEP: f32 = 1.0 / 60.0;

/// An axis-aligned box in world units: a wall tile or a piece of furniture.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    pub min: Vec2,
    pub max: Vec2,
}

impl Aabb {
    pub fn new(min: Vec2, max: Vec2) -> Self {
        Aabb { min, max }
    }

    pub fn expand(self, by: f32) -> Aabb {
        Aabb {
            min: self.min - Vec2::splat(by),
            max: self.max + Vec2::splat(by),
        }
    }

    pub fn contains(self, p: Vec2) -> bool {
        p.x >= self.min.x && p.x <= self.max.x && p.y >= self.min.y && p.y <= self.max.y
    }

    pub fn width(self) -> f32 {
        self.max.x - self.min.x
    }

    pub fn height(self) -> f32 {
        self.max.y - self.min.y
    }

    pub fn centre(self) -> Vec2 {
        (self.min + self.max) * 0.5
    }

    /// The point of the box nearest `p`.
    pub fn nearest(self, p: Vec2) -> Vec2 {
        vec2(
            clamp(p.x, self.min.x, self.max.x),
            clamp(p.y, self.min.y, self.max.y),
        )
    }

    /// Out of the box grown by `radius` by its nearest side.
    pub fn push_out(self, p: Vec2, radius: f32) -> Option<Vec2> {
        let grown = self.expand(radius);
        if !grown.contains(p) {
            return None;
        }
        let left = p.x - grown.min.x;
        let right = grown.max.x - p.x;
        let down = p.y - grown.min.y;
        let up = grown.max.y - p.y;
        let least = left.min(right).min(down).min(up);
        Some(if least == left {
            vec2(-left, 0.0)
        } else if least == right {
            vec2(right, 0.0)
        } else if least == down {
            vec2(0.0, -down)
        } else {
            vec2(0.0, up)
        })
    }

    /// [`push_out`](Self::push_out) with the grown box's corners rounded to
    /// `round` where `exposed(corner, sx, sy)` says the corner stands alone.
    /// The flag is whether a rounded corner did the pushing.
    pub fn push_out_round(
        self,
        p: Vec2,
        radius: f32,
        round: f32,
        exposed: impl Fn(Vec2, f32, f32) -> bool,
    ) -> Option<(Vec2, bool)> {
        let grown = self.expand(radius);
        if !grown.contains(p) {
            return None;
        }
        let round = round
            .max(radius)
            .min(grown.width() * 0.5)
            .min(grown.height() * 0.5);
        let mid = self.centre();
        let sx = if p.x >= mid.x { 1.0 } else { -1.0 };
        let sy = if p.y >= mid.y { 1.0 } else { -1.0 };
        let corner = vec2(
            if sx > 0.0 { self.max.x } else { self.min.x },
            if sy > 0.0 { self.max.y } else { self.min.y },
        );
        let centre = vec2(
            corner.x + sx * (radius - round),
            corner.y + sy * (radius - round),
        );
        let off = p - centre;
        let beyond = off.x * sx > 0.0 && off.y * sy > 0.0;
        if beyond && exposed(corner, sx, sy) {
            let d = off.length();
            if d >= round {
                return None;
            }
            if d > 1e-3 {
                return Some((off * ((round - d) / d), true));
            }
        }
        self.push_out(p, radius).map(|out| (out, false))
    }
}

/// What a body can bump into: the map's bounds and its solids, with a
/// grid over them so a step looks at the few nearby and not every one.
pub struct Solids {
    pub bounds: Aabb,
    rects: Vec<Aabb>,
    /// Per grid cell, the solids whose box grown by the reach of a push
    /// touches the cell, in the order they were added.
    cells: Vec<Vec<u32>>,
    cols: i32,
    rows: i32,
}

impl Solids {
    pub fn new(bounds: Aabb, rects: Vec<Aabb>) -> Self {
        let cols = (bounds.width() / TILE).ceil().max(1.0) as i32;
        let rows = (bounds.height() / TILE).ceil().max(1.0) as i32;
        let mut cells = vec![Vec::new(); (cols * rows) as usize];
        let reach = BODY_MARGIN + CORNER_ROUND + 2.0;
        for (i, r) in rects.iter().enumerate() {
            let g = r.expand(reach);
            let (x0, y0) = cell_of(bounds, g.min);
            let (x1, y1) = cell_of(bounds, g.max);
            for y in y0.max(0)..=y1.min(rows - 1) {
                for x in x0.max(0)..=x1.min(cols - 1) {
                    cells[(y * cols + x) as usize].push(i as u32);
                }
            }
        }
        Solids {
            bounds,
            rects,
            cells,
            cols,
            rows,
        }
    }

    /// The solids near `p`, in the order they were added.
    fn near(&self, p: Vec2) -> &[u32] {
        let (x, y) = cell_of(self.bounds, p);
        if x < 0 || y < 0 || x >= self.cols || y >= self.rows {
            return &[];
        }
        &self.cells[(y * self.cols + x) as usize]
    }

    /// Whether any solid holds the point.
    pub fn solid_at(&self, p: Vec2) -> bool {
        self.near(p)
            .iter()
            .any(|&i| self.rects[i as usize].contains(p))
    }
}

fn cell_of(bounds: Aabb, p: Vec2) -> (i32, i32) {
    let q = (p - bounds.min) / TILE;
    (q.x.floor() as i32, q.y.floor() as i32)
}

/// What the player's keys and pointer say this step.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Steer {
    /// The way the keys walk it, `None` with none down.
    pub walk: Option<f32>,
    /// Where the pointer is from the body, as an angle.
    pub aim: f32,
    /// Shift.
    pub sprint: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Roll {
    dir: f32,
    left: f32,
}

/// The player's body.
#[derive(Clone, Debug)]
pub struct Body {
    pub pos: Vec2,
    /// Where it was at the start of the last step: what the picture blends from.
    pub was: Vec2,
    pub heading: f32,
    pub speed: f32,
    target_speed: f32,
    /// The way the feet go.
    pub intent: f32,
    /// The walk cycle, a phase; runs backwards for a body stepping back.
    pub stride: f32,
    steer: Steer,
    roll: Option<Roll>,
    roll_cool: f32,
    last_walk: Option<f32>,
}

impl Body {
    pub fn new(pos: Vec2, heading: f32) -> Self {
        Body {
            pos,
            was: pos,
            heading,
            speed: 0.0,
            target_speed: 0.0,
            intent: heading,
            stride: 0.0,
            steer: Steer {
                aim: heading,
                ..Steer::default()
            },
            roll: None,
            roll_cool: 0.0,
            last_walk: None,
        }
    }

    /// The keys and the pointer, said every step.
    pub fn set_steer(&mut self, steer: Steer) {
        if steer.walk.is_some() {
            self.last_walk = steer.walk;
        }
        self.steer = steer;
    }

    pub fn is_sprinting(&self) -> bool {
        self.steer.sprint && self.steer.walk.is_some() && self.roll.is_none()
    }

    pub fn is_rolling(&self) -> bool {
        self.roll.is_some()
    }

    /// How far through its roll it is, nought to one.
    pub fn roll_share(&self) -> Option<f32> {
        self.roll.map(|r| clamp(1.0 - r.left / ROLL_TIME, 0.0, 1.0))
    }

    pub fn roll_dir(&self) -> Option<f32> {
        self.roll.map(|r| r.dir)
    }

    /// The share of the cooldown still to run, nought when a roll is ready.
    pub fn roll_cooling(&self) -> f32 {
        self.roll_cool / ROLL_COOLDOWN
    }

    /// A dodge roll the way the keys walk, else the way they last walked,
    /// else the heading — never at the pointer. `true` when it rolls.
    pub fn start_roll(&mut self) -> bool {
        if self.roll.is_some() || self.roll_cool > 0.0 {
            return false;
        }
        let dir = self.steer.walk.or(self.last_walk).unwrap_or(self.heading);
        self.roll = Some(Roll {
            dir: wrap_angle(dir),
            left: ROLL_TIME,
        });
        self.roll_cool = ROLL_COOLDOWN;
        true
    }

    /// One step of `dt` seconds.
    pub fn update(&mut self, dt: f32, solids: &Solids) {
        self.was = self.pos;
        self.roll_cool = (self.roll_cool - dt).max(0.0);
        if let Some(roll) = self.roll.as_mut() {
            let travel = dt.min(roll.left.max(0.0));
            roll.left -= dt;
            let (dir, done) = (roll.dir, roll.left <= 0.0);
            let pace = ROLL_DISTANCE / ROLL_TIME;
            self.heading = dir;
            self.intent = dir;
            self.speed = pace;
            self.pos += Vec2::from_angle(dir) * (pace * travel);
            self.keep_clear(solids);
            self.stride = (self.stride + pace * travel * 0.10) % TAU;
            if done {
                self.roll = None;
                self.speed = MARCH_SPEED;
                self.heading = wrap_angle(self.steer.aim);
            }
            return;
        }

        let steer = self.steer;
        let sprinting = self.is_sprinting();
        let goal = if let Some(walk) = steer.walk {
            self.intent = walk;
            self.target_speed = MARCH_SPEED * if sprinting { SPRINT } else { 1.0 };
            walk
        } else {
            self.target_speed = 0.0;
            self.heading
        };

        if sprinting {
            self.heading = angle_lerp(self.heading, goal, approach(SPRINT_TURN, dt));
        } else {
            self.heading = wrap_angle(steer.aim);
        }

        // Any slowing under the keys is at once; speeding up eases, and a
        // sprint eases in from the walk over `SPRINT_EASE`.
        if self.target_speed < self.speed {
            self.speed = self.target_speed;
        } else {
            let mut rate = ACCEL * STEER_ACCEL;
            if sprinting {
                rate = rate.max(MARCH_SPEED * (SPRINT - 1.0) / SPRINT_EASE);
            }
            let step = rate * dt;
            self.speed += clamp(self.target_speed - self.speed, -step, step);
        }

        let along = Vec2::from_angle(self.intent);
        let from = self.pos;
        let stride = self.speed * dt;
        self.pos += along * stride;
        // Round a lone corner at the whole pace: what the corner took off
        // the step is given back along the way it was turned.
        if self.keep_clear(solids) {
            let moved = self.pos - from;
            let went = moved.length();
            if went > 0.05 * stride && went < stride && moved.dot(along) > SLIDE_KEEPS_PACE * went {
                self.pos = from + moved * (stride / went);
                self.keep_clear(solids);
            }
        }

        let paces = self.speed * dt * 0.10;
        let backwards = along.dot(Vec2::from_angle(self.heading)) < 0.0;
        self.stride = (self.stride + if backwards { -paces } else { paces }) % TAU;
    }

    /// Inside the bounds, then out of anything walked into — round a lone
    /// corner rather than square off it. Whether a rounded corner pushed.
    fn keep_clear(&mut self, solids: &Solids) -> bool {
        self.pos = solids.bounds.expand(-BODY_MARGIN).nearest(self.pos);
        let exposed = |c: Vec2, sx: f32, sy: f32| {
            let e = 1.0;
            let probes = [
                vec2(c.x + sx * e, c.y - sy * e),
                vec2(c.x - sx * e, c.y + sy * e),
                vec2(c.x + sx * e, c.y + sy * e),
            ];
            !probes.iter().any(|&q| solids.solid_at(q))
        };
        let mut rounded = false;
        let near: Vec<u32> = solids.near(self.pos).to_vec();
        for i in near {
            let solid = solids.rects[i as usize];
            if let Some((out, round)) =
                solid.push_out_round(self.pos, BODY_MARGIN, CORNER_ROUND, exposed)
            {
                self.pos += out;
                rounded |= round;
            }
        }
        rounded
    }
}

/// `f32::clamp` without the panic when the bounds cross.
pub fn clamp(v: f32, lo: f32, hi: f32) -> f32 {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

/// The share of the way to a goal a rate covers in `dt`, frame-rate free.
pub fn approach(rate: f32, dt: f32) -> f32 {
    1.0 - (-rate * dt).exp()
}

pub fn wrap_angle(a: f32) -> f32 {
    let mut a = (a + PI) % TAU;
    if a < 0.0 {
        a += TAU;
    }
    a - PI
}

/// From `a` towards `b` by share `t`, the short way round.
pub fn angle_lerp(a: f32, b: f32, t: f32) -> f32 {
    wrap_angle(a + wrap_angle(b - a) * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open() -> Solids {
        Solids::new(
            Aabb::new(Vec2::ZERO, vec2(40.0 * TILE, 40.0 * TILE)),
            Vec::new(),
        )
    }

    fn tile(x: i32, y: i32) -> Aabb {
        let min = vec2(x as f32, y as f32) * TILE;
        Aabb::new(min, min + Vec2::splat(TILE))
    }

    fn walk(body: &mut Body, solids: &Solids, angle: f32, seconds: f32) {
        let steps = (seconds / STEP).round() as usize;
        for _ in 0..steps {
            body.set_steer(Steer {
                walk: Some(angle),
                aim: body.steer.aim,
                sprint: body.steer.sprint,
            });
            body.update(STEP, solids);
        }
    }

    #[test]
    fn the_keys_reach_the_march_quickly_and_stop_at_once() {
        let solids = open();
        let mut b = Body::new(vec2(500.0, 500.0), 0.0);
        walk(&mut b, &solids, 0.0, 0.25);
        assert!((b.speed - MARCH_SPEED).abs() < 1e-3, "{}", b.speed);
        b.set_steer(Steer::default());
        let at = b.pos;
        b.update(STEP, &solids);
        assert_eq!(b.speed, 0.0);
        assert_eq!(b.pos, at);
    }

    #[test]
    fn a_body_faces_the_pointer_at_once_and_strides_back_walking_away() {
        let solids = open();
        let mut b = Body::new(vec2(500.0, 500.0), 0.0);
        b.set_steer(Steer {
            walk: Some(PI),
            aim: 0.0,
            sprint: false,
        });
        for _ in 0..30 {
            b.update(STEP, &solids);
        }
        assert_eq!(b.heading, 0.0);
        assert!(b.pos.x < 500.0);
        assert!(b.stride < 0.0, "{}", b.stride);
    }

    #[test]
    fn a_sprint_is_quicker_and_faces_the_run() {
        let solids = open();
        let mut b = Body::new(vec2(500.0, 500.0), 0.0);
        b.set_steer(Steer {
            walk: Some(PI / 2.0),
            aim: 0.0,
            sprint: true,
        });
        for _ in 0..60 {
            b.update(STEP, &solids);
        }
        assert!((b.speed - MARCH_SPEED * SPRINT).abs() < 1e-3);
        assert!((b.heading - PI / 2.0).abs() < 0.01, "{}", b.heading);
    }

    #[test]
    fn a_roll_goes_its_distance_the_way_the_keys_walked_and_cools_down() {
        let solids = open();
        let mut b = Body::new(vec2(500.0, 500.0), 0.0);
        b.set_steer(Steer {
            walk: Some(PI / 2.0),
            aim: 0.0,
            sprint: false,
        });
        b.update(STEP, &solids);
        b.set_steer(Steer::default());
        let from = b.pos;
        assert!(b.start_roll());
        while b.is_rolling() {
            b.update(STEP, &solids);
        }
        let went = b.pos - from;
        assert!((went.y - ROLL_DISTANCE).abs() < 0.5, "{went:?}");
        assert!(went.x.abs() < 1e-3);
        assert!(!b.start_roll());
    }

    #[test]
    fn a_lone_corner_is_slid_round_and_a_wall_of_tiles_has_no_seams() {
        // A pillar a tile square: walked at a few units off its edge, the
        // body comes round it rather than stopping.
        let solids = Solids::new(
            Aabb::new(Vec2::ZERO, vec2(40.0 * TILE, 40.0 * TILE)),
            vec![tile(10, 10)],
        );
        let mut b = Body::new(
            vec2(10.0 * TILE - 200.0, 11.0 * TILE + BODY_MARGIN - 6.0),
            0.0,
        );
        walk(&mut b, &solids, 0.0, 2.0);
        assert!(b.pos.x > 11.0 * TILE + BODY_MARGIN, "{:?}", b.pos);

        // A wall of tiles: sliding along it keeps a straight line.
        let wall: Vec<Aabb> = (0..20).map(|x| tile(x, 5)).collect();
        let solids = Solids::new(Aabb::new(Vec2::ZERO, vec2(40.0 * TILE, 40.0 * TILE)), wall);
        let y = 6.0 * TILE + BODY_MARGIN;
        let mut b = Body::new(vec2(TILE, y), 0.0);
        // Pressed into the wall at a slant: it slides along, never caught.
        for _ in 0..120 {
            b.set_steer(Steer {
                walk: Some(-0.3),
                aim: 0.0,
                sprint: false,
            });
            b.update(STEP, &solids);
            assert!((b.pos.y - y).abs() < 1e-3, "{:?}", b.pos);
        }
        assert!(b.pos.x > TILE + 300.0);
    }
}
