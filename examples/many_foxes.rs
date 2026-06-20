use std::f32::consts::PI;

use argh::FromArgs;
use bevy::{
    diagnostic::{FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin},
    light::CascadeShadowConfigBuilder,
    prelude::*,
    window::{PresentMode, WindowResolution},
    winit::WinitSettings,
};
use bevy_open_vat::prelude::*;

#[derive(FromArgs, Resource)]
/// `many_foxes` stress test
struct Args {
    /// whether all foxes run in sync.
    #[argh(switch)]
    sync: bool,

    /// total number of foxes.
    #[argh(option, default = "1000")]
    count: usize,
}

#[derive(Resource)]
struct Foxes {
    count: usize,
    speed: f32,
    moving: bool,
}

fn main() {
    // `from_env` panics on the web
    #[cfg(not(target_arch = "wasm32"))]
    let args: Args = argh::from_env();
    #[cfg(target_arch = "wasm32")]
    let args = Args::from_args(&[], &[]).unwrap();

    App::new()
        .add_plugins((
            DefaultPlugins.set(WindowPlugin {
                primary_window: Some(Window {
                    title: "🦊🦊🦊 Many Foxes! 🦊🦊🦊".into(),
                    present_mode: if args.sync {
                        PresentMode::AutoVsync
                    } else {
                        PresentMode::AutoNoVsync
                    },
                    resolution: WindowResolution::new(1920, 1080).with_scale_factor_override(1.0),
                    ..default()
                }),
                ..default()
            }),
            OpenVatPlugin,
            FrameTimeDiagnosticsPlugin::default(),
            LogDiagnosticsPlugin::default(),
        ))
        .insert_resource(StaticTransformOptimizations::Disabled)
        .insert_resource(WinitSettings::continuous())
        .insert_resource(Foxes {
            count: args.count,
            speed: 2.0,
            moving: true,
        })
        .insert_resource(args)
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                keyboard_animation_control,
                update_fox_rings.after(keyboard_animation_control),
            ),
        )
        .run();
}

#[derive(Resource)]
struct Animations {
    clips: Vec<Handle<VatAnimationClip>>,
}

const RING_SPACING: f32 = 2.0;
const FOX_SPACING: f32 = 2.0;

#[derive(Component, Clone, Copy)]
enum RotationDirection {
    CounterClockwise,
    Clockwise,
}

impl RotationDirection {
    fn sign(&self) -> f32 {
        match self {
            RotationDirection::CounterClockwise => 1.0,
            RotationDirection::Clockwise => -1.0,
        }
    }
}

#[derive(Component)]
struct PlaneMarker;

#[derive(Component)]
struct Ring {
    radius: f32,
}

fn setup(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    foxes: Res<Foxes>,
    args: Res<Args>,
) {
    let remap_info = asset_server.load("models/vat/Fox-remap_info.json");
    let vat_texture = asset_server.load("models/vat/Fox_vat.exr");

    let clips = vec![
        asset_server.load("models/vat/Fox-remap_info.json#Run"),
        asset_server.load("models/vat/Fox-remap_info.json#Walk"),
        asset_server.load("models/vat/Fox-remap_info.json#Survey"),
    ];

    commands.insert_resource(Animations {
        clips: clips.clone(),
    });

    let fox_handle = asset_server.load(GltfAssetLabel::Scene(0).from_asset("models/vat/Fox.glb"));

    let ring_directions = [
        (
            Quat::from_rotation_y(PI),
            RotationDirection::CounterClockwise,
        ),
        (Quat::IDENTITY, RotationDirection::Clockwise),
    ];

    let mut ring_index = 0;
    let mut radius = RING_SPACING;
    let mut foxes_remaining = foxes.count;

    info!("Spawning {} foxes...", foxes.count);

    while foxes_remaining > 0 {
        let (base_rotation, ring_direction) = ring_directions[ring_index % 2];
        let ring_parent = commands
            .spawn((
                Transform::default(),
                Visibility::default(),
                ring_direction,
                Ring { radius },
            ))
            .id();

        let circumference = PI * 2. * radius;
        let foxes_in_ring = ((circumference / FOX_SPACING) as usize).min(foxes_remaining);
        let fox_spacing_angle = circumference / (foxes_in_ring as f32 * radius);

        for fox_i in 0..foxes_in_ring {
            let fox_angle = fox_i as f32 * fox_spacing_angle;
            let (s, c) = ops::sin_cos(fox_angle);
            let (x, z) = (radius * c, radius * s);

            commands.entity(ring_parent).with_children(|builder| {
                builder.spawn((
                    WorldAssetRoot(fox_handle.clone()),
                    Transform::from_xyz(x, 0.0, z)
                        .with_scale(Vec3::splat(0.01))
                        .with_rotation(base_rotation * Quat::from_rotation_y(-fox_angle)),
                    VatAnimator {
                        remap_info: remap_info.clone(),
                        vat_texture: vat_texture.clone(),
                        current_clip: clips[0].clone(),
                        speed: foxes.speed,
                        is_playing: foxes.moving,
                        start_time: if args.sync { 0.0 } else { fox_i as f32 * 0.1 },
                        offset: 0.0,
                    },
                ));
            });
        }

        foxes_remaining -= foxes_in_ring;
        radius += RING_SPACING;
        ring_index += 1;
    }

    // Camera
    let zoom = 0.8;
    let translation = Vec3::new(
        radius * 1.25 * zoom,
        radius * 0.5 * zoom,
        radius * 1.5 * zoom,
    );
    commands.spawn((
        Camera3d::default(),
        Transform::from_translation(translation)
            .looking_at(0.2 * Vec3::new(translation.x, 0.0, translation.z), Vec3::Y),
    ));

    // Plane
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(5000.0, 5000.0))),
        MeshMaterial3d(materials.add(Color::srgb(0.3, 0.5, 0.3))),
        PlaneMarker,
    ));

    // Light
    commands.spawn((
        Transform::from_rotation(Quat::from_euler(EulerRot::ZYX, 0.0, 1.0, -PI / 4.)),
        DirectionalLight {
            shadow_maps_enabled: true,
            ..default()
        },
        CascadeShadowConfigBuilder {
            first_cascade_far_bound: 0.9 * radius,
            maximum_distance: 2.8 * radius,
            ..default()
        }
        .build(),
    ));

    println!("Animation controls:");
    println!("  - spacebar: play / pause");
    println!("  - arrow up / down: speed up / slow down animation playback");
    println!("  - arrow left / right: seek backward / forward");
    println!("  - return: change animation");
}

