use std::f32::consts::FRAC_PI_2;

use avian3d::prelude::*;
use bevy::{
    color::palettes::tailwind,
    input::mouse::AccumulatedMouseMotion,
    prelude::*,
    window::{CursorGrabMode, CursorOptions},
};

use crate::AppState;

pub struct GamePlugin;

// ── Marker components ──────────────────────────────────────────────────────

#[derive(Component)]
pub struct Player;

/// Marks the first-person camera child.
#[derive(Component)]
struct PlayerCamera;

/// Marks a procedural body segment on the player.
#[derive(Component)]
struct BodySegment;

/// Which anatomical part this segment represents.
#[derive(Component, PartialEq, Clone, Copy)]
enum BodySegmentKind {
    Head,
    Neck,
    Torso,
    Clavicle,
    UpperArm,
    Forearm,
    Hand,
    UpperLeg,
    LowerLeg,
    Foot,
}

/// Which side of the body this segment is on.
#[derive(Component, PartialEq, Clone, Copy)]
enum BodySide {
    Left,
    Right,
    Center,
}

/// Marks first-person viewmodel arm segments (children of the camera).
#[derive(Component)]
struct ViewmodelArm;

#[derive(Component)]
pub struct Target;

/// All in-game entities carry this so they are bulk-despawned on exit.
#[derive(Component)]
struct GameWorld;

/// Inserted when the player is standing on the ground; removed when airborne.
#[derive(Component)]
struct Grounded;

#[derive(Component, Deref, DerefMut)]
struct CameraSensitivity(Vec2);

impl Default for CameraSensitivity {
    fn default() -> Self {
        Self(Vec2::new(0.003, 0.002))
    }
}

/// Stores the current camera pitch so it can be shared between systems.
#[derive(Component, Default)]
struct PlayerPitch(f32);

/// Accumulated walk-cycle phase (radians). Driven by horizontal speed.
#[derive(Component, Default)]
struct WalkPhase(f32);

/// First-person or third-person camera view.
#[derive(Resource, Default, PartialEq, Clone, Copy)]
enum CameraMode {
    #[default]
    FirstPerson,
    ThirdPerson,
}

// ── Plugin ─────────────────────────────────────────────────────────────────

impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CameraMode>()
            .add_systems(OnEnter(AppState::InGame), (spawn_world, grab_cursor))
            .add_systems(OnExit(AppState::InGame), (cleanup_game, release_cursor))
            .add_systems(
                Update,
                (
                    check_grounded,
                    move_player,
                    toggle_camera_mode,
                    update_camera,
                    update_body_visibility,
                    update_head_pitch,
                    animate_walk,
                    shoot,
                    handle_escape,
                )
                    .chain()
                    .run_if(in_state(AppState::InGame)),
            );
    }
}

// ── Setup ──────────────────────────────────────────────────────────────────

