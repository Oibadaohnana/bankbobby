//! The robber: the keys and the pointer read every frame, the body moved at
//! a fixed sixty steps a second (`movement.rs`, bims' rules), and the
//! picture stood between the last two steps and animated off the stride.

use crate::dev::Script;
use crate::movement::{Body, MARCH_SPEED, STEP, Solids, Steer, approach};
use crate::paint;
use crate::render::{WorldMaterial, WorldMaterials, WorldPart};
use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use std::f32::consts::TAU;

/// The body and what it bumps into.
#[derive(Resource)]
pub struct Player {
    pub body: Body,
    pub solids: Solids,
}

/// What the keys and the pointer say, gathered once a frame and read by
/// every fixed step until the next.
#[derive(Resource, Default)]
pub struct Controls {
    pub walk: Option<f32>,
    pub aim: f32,
    pub sprint: bool,
    /// A roll asked for and not yet started.
    pub roll: bool,
}

/// Where the picture stands this frame, between the last two steps.
#[derive(Resource, Default)]
pub struct Shown {
    pub pos: Vec2,
}

#[derive(Component)]
pub struct Robber;

#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    Shadow,
    Foot(i8),
    Bag,
    Torso,
    Arm(i8),
    Head,
    Streak(i8),
}

pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Time::<Fixed>::from_seconds(STEP as f64))
            .init_resource::<Controls>()
            .init_resource::<Shown>()
            .add_systems(
                PreUpdate,
                read_controls
                    .after(bevy::input::InputSystems)
                    .run_if(resource_exists::<Player>),
            )
            .add_systems(FixedUpdate, step.run_if(resource_exists::<Player>))
            .add_systems(
                Update,
                (show, animate, follow, zoom)
                    .chain()
                    .run_if(resource_exists::<Player>),
            );
    }
}

/// Spawn the robber at the body, its parts as children.
pub fn spawn_robber(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    material: Handle<WorldMaterial>,
    at: Vec2,
) {
    let parts = paint::robber();
    let mesh = |m: Mesh, meshes: &mut Assets<Mesh>| Mesh2d(meshes.add(m));
    let mat = MeshMaterial2d(material);
    let foot = meshes.add(parts.foot);
    let arm = meshes.add(parts.arm);
    let streak = meshes.add(parts.streak);
    commands
        .spawn((
            WorldPart,
            Robber,
            Transform::from_xyz(at.x, at.y, 10.0),
            Visibility::default(),
        ))
        .with_children(|r| {
            r.spawn((
                Part::Shadow,
                mesh(parts.shadow, meshes),
                mat.clone(),
                Transform::from_xyz(0.0, 0.0, -0.9),
            ));
            for side in [-1i8, 1] {
                r.spawn((
                    Part::Foot(side),
                    Mesh2d(foot.clone()),
                    mat.clone(),
                    Transform::from_xyz(0.0, 8.0 * side as f32, -0.6),
                ));
            }
            r.spawn((
                Part::Torso,
                mesh(parts.torso, meshes),
                mat.clone(),
                Transform::from_xyz(-1.0, 0.0, 0.0),
            ));
            r.spawn((
                Part::Bag,
                mesh(parts.bag, meshes),
                mat.clone(),
                Transform::from_xyz(-13.0, 0.0, 0.2),
            ));
            for side in [-1i8, 1] {
                let mut arm_entity = r.spawn((
                    Part::Arm(side),
                    Mesh2d(arm.clone()),
                    mat.clone(),
                    Transform::from_xyz(1.0, 14.0 * side as f32, 0.3),
                ));
                if side < 0 {
                    let crowbar = meshes.add(paint::robber().crowbar);
                    arm_entity.with_children(|a| {
                        a.spawn((
                            Mesh2d(crowbar),
                            mat.clone(),
                            Transform::from_xyz(15.0, 0.0, 0.05)
                                .with_rotation(Quat::from_rotation_z(0.35)),
                        ));
                    });
                }
            }
            r.spawn((
                Part::Head,
                mesh(parts.head, meshes),
                mat.clone(),
                Transform::from_xyz(2.0, 0.0, 0.5),
            ));
            for k in [-1i8, 0, 1] {
                r.spawn((
                    Part::Streak(k),
                    Mesh2d(streak.clone()),
                    mat.clone(),
                    Transform::from_xyz(-24.0, 9.0 * k as f32, -0.8),
                    Visibility::Hidden,
                ));
            }
        });
}

fn read_controls(
    keys: Res<ButtonInput<KeyCode>>,
    script: Res<Script>,
    window: Query<&Window, With<PrimaryWindow>>,
    camera: Query<(&Camera, &GlobalTransform)>,
    shown: Res<Shown>,
    mut controls: ResMut<Controls>,
) {
    let held = |k: KeyCode, name: &str| keys.pressed(k) || script.held(name);
    let mut d = Vec2::ZERO;
    if held(KeyCode::KeyW, "W") || held(KeyCode::ArrowUp, "Up") {
        d.y += 1.0;
    }
    if held(KeyCode::KeyS, "S") || held(KeyCode::ArrowDown, "Down") {
        d.y -= 1.0;
    }
    if held(KeyCode::KeyA, "A") || held(KeyCode::ArrowLeft, "Left") {
        d.x -= 1.0;
    }
    if held(KeyCode::KeyD, "D") || held(KeyCode::ArrowRight, "Right") {
        d.x += 1.0;
    }
    controls.walk = (d != Vec2::ZERO).then(|| d.y.atan2(d.x));
    controls.sprint = held(KeyCode::ShiftLeft, "Shift") || keys.pressed(KeyCode::ShiftRight);
    if keys.just_pressed(KeyCode::Space)
        || keys.just_pressed(KeyCode::AltLeft)
        || keys.just_pressed(KeyCode::AltRight)
        || script.pressed("Roll")
    {
        controls.roll = true;
    }
    if let Some(aim) = script.aim {
        controls.aim = aim;
        return;
    }
    let (Ok(window), Ok((camera, at))) = (window.single(), camera.single()) else {
        return;
    };
    if let Some(cursor) = window.cursor_position()
        && let Ok(world) = camera.viewport_to_world_2d(at, cursor)
    {
        let off = world - shown.pos;
        if off.length() > 1.0 {
            controls.aim = off.y.atan2(off.x);
        }
    }
}

