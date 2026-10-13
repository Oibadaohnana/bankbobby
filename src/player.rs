use crate::dev::Script;
use crate::movement::{Aabb, BODY_MARGIN, Body, MARCH_SPEED, STEP, Solids, Steer, TILE, approach};
use crate::paint;
use crate::render::{Lamps, WorldMaterial, WorldMaterials, WorldPart, write_params};
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use std::f32::consts::{PI, TAU};

#[derive(Resource)]
pub struct Player {
    pub body: Body,
    pub solids: Solids,
    /// The half-height props' bodies.
    pub cover: Vec<Aabb>,
}

impl Player {
    pub fn in_cover(&self) -> bool {
        self.body.is_crouching()
            && self
                .cover
                .iter()
                .any(|c| c.expand(BODY_MARGIN + TILE * 0.5).contains(self.body.pos))
    }
}

#[derive(Resource, Default)]
pub struct Controls {
    pub walk: Option<f32>,
    pub aim: f32,
    pub sprint: bool,
    /// A roll asked for and not yet started.
    pub roll: bool,
    pub torch: bool,
    pub crouch: bool,
}

#[derive(Resource, Default)]
pub struct Shown {
    pub pos: Vec2,
}

#[derive(Component)]
pub struct Robber;

#[derive(Component, Clone, Copy)]
pub enum Part {
    Foot(i8),
    Knee(i8),
    Bag,
    Torso,
    Arm(i8),
    Lens,
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

pub fn spawn_robber(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    material: Handle<WorldMaterial>,
    at: Vec2,
) {
    let p = paint::robber();
    let [foot, knee, arm, crowbar, torch, lens, streak] =
        [p.foot, p.knee, p.arm, p.crowbar, p.torch, p.lens, p.streak]
            .map(|m| Mesh2d(meshes.add(m)));
    let mat = MeshMaterial2d(material);
    commands
        .spawn((
            WorldPart,
            Robber,
            Transform::from_xyz(at.x, at.y, 10.0),
            Visibility::default(),
        ))
        .with_children(|r| {
            r.spawn((
                Mesh2d(meshes.add(p.shadow)),
                mat.clone(),
                Transform::from_xyz(0.0, 0.0, -0.9),
            ));
            for side in [-1i8, 1] {
                r.spawn((
                    Part::Foot(side),
                    foot.clone(),
                    mat.clone(),
                    Transform::from_xyz(0.0, 8.0 * side as f32, -0.6),
                ));
                r.spawn((
                    Part::Knee(side),
                    knee.clone(),
                    mat.clone(),
                    Transform::from_xyz(0.0, 0.0, -0.3).with_scale(Vec3::ZERO),
                ));
            }
            r.spawn((
                Part::Torso,
                Mesh2d(meshes.add(p.torso)),
                mat.clone(),
                Transform::from_xyz(-1.0, 0.0, 0.0),
            ));
            r.spawn((
                Part::Bag,
                Mesh2d(meshes.add(p.bag)),
                mat.clone(),
                Transform::from_xyz(-13.0, 0.0, 0.2),
            ));
            for side in [-1i8, 1] {
                let hand = Transform::from_xyz(15.0, 0.0, 0.05);
                r.spawn((
                    Part::Arm(side),
                    arm.clone(),
                    mat.clone(),
                    Transform::from_xyz(1.0, 14.0 * side as f32, 0.3),
                ))
                .with_children(|a| {
                    if side < 0 {
                        a.spawn((
                            crowbar.clone(),
                            mat.clone(),
                            hand.with_rotation(Quat::from_rotation_z(0.35)),
                        ));
                    } else {
                        a.spawn((
                            torch.clone(),
                            mat.clone(),
                            hand.with_rotation(Quat::from_rotation_z(0.42)),
                        ))
                        .with_children(|t| {
                            t.spawn((
                                Part::Lens,
                                lens.clone(),
                                mat.clone(),
                                Transform::from_xyz(0.0, 0.0, 0.01),
                                Visibility::Hidden,
                            ));
                        });
                    }
                });
            }
            r.spawn((
                Part::Head,
                Mesh2d(meshes.add(p.head)),
                mat.clone(),
                Transform::from_xyz(2.0, 0.0, 0.5),
            ));
            for k in [-1i8, 0, 1] {
                r.spawn((
                    Part::Streak(k),
                    streak.clone(),
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
    if keys.just_pressed(KeyCode::KeyC) || script.pressed("Crouch") {
        controls.crouch = !controls.crouch;
    }
    if controls.sprint && controls.walk.is_some() {
        controls.crouch = false;
    }
    if keys.just_pressed(KeyCode::Space)
        || keys.just_pressed(KeyCode::AltLeft)
        || keys.just_pressed(KeyCode::AltRight)
        || script.pressed("Roll")
    {
        controls.roll = true;
    }
    if keys.just_pressed(KeyCode::KeyT) || script.pressed("Torch") {
        controls.torch = !controls.torch;
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
        crouch: controls.crouch,
    });
    if controls.roll {
        controls.roll = false;
        player.body.start_roll();
    }
    player.body.update(STEP, &player.solids);
}

fn show(player: Res<Player>, time: Res<Time<Fixed>>, mut shown: ResMut<Shown>) {
    let b = &player.body;
    shown.pos = if b.pos.distance(b.was) > 52.0 {
        b.pos
    } else {
        b.was + (b.pos - b.was) * time.overstep_fraction()
    };
}

fn animate(
    player: Res<Player>,
    controls: Res<Controls>,
    shown: Res<Shown>,
    time: Res<Time>,
    mut crouch: Local<f32>,
    mut root: Query<&mut Transform, (With<Robber>, Without<Part>)>,
    mut parts: Query<(&Part, &mut Transform, &mut Visibility), Without<Robber>>,
) {
    let b = &player.body;
    let Ok(mut t) = root.single_mut() else {
        return;
    };
    let rolling = b.roll_share();
    t.translation = shown.pos.extend(10.0);
    t.rotation = Quat::from_rotation_z(b.heading);
    let down = if b.is_crouching() { 1.0 } else { 0.0 };
    *crouch += (down - *crouch) * approach(14.0, time.delta_secs());
    // A roll tucks into the crouch pose, quickly, and back out at the end.
    let tuck = rolling.map_or(0.0, |s| ((PI * s).sin() * 2.0).min(1.0));
    let c = crouch.max(tuck);
    // Crouched from above: folded forward, knees out in front, a kneeling stance.
    let m = |stand: f32, low: f32| stand + (low - stand) * c;
    t.scale = Vec3::splat(m(1.0, 0.92) * (1.0 - 0.12 * tuck));
    // Rolling, every part turns head over heels about the axis across the
    // body: a part `up` above the middle goes forward, under, and back over
    // the top. Draw order follows height.
    let (sin, cos) = rolling.map_or((0.0, 1.0), |s| (s * TAU).sin_cos());
    let flip = |pt: &mut Transform, x: f32, up: f32, z: f32| {
        pt.translation.x = x * cos + up * sin;
        pt.translation.z = if rolling.is_some() {
            (up * cos - x * sin) * 0.03
        } else {
            z
        };
    };

    let sprint = b.is_sprinting();
    let pace = if rolling.is_some() {
        0.0
    } else {
        (b.speed / MARCH_SPEED).clamp(0.0, 1.0)
    };
    let s = b.stride.sin();
    let reach = if sprint { 15.0 } else { 9.5 };
    let clock = time.elapsed_secs();
    for (part, mut pt, mut vis) in &mut parts {
        match *part {
            Part::Foot(side) => {
                let sd = side as f32;
                let shuffle = s * sd * 4.0 * pace;
                flip(
                    &mut pt,
                    m(s * sd * reach * pace, -1.0 - 9.0 * sd + shuffle),
                    -14.0,
                    -0.6,
                );
                pt.translation.y = m(8.0, 10.0) * sd;
            }
            Part::Knee(side) => {
                let sd = side as f32;
                flip(&mut pt, m(4.0, 23.0) + s * sd * 2.0 * pace, 0.0, -0.3);
                pt.translation.y = 7.5 * sd;
                pt.scale = Vec3::splat(c);
            }
            Part::Arm(side) => {
                let sd = side as f32;
                let swing = if sprint { 0.55 } else { m(0.14, 0.05) };
                pt.rotation = Quat::from_rotation_z(m(-0.42, -0.2) * sd - s * sd * swing * pace);
                let x = if sprint {
                    1.0 - s * sd * 3.0
                } else {
                    m(1.0, 6.0)
                };
                flip(&mut pt, x, 8.0, 0.3);
                pt.translation.y = m(14.0, 13.0) * sd;
            }
            Part::Torso => {
                pt.rotation = Quat::from_rotation_z(0.07 * s * pace);
                flip(&mut pt, m(-1.0, 3.0), 2.0, 0.0);
                // Mid-roll it shows its back or chest, longer than the shoulders.
                let long = cos.abs() + 1.7 * sin.abs();
                pt.scale = vec3(m(1.0, 1.25) * long, m(1.0, 0.85), 1.0);
            }
            Part::Bag => {
                let bob = 1.0 + 0.04 * (2.0 * b.stride).sin() * pace;
                pt.scale = Vec3::new(bob, 1.0 / bob, 1.0) * m(1.0, 0.92);
                pt.rotation = Quat::from_rotation_z(-0.1 * s * pace);
                flip(&mut pt, m(-13.0, -6.0), 6.0, 0.2);
            }
            Part::Head => {
                let x = if sprint { 4.5 } else { m(2.0, 11.0) };
                flip(&mut pt, x, 16.0, 0.5);
                pt.scale = Vec3::splat(m(1.0, 0.9));
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
            Part::Lens => {
                *vis = if controls.torch {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                };
            }
        }
    }
}

fn follow(
    shown: Res<Shown>,
    player: Res<Player>,
    controls: Res<Controls>,
    mut camera: Query<&mut Transform, (With<Camera2d>, Without<Robber>)>,
    world_materials: Res<WorldMaterials>,
    lamps: Res<Lamps>,
    mut materials: ResMut<Assets<WorldMaterial>>,
) {
    if let Ok(mut c) = camera.single_mut() {
        c.translation.x = shown.pos.x;
        c.translation.y = shown.pos.y;
    }
    write_params(
        shown.pos,
        player.body.heading,
        controls.torch,
        player.body.is_crouching(),
        &world_materials,
        &lamps,
        &mut materials,
    );
}

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
    // Zoomed out at most to 12.5 tiles above and below the robber.
    let most = match ortho.area.height() {
        h if h > 0.0 => ortho.scale * 25.0 * TILE / h,
        _ => f32::MAX,
    };
    let goal = goal.get_or_insert(ortho.scale);
    for e in wheel.read() {
        let notches = match e.unit {
            MouseScrollUnit::Line => e.y,
            MouseScrollUnit::Pixel => e.y / 40.0,
        };
        *goal *= 0.88f32.powf(notches);
    }
    *goal = goal.clamp(0.45f32.min(most), most);
    ortho.scale += (*goal - ortho.scale) * approach(14.0, time.delta_secs());
}