fn spawn_world(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Three distinct materials: exposed skin, shirt, and pants
    let mat_skin = materials.add(StandardMaterial {
        base_color: Color::srgb(0.90, 0.76, 0.60),
        ..default()
    });
    let mat_shirt = materials.add(StandardMaterial {
        base_color: Color::srgb(0.22, 0.25, 0.35), // dark navy
        ..default()
    });
    let mat_pants = materials.add(StandardMaterial {
        base_color: Color::srgb(0.28, 0.22, 0.16), // dark brown
        ..default()
    });

    // Player + camera + body meshes
    commands
        .spawn((
            Player,
            CameraSensitivity::default(),
            PlayerPitch::default(),
            WalkPhase::default(),
            Transform::from_xyz(0.0, 2.0, 6.0),
            Visibility::default(),
            GameWorld,
            RigidBody::Dynamic,
            Collider::from(Capsule3d::new(0.4, 1.0)),
            LockedAxes::ROTATION_LOCKED,
            LinearVelocity::default(),
        ))
        .with_children(|parent| {
            // ── Camera ────────────────────────────────────────────────────
            parent
                .spawn((
                    PlayerCamera,
                    Camera3d::default(),
                    Projection::from(PerspectiveProjection {
                        fov: 90.0_f32.to_radians(),
                        near: 0.02, // tight near plane so viewmodel arms don't clip
                        ..default()
                    }),
                    Transform::from_xyz(0.0, 0.65, 0.0),
                ))
                .with_children(|cam| {
                    // Viewmodel: left forearm (camera-space, angled slightly downward)
                    cam.spawn((
                        ViewmodelArm,
                        BodySegmentKind::Forearm,
                        BodySide::Left,
                        Mesh3d(meshes.add(Capsule3d::new(0.055, 0.24))),
                        MeshMaterial3d(mat_skin.clone()),
                        Transform {
                            translation: Vec3::new(-0.22, -0.22, -0.38),
                            rotation: Quat::from_rotation_x(-0.35),
                            ..default()
                        },
                        Visibility::Hidden,
                    ));
                    // Viewmodel: left hand
                    cam.spawn((
                        ViewmodelArm,
                        BodySegmentKind::Hand,
                        BodySide::Left,
                        Mesh3d(meshes.add(Cuboid::new(0.08, 0.10, 0.04))),
                        MeshMaterial3d(mat_skin.clone()),
                        Transform::from_xyz(-0.22, -0.32, -0.28),
                        Visibility::Hidden,
                    ));
                    // Viewmodel: right forearm
                    cam.spawn((
                        ViewmodelArm,
                        BodySegmentKind::Forearm,
                        BodySide::Right,
                        Mesh3d(meshes.add(Capsule3d::new(0.055, 0.24))),
                        MeshMaterial3d(mat_skin.clone()),
                        Transform {
                            translation: Vec3::new(0.22, -0.22, -0.38),
                            rotation: Quat::from_rotation_x(-0.35),
                            ..default()
                        },
                        Visibility::Hidden,
                    ));
                    // Viewmodel: right hand
                    cam.spawn((
                        ViewmodelArm,
                        BodySegmentKind::Hand,
                        BodySide::Right,
                        Mesh3d(meshes.add(Cuboid::new(0.08, 0.10, 0.04))),
                        MeshMaterial3d(mat_skin.clone()),
                        Transform::from_xyz(0.22, -0.32, -0.28),
                        Visibility::Hidden,
                    ));
                });

            // ── Full body segments ─────────────────────────────────────────
            // Physics capsule: Capsule3d::new(0.4, 1.0) — total height 1.8m,
            // bottom at y = -0.9 from entity origin.

            // Head
            parent.spawn((
                BodySegment,
                BodySegmentKind::Head,
                BodySide::Center,
                Mesh3d(meshes.add(Sphere::new(0.13))),
                MeshMaterial3d(mat_skin.clone()),
                Transform::from_xyz(0.0, 0.76, 0.0),
            ));
            // Neck
            parent.spawn((
                BodySegment,
                BodySegmentKind::Neck,
                BodySide::Center,
                Mesh3d(meshes.add(Capsule3d::new(0.06, 0.08))),
                MeshMaterial3d(mat_skin.clone()),
                Transform::from_xyz(0.0, 0.59, 0.0),
            ));
            // Torso
            parent.spawn((
                BodySegment,
                BodySegmentKind::Torso,
                BodySide::Center,
                Mesh3d(meshes.add(Cuboid::new(0.42, 0.52, 0.22))),
                MeshMaterial3d(mat_shirt.clone()),
                Transform::from_xyz(0.0, 0.22, 0.0),
            ));
            // Left clavicle (horizontal bar connecting torso to shoulder)
            parent.spawn((
                BodySegment,
                BodySegmentKind::Clavicle,
                BodySide::Left,
                Mesh3d(meshes.add(Capsule3d::new(0.055, 0.16))),
                MeshMaterial3d(mat_shirt.clone()),
                Transform {
                    translation: Vec3::new(-0.28, 0.42, 0.0),
                    rotation: Quat::from_rotation_z(FRAC_PI_2),
                    ..default()
                },
            ));
            // Right clavicle
            parent.spawn((
                BodySegment,
                BodySegmentKind::Clavicle,
                BodySide::Right,
                Mesh3d(meshes.add(Capsule3d::new(0.055, 0.16))),
                MeshMaterial3d(mat_shirt.clone()),
                Transform {
                    translation: Vec3::new(0.28, 0.42, 0.0),
                    rotation: Quat::from_rotation_z(FRAC_PI_2),
                    ..default()
                },
            ));
            // Left upper arm
            parent.spawn((
                BodySegment,
                BodySegmentKind::UpperArm,
                BodySide::Left,
                Mesh3d(meshes.add(Capsule3d::new(0.07, 0.28))),
                MeshMaterial3d(mat_shirt.clone()),
                Transform::from_xyz(-0.37, 0.20, 0.0),
            ));
            // Right upper arm
            parent.spawn((
                BodySegment,
                BodySegmentKind::UpperArm,
                BodySide::Right,
                Mesh3d(meshes.add(Capsule3d::new(0.07, 0.28))),
                MeshMaterial3d(mat_shirt.clone()),
                Transform::from_xyz(0.37, 0.20, 0.0),
            ));
            // Left forearm (skin — short sleeve shirt)
            parent.spawn((
                BodySegment,
                BodySegmentKind::Forearm,
                BodySide::Left,
                Mesh3d(meshes.add(Capsule3d::new(0.055, 0.24))),
                MeshMaterial3d(mat_skin.clone()),
                Transform::from_xyz(-0.37, -0.10, 0.0),
            ));
            // Right forearm
            parent.spawn((
                BodySegment,
                BodySegmentKind::Forearm,
                BodySide::Right,
                Mesh3d(meshes.add(Capsule3d::new(0.055, 0.24))),
                MeshMaterial3d(mat_skin.clone()),
                Transform::from_xyz(0.37, -0.10, 0.0),
            ));
            // Left hand
            parent.spawn((
                BodySegment,
                BodySegmentKind::Hand,
                BodySide::Left,
                Mesh3d(meshes.add(Cuboid::new(0.08, 0.10, 0.04))),
                MeshMaterial3d(mat_skin.clone()),
                Transform::from_xyz(-0.37, -0.30, 0.0),
            ));
            // Right hand
            parent.spawn((
                BodySegment,
                BodySegmentKind::Hand,
                BodySide::Right,
                Mesh3d(meshes.add(Cuboid::new(0.08, 0.10, 0.04))),
                MeshMaterial3d(mat_skin.clone()),
                Transform::from_xyz(0.37, -0.30, 0.0),
            ));
            // Left upper leg
            parent.spawn((
                BodySegment,
                BodySegmentKind::UpperLeg,
                BodySide::Left,
                Mesh3d(meshes.add(Capsule3d::new(0.09, 0.30))),
                MeshMaterial3d(mat_pants.clone()),
                Transform::from_xyz(-0.13, -0.46, 0.0),
            ));
            // Right upper leg
            parent.spawn((
                BodySegment,
                BodySegmentKind::UpperLeg,
                BodySide::Right,
                Mesh3d(meshes.add(Capsule3d::new(0.09, 0.30))),
                MeshMaterial3d(mat_pants.clone()),
                Transform::from_xyz(0.13, -0.46, 0.0),
            ));
            // Left lower leg
            parent.spawn((
                BodySegment,
                BodySegmentKind::LowerLeg,
                BodySide::Left,
                Mesh3d(meshes.add(Capsule3d::new(0.075, 0.28))),
                MeshMaterial3d(mat_pants.clone()),
                Transform::from_xyz(-0.13, -0.80, 0.0),
            ));
            // Right lower leg
            parent.spawn((
                BodySegment,
                BodySegmentKind::LowerLeg,
                BodySide::Right,
                Mesh3d(meshes.add(Capsule3d::new(0.075, 0.28))),
                MeshMaterial3d(mat_pants.clone()),
                Transform::from_xyz(0.13, -0.80, 0.0),
            ));
            // Left foot
            parent.spawn((
                BodySegment,
                BodySegmentKind::Foot,
                BodySide::Left,
                Mesh3d(meshes.add(Cuboid::new(0.09, 0.06, 0.18))),
                MeshMaterial3d(mat_pants.clone()),
                Transform::from_xyz(-0.13, -0.95, -0.04),
            ));
            // Right foot
            parent.spawn((
                BodySegment,
                BodySegmentKind::Foot,
                BodySide::Right,
                Mesh3d(meshes.add(Cuboid::new(0.09, 0.06, 0.18))),
                MeshMaterial3d(mat_pants.clone()),
                Transform::from_xyz(0.13, -0.95, -0.04),
            ));
        });

    // Crosshair – small white square centred on screen
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            width: Val::Px(8.0),
            height: Val::Px(8.0),
            left: Val::Percent(50.0),
            top: Val::Percent(50.0),
            margin: UiRect {
                left: Val::Px(-4.0),
                top: Val::Px(-4.0),
                ..default()
            },
            ..default()
        },
        BackgroundColor(Color::WHITE),
        GameWorld,
    ));

    // HUD hint
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(12.0),
                left: Val::Px(12.0),
                ..default()
            },
            GameWorld,
        ))
        .with_child((
            Text::new(
                "WASD: move  |  Space: jump  |  Mouse: look  |  LMB: shoot  |  V: camera  |  Esc: menu",
            ),
            TextFont {
                font_size: 18.0,
                ..default()
            },
            TextColor(Color::srgba(1.0, 1.0, 1.0, 0.7)),
        ));

    // Floor – static physics body so the player lands on it
    let floor_mesh = meshes.add(Plane3d::new(Vec3::Y, Vec2::splat(20.0)));
    let floor_mat = materials.add(Color::from(tailwind::GRAY_600));
    commands.spawn((
        Mesh3d(floor_mesh),
        MeshMaterial3d(floor_mat),
        GameWorld,
        RigidBody::Static,
        Collider::cuboid(40.0, 0.1, 40.0),
        Transform::from_xyz(0.0, -0.05, 0.0),
    ));

    // Target – red sphere the player must hit
    let target_mesh = meshes.add(Sphere::new(0.5));
    let target_mat = materials.add(StandardMaterial {
        base_color: Color::from(tailwind::RED_500),
        emissive: LinearRgba::from(Color::from(tailwind::RED_700)) * 0.4,
        ..default()
    });
    commands.spawn((
        Target,
        Mesh3d(target_mesh),
        MeshMaterial3d(target_mat),
        Transform::from_xyz(0.0, 1.0, -5.0),
        GameWorld,
    ));

    // Directional light
    commands.spawn((
        DirectionalLight {
            shadows_enabled: true,
            illuminance: 10_000.0,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::YXZ, -0.8, -0.8, 0.0)),
        GameWorld,
    ));

    // Ambient light
    commands.spawn((
        AmbientLight {
            color: Color::WHITE,
            brightness: 200.0,
            ..default()
        },
        GameWorld,
    ));
}