fn update_fox_rings(
    time: Res<Time>,
    foxes: Res<Foxes>,
    mut rings: Query<(&Ring, &RotationDirection, &mut Transform)>,
) {
    if !foxes.moving {
        return;
    }

    let dt = time.delta_secs();
    for (ring, rotation_direction, mut transform) in &mut rings {
        let angular_velocity = foxes.speed / ring.radius;
        transform.rotate_y(rotation_direction.sign() * angular_velocity * dt);
    }
}

fn keyboard_animation_control(
    keyboard_input: Res<ButtonInput<KeyCode>>,
    mut animation_player: Query<&mut VatAnimator>,
    animations: Res<Animations>,
    clips: Res<Assets<VatAnimationClip>>,
    mut current_animation: Local<usize>,
    mut foxes: ResMut<Foxes>,
) {
    if keyboard_input.just_pressed(KeyCode::Space) {
        foxes.moving = !foxes.moving;
    }

    if keyboard_input.just_pressed(KeyCode::ArrowUp) {
        foxes.speed *= 1.25;
    }

    if keyboard_input.just_pressed(KeyCode::ArrowDown) {
        foxes.speed *= 0.8;
    }

    if keyboard_input.just_pressed(KeyCode::Enter) {
        *current_animation = (*current_animation + 1) % animations.clips.len();
        let new_clip = animations.clips[*current_animation].clone();
        for mut controller in &mut animation_player {
            controller.current_clip = new_clip.clone();
        }
    }

    for mut controller in &mut animation_player {
        if keyboard_input.just_pressed(KeyCode::Space) {
            controller.is_playing = !controller.is_playing;
        }

        if keyboard_input.just_pressed(KeyCode::ArrowUp) {
            controller.speed *= 1.25;
        }

        if keyboard_input.just_pressed(KeyCode::ArrowDown) {
            controller.speed *= 0.8;
        }

        // Seek backward
        if keyboard_input.just_pressed(KeyCode::ArrowLeft) {
            if let Some(clip) = clips.get(&controller.current_clip) {
                let duration = clip.duration().unwrap_or(0.0);
                let diff = controller.start_time - 0.1;
                if diff < 0.0 {
                    controller.start_time = duration + diff;
                } else {
                    controller.start_time = diff;
                }
            }
        }

        // Seek forward
        if keyboard_input.just_pressed(KeyCode::ArrowRight) {
            if let Some(clip) = clips.get(&controller.current_clip) {
                let duration = clip.duration().unwrap_or(0.0);
                let diff = controller.start_time + 0.1;
                if diff > duration {
                    controller.start_time = diff - duration;
                } else {
                    controller.start_time = diff;
                }
            }
        }
    }
}
