//! Bank Bobbery — on foot, at night, round a bank nobody has built before.
//!
//! The movement is bims_bevy's steered walk (`movement.rs`), the bank is
//! generated from a seed (`map.rs`), and what the robber sees — and what
//! the lamps light — is ray traced per pixel through the bank's walls and
//! furniture (`shaders/world.wgsl`). No gameplay yet: walk, sprint, roll.
//!
//! WASD walk, the mouse aims, Shift sprints, Space (or Alt) rolls, the
//! wheel zooms, R builds a new bank, F11 is full screen.

mod dev;
mod map;
mod movement;
mod paint;
mod player;
mod render;
mod rng;

use bevy::camera::Hdr;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::window::{PresentMode, WindowMode, WindowResolution};

use crate::movement::{Body, Solids};
use crate::player::{Player, PlayerPlugin, spawn_robber};
use crate::render::{Lamps, RenderPlugin, WorldMaterial, WorldMaterials, WorldPart};

/// The map to build next, if one is asked for.
#[derive(Resource)]
struct NewMap(Option<u64>);

/// The map on show.
#[derive(Resource)]
struct Current {
    seed: u64,
    rooms: usize,
    props: usize,
    lamps: usize,
}

#[derive(Component)]
struct Hud;

fn main() {
    let seed = dev::env("BB_SEED")
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(clock_seed);
    let (w, h) = dev::window_size();
    let smoke = dev::smoke_frames().is_some();
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Bank Bobbery".into(),
                resolution: WindowResolution::new(w, h),
                present_mode: if smoke {
                    PresentMode::AutoNoVsync
                } else {
                    PresentMode::AutoVsync
                },
                ..default()
            }),
            ..default()
        }))
        .add_plugins(FrameTimeDiagnosticsPlugin::default())
        .add_plugins((RenderPlugin, PlayerPlugin, dev::DevPlugin))
        .insert_resource(ClearColor(Color::srgb(0.01, 0.012, 0.02)))
        .insert_resource(NewMap(Some(seed)))
        .init_resource::<WorldMaterials>()
        .init_resource::<Lamps>()
        .add_systems(Startup, setup)
        .add_systems(Update, (keys, build_map, hud).chain())
        .run();
}

fn clock_seed() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() % 100_000)
        .unwrap_or(1)
}

fn setup(mut commands: Commands) {
    let scale = dev::env("BB_ZOOM")
        .and_then(|z| z.parse().ok())
        .unwrap_or(0.7f32);
    commands.spawn((
        Camera2d,
        Hdr,
        Tonemapping::None,
        Bloom {
            intensity: 0.12,
            ..Bloom::NATURAL
        },
        Projection::Orthographic(OrthographicProjection {
            scale,
            ..OrthographicProjection::default_2d()
        }),
    ));
    commands.spawn((
        Hud,
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(14.0),
            ..default()
        },
        TextColor(Color::srgba(0.85, 0.85, 0.8, 0.85)),
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(8.0),
            top: Val::Px(8.0),
            padding: UiRect::axes(Val::Px(8.0), Val::Px(5.0)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.02, 0.6)),
    ));
}

fn keys(
    input: Res<ButtonInput<KeyCode>>,
    current: Option<Res<Current>>,
    mut new_map: ResMut<NewMap>,
    mut window: Query<&mut Window>,
) {
    if input.just_pressed(KeyCode::KeyR) {
        let next = current.map_or(1, |c| {
            c.seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407)
                % 100_000
        });
        new_map.0 = Some(next);
    }
    if input.just_pressed(KeyCode::F11)
        && let Ok(mut w) = window.single_mut()
    {
        w.mode = match w.mode {
            WindowMode::Windowed => WindowMode::BorderlessFullscreen(MonitorSelection::Current),
            _ => WindowMode::Windowed,
        };
    }
}

#[allow(clippy::too_many_arguments)]
fn build_map(
    mut commands: Commands,
    mut new_map: ResMut<NewMap>,
    old: Query<Entity, With<WorldPart>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<WorldMaterial>>,
    mut world_materials: ResMut<WorldMaterials>,
    mut lamps: ResMut<Lamps>,
) {
    let Some(seed) = new_map.0.take() else {
        return;
    };
    for e in &old {
        commands.entity(e).despawn();
    }
    let map = map::generate(seed);
    render::spawn_map(
        &mut commands,
        &map,
        &mut meshes,
        &mut images,
        &mut materials,
        &mut world_materials,
        &mut lamps,
    );
    // `BB_SPAWN=x,y`: start on that tile instead, for a look round.
    let spawn = dev::env("BB_SPAWN")
        .and_then(|v| {
            let (x, y) = v.split_once(',')?;
            Some(map::tile_centre(x.parse().ok()?, y.parse().ok()?))
        })
        .unwrap_or(map.spawn);
    spawn_robber(
        &mut commands,
        &mut meshes,
        render::things_material(&world_materials),
        spawn,
    );
    commands.insert_resource(Player {
        body: Body::new(spawn, std::f32::consts::FRAC_PI_2),
        solids: Solids::new(map.bounds(), map.solids()),
    });
    commands.insert_resource(Current {
        seed,
        rooms: map.rooms.len(),
        props: map.props.len(),
        lamps: map.lights.len(),
    });
    info!(
        "bank {seed}: {}x{} tiles, {} rooms, {} props, {} lamps",
        map.w,
        map.h,
        map.rooms.len(),
        map.props.len(),
        map.lights.len()
    );
}

fn hud(
    current: Option<Res<Current>>,
    player: Option<Res<Player>>,
    diagnostics: Res<DiagnosticsStore>,
    mut text: Query<&mut Text, With<Hud>>,
) {
    let (Some(current), Some(player), Ok(mut text)) = (current, player, text.single_mut()) else {
        return;
    };
    let fps = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|d| d.smoothed())
        .unwrap_or(0.0);
    let b = &player.body;
    let state = if b.is_rolling() {
        "rolling"
    } else if b.is_sprinting() {
        "sprinting"
    } else if b.speed > 1.0 {
        "walking"
    } else {
        "standing"
    };
    let roll = if b.roll_cooling() > 0.0 {
        "cooling"
    } else {
        "ready"
    };
    text.0 = format!(
        "BANK BOBBERY   bank #{}  ({} rooms, {} props, {} lamps)   {:.0} fps\n\
         WASD walk | mouse aim | Shift sprint | Space/Alt roll | wheel zoom | R new bank | F11 full screen\n\
         {state}   roll {roll}",
        current.seed, current.rooms, current.props, current.lamps, fps
    );
}