// ── Cursor ─────────────────────────────────────────────────────────────────

fn grab_cursor(mut cursor_options: Single<&mut CursorOptions>) {
    cursor_options.grab_mode = CursorGrabMode::Locked;
    cursor_options.visible = false;
}

fn release_cursor(mut cursor_options: Single<&mut CursorOptions>) {
    cursor_options.grab_mode = CursorGrabMode::None;
    cursor_options.visible = true;
}

// ── Cleanup ─────────────────────────────────────────────────────────────────

fn cleanup_game(mut commands: Commands, query: Query<Entity, With<GameWorld>>) {
    for entity in &query {
        commands.entity(entity).despawn();
    }
}

// ── Ground detection ───────────────────────────────────────────────────────

fn check_grounded(
    mut commands: Commands,
    spatial_query: SpatialQuery,
    player: Query<(Entity, &Position), With<Player>>,
) {
    let Ok((entity, pos)) = player.single() else {
        return;
    };

    let mut filter = SpatialQueryFilter::default();
    filter.excluded_entities.insert(entity);

    let hit = spatial_query.cast_ray(pos.0, Dir3::NEG_Y, 1.0, true, &filter);
    if hit.is_some() {
        commands.entity(entity).insert(Grounded);
    } else {
        commands.entity(entity).remove::<Grounded>();
    }
}

