//! Running the game with nobody at the keyboard, for checking a change:
//!
//! - `BB_SEED=n` — the map's seed (otherwise the clock's);
//! - `BB_SMOKE_FRAMES=n` — run `n` frames and exit, each frame exactly a
//!   sixtieth of a second, so a run plays the same every time;
//! - `BB_SCREENSHOT=path.png` — save the frame thirty before the end;
//! - `BB_KEYS="10:+W,40:-W,40:+D,45:Roll,60:-D"` — hold (`+`) and let go
//!   (`-`) of W A S D Shift Up Down Left Right at those frames, or press
//!   `Roll` once;
//! - `BB_AIM=degrees` — aim there instead of at the pointer (0 is east,
//!   90 north);
//! - `BB_ZOOM=scale` — world units a pixel (0.7 by default);
//! - `BB_WINDOW=1400x900` — the window's size;
//! - `BB_SPAWN=x,y` — start on that tile;
//! - `BB_UNPACED=1` — a smoke run's frames as fast as they go, the
//!   average frame rate logged at the end.
//!
//! `./hidden` runs a smoke run on a headless compositor.

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use std::collections::HashSet;

pub fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

pub fn smoke_frames() -> Option<u32> {
    env("BB_SMOKE_FRAMES").and_then(|v| v.parse().ok())
}

pub fn window_size() -> (u32, u32) {
    env("BB_WINDOW")
        .and_then(|v| {
            let (w, h) = v.split_once('x')?;
            Some((w.parse().ok()?, h.parse().ok()?))
        })
        .unwrap_or((1400, 900))
}

/// The scripted keys and aim.
#[derive(Resource, Default)]
pub struct Script {
    events: Vec<(u32, String)>,
    held: HashSet<String>,
    pressed: HashSet<String>,
    pub aim: Option<f32>,
}

impl Script {
    pub fn from_env() -> Self {
        let mut s = Script::default();
        if let Some(keys) = env("BB_KEYS") {
            for item in keys.split(',') {
                if let Some((frame, what)) = item.trim().split_once(':')
                    && let Ok(frame) = frame.parse()
                {
                    s.events.push((frame, what.to_string()));
                }
            }
        }
        s.aim = env("BB_AIM")
            .and_then(|v| v.parse::<f32>().ok())
            .map(f32::to_radians);
        s
    }

    pub fn held(&self, name: &str) -> bool {
        self.held.contains(name)
    }

    pub fn pressed(&self, name: &str) -> bool {
        self.pressed.contains(name)
    }
}

#[derive(Resource, Default)]
pub struct Frames(pub u32);

pub struct DevPlugin;

impl Plugin for DevPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Script::from_env())
            .init_resource::<Frames>()
            .add_systems(First, (count, play_script).chain())
            .add_systems(Last, smoke);
        if smoke_frames().is_some() {
            app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
                std::time::Duration::from_secs_f64(1.0 / 60.0),
            ));
        }
    }
}

fn count(mut frames: ResMut<Frames>) {
    frames.0 += 1;
}

fn play_script(frames: Res<Frames>, mut script: ResMut<Script>) {
    script.pressed.clear();
    let now: Vec<String> = script
        .events
        .iter()
        .filter(|(f, _)| *f == frames.0)
        .map(|(_, w)| w.clone())
        .collect();
    for what in now {
        if let Some(key) = what.strip_prefix('+') {
            script.held.insert(key.to_string());
        } else if let Some(key) = what.strip_prefix('-') {
            script.held.remove(key);
        } else {
            script.pressed.insert(what);
        }
    }
}

fn smoke(
    mut commands: Commands,
    frames: Res<Frames>,
    mut exit: MessageWriter<AppExit>,
    mut last: Local<Option<std::time::Instant>>,
    mut warm: Local<Option<std::time::Instant>>,
) {
    // The frames after the shaders have compiled are the ones timed.
    const WARM: u32 = 120;
    if frames.0 == WARM {
        *warm = Some(std::time::Instant::now());
    }
    let Some(limit) = smoke_frames() else {
        return;
    };
    // Paced at sixty: a headless compositor has no vblank, and a frame
    // count is only a time if the frames take one — the shaders compile
    // in the first second.
    // `BB_UNPACED=1` runs flat out instead, and says how fast it went.
    let frame = std::time::Duration::from_secs_f64(1.0 / 60.0);
    let unpaced = env("BB_UNPACED").is_some();
    if unpaced {
        // nothing
    } else if let Some(at) = *last {
        let spent = at.elapsed();
        if spent < frame {
            std::thread::sleep(frame - spent);
        }
    }
    *last = Some(std::time::Instant::now());
    if frames.0 + 30 == limit
        && let Some(path) = env("BB_SCREENSHOT")
    {
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path));
    }
    if frames.0 >= limit {
        if let Some(at) = *warm {
            let n = frames.0.saturating_sub(WARM);
            let secs = at.elapsed().as_secs_f64();
            info!(
                "smoke run: {n} frames after the first {WARM} in {secs:.2} s, {:.0} fps",
                n as f64 / secs
            );
        }
        exit.write(AppExit::Success);
    }
}
