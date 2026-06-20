use std::f32::consts::PI;

use argh::FromArgs;
use bevy::{
    diagnostic::{FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin},
    light::CascadeShadowConfigBuilder,
    pbr::ExtendedMaterial,
    platform::collections::HashMap,
    prelude::*,
    render::storage::ShaderBuffer,
    window::{PresentMode, WindowResolution},
    winit::WinitSettings,
};
use bevy_open_vat::{data::VatInstanceData, prelude::*};

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
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                insert_extended_materials,
                keyboard_animation_control,
                update_fox_rings.after(keyboard_animation_control),
            ),
        )
        .run();
}

#[derive(Resource)]
struct Animations {
    remap_info: Handle<RemapInfo>,
    vat_texture: Handle<Image>,
    keys: Vec<String>,
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
) {
    // Insert a resource with the current scene information
    let remap_info = asset_server.load("models/vat/Fox-remap_info.json");
    let vat_texture = asset_server.load("models/vat/Fox_vat.exr");
    commands.insert_resource(Animations {
        remap_info,
        vat_texture,
        keys: Vec::new(),
    });

    // Foxes
    // Concentric rings of foxes, running in opposite directions. The rings are spaced at 2m radius intervals.
    // The foxes in each ring are spaced at least 2m apart around its circumference.'

    // NOTE: This fox model faces +z
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

    // Spawn concentric rings of foxes until we reach the total count.
    while foxes_remaining > 0 {
        let (base_rotation, ring_direction) = ring_directions[ring_index % 2];
        // Create a parent entity for the ring to simplify rotation logic.
        let ring_parent = commands
            .spawn((
                Transform::default(),
                Visibility::default(),
                ring_direction,
                Ring { radius },
            ))
            .id();

        let circumference = PI * 2. * radius;
        // Calculate how many foxes fit in this ring with the desired spacing.
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

fn insert_extended_materials(
    mut commands: Commands,
    foxes: Res<Foxes>,
    mut animations: ResMut<Animations>,
    images: Res<Assets<Image>>,
    remap_infos: Res<Assets<RemapInfo>>,
    std_materials: Res<Assets<StandardMaterial>>,
    mut vat_materials: ResMut<Assets<ExtendedMaterial<StandardMaterial, OpenVatExtension>>>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
    mut players: Query<(Entity, &MeshMaterial3d<StandardMaterial>), Without<PlaneMarker>>,
) {
    let entities: Vec<_> = players.iter_mut().collect();
    if entities.len() < foxes.count {
        return;
    }

    let Some(remap_info) = remap_infos.get(&animations.remap_info) else {
        return;
    };
    animations.keys = remap_info.animations.keys().cloned().collect();

    let vat_texture = &animations.vat_texture;
    let y_resolution = if let Some(image) = images.get(vat_texture) {
        image.texture_descriptor.size.height as f32
    } else {
        return;
    };

    let instance_data_vec: Vec<VatInstanceData> = Vec::with_capacity(entities.len());
    let buffer_handle = buffers.add(ShaderBuffer::from(&instance_data_vec));

    let mut material_cache: HashMap<
        _,
        Handle<ExtendedMaterial<StandardMaterial, OpenVatExtension>>,
    > = HashMap::default();
    for (entity, old_mat) in entities.into_iter() {
        let Some(std_material) = std_materials.get(&old_mat.0) else {
            continue;
        };

        match material_cache.get(&old_mat.0) {
            Some(material) => {
                commands
                    .entity(entity)
                    .remove::<MeshMaterial3d<StandardMaterial>>();
                commands.entity(entity).insert((
                    MeshMaterial3d(material.clone()),
                    VatAnimationController {
                        remap_info: animations.remap_info.clone(),
                        current_clip: animations.keys[0].clone(),
                        ..Default::default()
                    },
                ));
            }
            None => {
                let extended_material = ExtendedMaterial {
                    base: StandardMaterial {
                        // To prevent bind groups from being deleted in Prepass.
                        alpha_mode: AlphaMode::Mask(0.0),
                        ..std_material.clone()
                    },
                    extension: OpenVatExtension {
                        vat_texture: vat_texture.clone(),
                        min_pos: remap_info.os_remap.min.into(),
                        frame_count: remap_info.os_remap.frames,
                        max_pos: remap_info.os_remap.max.into(),
                        y_resolution,
                        instance: buffer_handle.clone(),
                        ..Default::default()
                    },
                };

                let material = vat_materials.add(extended_material);
                commands
                    .entity(entity)
                    .remove::<MeshMaterial3d<StandardMaterial>>();
                commands.entity(entity).insert((
                    MeshMaterial3d(material.clone()),
                    VatAnimationController {
                        remap_info: animations.remap_info.clone(),
                        current_clip: animations.keys[0].clone(),
                        ..Default::default()
                    },
                ));

                material_cache.insert(old_mat.0.clone(), material);
            }
        }
    }
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
    mut animation_player: Query<&mut VatAnimationController>,
    animations: Res<Animations>,
    remap_infos: Res<Assets<RemapInfo>>,
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
        *current_animation = (*current_animation + 1) % animations.keys.len();
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
            if let Some(remap) = remap_infos.get(&controller.remap_info) {
                if let Some(clip) = remap.animations.get(&controller.current_clip) {
                    let duration = clip.duration().unwrap_or(0.0);
                    let diff = controller.start_time - 0.1;
                    if diff < 0.0 {
                        controller.start_time = duration + diff;
                    } else {
                        controller.start_time = diff;
                    }
                }
            }
        }

        // Seek forward
        if keyboard_input.just_pressed(KeyCode::ArrowRight) {
            if let Some(remap) = remap_infos.get(&controller.remap_info) {
                if let Some(clip) = remap.animations.get(&controller.current_clip) {
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

        // Change Animation Clip
        if keyboard_input.just_pressed(KeyCode::Enter) {
            controller.current_clip = animations.keys[*current_animation].clone();
        }
    }
}