// ── Player movement ────────────────────────────────────────────────────────

fn move_player(
    accumulated_mouse_motion: Res<AccumulatedMouseMotion>,
    input: Res<ButtonInput<KeyCode>>,
    player: Single<
        (
            &mut Rotation,
            &CameraSensitivity,
            &mut LinearVelocity,
            Option<&Grounded>,
            &mut PlayerPitch,
        ),
        With<Player>,
    >,
) {
    let (mut rotation, sensitivity, mut velocity, grounded, mut player_pitch) =
        player.into_inner();

    let delta = accumulated_mouse_motion.delta;

    // Yaw: applied to the physics Rotation on the player entity
    let (current_yaw, _, _) = rotation.to_euler(EulerRot::YXZ);
    let new_yaw = if delta != Vec2::ZERO {
        current_yaw + -delta.x * sensitivity.x
    } else {
        current_yaw
    };

    if new_yaw != current_yaw {
        *rotation = Rotation(Quat::from_rotation_y(new_yaw));
    }

    // Pitch: stored in component, applied to camera by update_camera
    if delta != Vec2::ZERO {
        const PITCH_LIMIT: f32 = FRAC_PI_2 - 0.01;
        player_pitch.0 =
            (player_pitch.0 + -delta.y * sensitivity.y).clamp(-PITCH_LIMIT, PITCH_LIMIT);
    }

    // Derive horizontal directions from yaw (ignore vertical tilt)
    let s = new_yaw.sin();
    let c = new_yaw.cos();
    let forward = Vec3::new(-s, 0.0, -c);
    let right = Vec3::new(c, 0.0, -s);

    // WASD horizontal velocity (preserve Y for gravity)
    let mut dir = Vec3::ZERO;
    if input.pressed(KeyCode::KeyW) {
        dir += forward;
    }
    if input.pressed(KeyCode::KeyS) {
        dir -= forward;
    }
    if input.pressed(KeyCode::KeyA) {
        dir -= right;
    }
    if input.pressed(KeyCode::KeyD) {
        dir += right;
    }

    let speed = 5.0;
    if dir.length_squared() > 0.0 {
        let d = dir.normalize();
        velocity.x = d.x * speed;
        velocity.z = d.z * speed;
    } else {
        velocity.x = 0.0;
        velocity.z = 0.0;
    }

    // Jump
    if input.just_pressed(KeyCode::Space) && grounded.is_some() {
        velocity.y = 6.0;
    }
}