fn step(mut player: ResMut<Player>, mut controls: ResMut<Controls>) {
    let player = &mut *player;
    player.body.set_steer(Steer {
        walk: controls.walk,
        aim: controls.aim,
        sprint: controls.sprint,
    });
    if controls.roll {
        controls.roll = false;
        player.body.start_roll();
    }
    player.body.update(STEP, &player.solids);
}

/// Stand the picture between the last two steps.
fn show(player: Res<Player>, time: Res<Time<Fixed>>, mut shown: ResMut<Shown>) {
    let b = &player.body;
    let blend = time.overstep_fraction();
    shown.pos = if b.pos.distance(b.was) > 52.0 {
        b.pos
    } else {
        b.was + (b.pos - b.was) * blend
    };
}

fn animate(
    player: Res<Player>,
    shown: Res<Shown>,
    time: Res<Time>,
    mut root: Query<&mut Transform, (With<Robber>, Without<Part>)>,
    mut parts: Query<(&Part, &mut Transform, &mut Visibility), Without<Robber>>,
) {
    let b = &player.body;
    let Ok(mut t) = root.single_mut() else {
        return;
    };
    let rolling = b.roll_share();
    t.translation = shown.pos.extend(10.0);
    let heading = match rolling {
        // A roll spins the body once round, tucked small.
        Some(share) => b.roll_dir().unwrap_or(b.heading) + share * TAU,
        None => b.heading,
    };
    t.rotation = Quat::from_rotation_z(heading);
    let tuck = rolling.map_or(1.0, |s| 0.8 + 0.2 * (s * TAU).cos().abs());
    t.scale = Vec3::splat(tuck);

    let sprint = b.is_sprinting();
    let pace = (b.speed / MARCH_SPEED).clamp(0.0, 1.0);
    let s = b.stride.sin();
    let reach = if sprint { 15.0 } else { 9.5 };
    let clock = time.elapsed_secs();
    for (part, mut pt, mut vis) in &mut parts {
        match *part {
            Part::Foot(side) => {
                let side = side as f32;
                pt.translation.x = s * side * reach * pace;
                pt.translation.y = 8.0 * side;
            }
            Part::Arm(side) => {
                let sd = side as f32;
                // Hands in front, a sway on the walk, pumping on a sprint.
                let swing = if sprint { 0.55 } else { 0.14 };
                let base = -0.42 * sd;
                let a = base - s * sd * swing * pace;
                pt.rotation = Quat::from_rotation_z(a);
                pt.translation.x = 1.0 + if sprint { -s * sd * 3.0 } else { 0.0 };
            }
            Part::Torso => {
                pt.rotation = Quat::from_rotation_z(0.07 * s * pace);
            }
            Part::Bag => {
                let bob = 1.0 + 0.04 * (2.0 * b.stride).sin() * pace;
                pt.scale = Vec3::new(bob, 1.0 / bob, 1.0);
                pt.rotation = Quat::from_rotation_z(-0.1 * s * pace);
            }
            Part::Head => {
                pt.translation.x = if sprint { 4.5 } else { 2.0 };
            }
            Part::Streak(k) => {
                *vis = if sprint && rolling.is_none() {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                };
                let flicker = 0.6 + 0.4 * ((clock * 30.0 + k as f32 * 2.1).sin() * 0.5 + 0.5);
                pt.scale = Vec3::new(flicker * (1.0 + 0.4 * (k as f32).abs()), 1.0, 1.0);
            }
            Part::Shadow => {}
        }
    }
}

/// The camera stays on the robber.
fn follow(
    shown: Res<Shown>,
    mut camera: Query<&mut Transform, (With<Camera2d>, Without<Robber>)>,
    world_materials: Res<WorldMaterials>,
    lamps: Res<crate::render::Lamps>,
    mut materials: ResMut<Assets<WorldMaterial>>,
    time: Res<Time>,
) {
    if let Ok(mut c) = camera.single_mut() {
        c.translation.x = shown.pos.x;
        c.translation.y = shown.pos.y;
    }
    // The eyes are the robber's head, where the picture stands.
    crate::render::write_params(
        shown.pos,
        time.elapsed_secs(),
        &world_materials,
        &lamps,
        &mut materials,
    );
}

/// The wheel zooms, eased.
fn zoom(
    mut wheel: MessageReader<MouseWheel>,
    mut projection: Query<&mut Projection, With<Camera2d>>,
    mut goal: Local<Option<f32>>,
    time: Res<Time>,
) {
    let Ok(mut projection) = projection.single_mut() else {
        return;
    };
    let Projection::Orthographic(ortho) = &mut *projection else {
        return;
    };
    let goal = goal.get_or_insert(ortho.scale);
    for e in wheel.read() {
        let notches = match e.unit {
            bevy::input::mouse::MouseScrollUnit::Line => e.y,
            bevy::input::mouse::MouseScrollUnit::Pixel => e.y / 40.0,
        };
        *goal = (*goal * 0.88f32.powf(notches)).clamp(0.45, 2.6);
    }
    ortho.scale += (*goal - ortho.scale) * approach(14.0, time.delta_secs());
}