// ── Camera mode toggle ──────────────────────────────────────────────────────

fn toggle_camera_mode(input: Res<ButtonInput<KeyCode>>, mut mode: ResMut<CameraMode>) {
    if input.just_pressed(KeyCode::KeyV) {
        *mode = match *mode {
            CameraMode::FirstPerson => CameraMode::ThirdPerson,
            CameraMode::ThirdPerson => CameraMode::FirstPerson,
        };
    }
}

// ── Camera update ───────────────────────────────────────────────────────────

fn update_camera(
    mode: Res<CameraMode>,
    player: Single<&PlayerPitch, With<Player>>,
    mut cam_query: Query<&mut Transform, With<PlayerCamera>>,
) {
    let pitch = player.0;
    let Ok(mut cam_tf) = cam_query.single_mut() else {
        return;
    };

    match *mode {
        CameraMode::FirstPerson => {
            cam_tf.translation = Vec3::new(0.0, 0.65, 0.0);
            cam_tf.rotation = Quat::from_rotation_x(pitch);
        }
        CameraMode::ThirdPerson => {
            // Orbit behind and above the player; vertical angle follows pitch
            let dist = 4.5_f32;
            let orbit_angle = pitch + 0.3; // slight upward default
            let y = 0.5 + dist * orbit_angle.sin().max(0.0);
            let z = dist * orbit_angle.cos().max(0.1);
            let pos = Vec3::new(0.0, y, z);
            *cam_tf = Transform::from_translation(pos)
                .looking_at(Vec3::new(0.0, 0.5, 0.0), Vec3::Y);
        }
    }
}

// ── Body visibility (FP/TP toggle) ─────────────────────────────────────────

fn update_body_visibility(
    mode: Res<CameraMode>,
    mut body_query: Query<&mut Visibility, With<BodySegment>>,
    mut viewmodel_query: Query<&mut Visibility, With<ViewmodelArm>>,
) {
    let (body_vis, viewmodel_vis) = match *mode {
        CameraMode::FirstPerson => (Visibility::Hidden, Visibility::Inherited),
        CameraMode::ThirdPerson => (Visibility::Inherited, Visibility::Hidden),
    };
    for mut vis in &mut body_query {
        *vis = body_vis;
    }
    for mut vis in &mut viewmodel_query {
        *vis = viewmodel_vis;
    }
}

// ── Head pitch tracking (third-person only) ─────────────────────────────────

fn update_head_pitch(
    mode: Res<CameraMode>,
    player: Single<&PlayerPitch, With<Player>>,
    mut segments: Query<(&mut Transform, &BodySegmentKind), With<BodySegment>>,
) {
    let pitch = player.0;
    for (mut tf, kind) in &mut segments {
        match kind {
            BodySegmentKind::Neck => {
                tf.rotation = if *mode == CameraMode::ThirdPerson {
                    Quat::from_rotation_x(pitch * 0.4)
                } else {
                    Quat::IDENTITY
                };
            }
            BodySegmentKind::Head => {
                tf.rotation = if *mode == CameraMode::ThirdPerson {
                    Quat::from_rotation_x(pitch * 0.6)
                } else {
                    Quat::IDENTITY
                };
            }
            _ => {}
        }
    }
}

// ── Procedural walk animation ────────────────────────────────────────────────

fn animate_walk(
    time: Res<Time>,
    player_query: Single<(&LinearVelocity, &mut WalkPhase), With<Player>>,
    mut segments: Query<(&mut Transform, &BodySegmentKind, &BodySide), With<BodySegment>>,
) {
    #[allow(unused_mut)]
    let (velocity, mut walk_phase) = player_query.into_inner();
    let speed_xz = Vec2::new(velocity.x, velocity.z).length();
    let dt = time.delta_secs();

    if speed_xz > 0.1 {
        // Advance phase proportional to horizontal speed
        walk_phase.0 += speed_xz * dt * 3.5;
    } else {
        // Smoothly decay phase back to 0 so limbs return to neutral
        walk_phase.0 = walk_phase.0 * (1.0 - dt * 8.0);
        if walk_phase.0.abs() < 0.001 {
            walk_phase.0 = 0.0;
        }
    }

    let phase = walk_phase.0;

    for (mut tf, kind, side) in &mut segments {
        match kind {
            BodySegmentKind::UpperArm => {
                let swing = match side {
                    BodySide::Left => phase.sin() * 0.45,
                    BodySide::Right => -phase.sin() * 0.45,
                    BodySide::Center => 0.0,
                };
                tf.rotation = Quat::from_rotation_x(swing);
            }
            BodySegmentKind::UpperLeg => {
                let swing = match side {
                    BodySide::Left => -phase.sin() * 0.5,
                    BodySide::Right => phase.sin() * 0.5,
                    BodySide::Center => 0.0,
                };
                tf.rotation = Quat::from_rotation_x(swing);
            }
            BodySegmentKind::LowerLeg => {
                // Knee bends symmetrically: always slightly bent during motion,
                // more on the back-swing. abs() gives a natural gait.
                let bend = (phase + 0.6).sin().abs() * 0.3;
                tf.rotation = Quat::from_rotation_x(bend);
            }
            _ => {}
        }
    }
}

// ── Shooting ───────────────────────────────────────────────────────────────

fn shoot(
    mouse_input: Res<ButtonInput<MouseButton>>,
    camera_query: Query<&GlobalTransform, With<PlayerCamera>>,
    target_query: Query<(Entity, &Transform), With<Target>>,
    mut commands: Commands,
    mut next_state: ResMut<NextState<AppState>>,
) {
    if !mouse_input.just_pressed(MouseButton::Left) {
        return;
    }

    let Ok(camera_gtf) = camera_query.single() else {
        return;
    };

    let ray_origin = camera_gtf.translation();
    let ray_dir = *camera_gtf.forward();

    for (target_entity, target_tf) in &target_query {
        let target_radius = 0.5_f32;
        let oc = target_tf.translation - ray_origin;
        let t = oc.dot(ray_dir);
        if t <= 0.0 {
            continue;
        }
        let closest_pt = ray_origin + ray_dir * t;
        let dist = (closest_pt - target_tf.translation).length();
        if dist <= target_radius {
            commands.entity(target_entity).despawn();
            next_state.set(AppState::YouWin);
            return;
        }
    }
}

// ── Escape ─────────────────────────────────────────────────────────────────

fn handle_escape(
    input: Res<ButtonInput<KeyCode>>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    if input.just_pressed(KeyCode::Escape) {
        next_state.set(AppState::MainMenu);
    }
}
