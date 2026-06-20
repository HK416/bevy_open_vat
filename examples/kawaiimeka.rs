use bevy::prelude::*;
use bevy_open_vat::prelude::*;

const NUM_WIDTH: usize = 50;
const NUM_DEPTH: usize = 50;
const SPACING: f32 = 5.0;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(OpenVatPlugin)
        .add_systems(Startup, (setup, setup_camera_instructions))
        .add_systems(Update, camera_movement)
        .run();
}

fn setup(mut commands: Commands, asset_server: Res<AssetServer>) {
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(200.0, 75.0, 200.0).looking_at(Vec3::new(30.0, 1.0, 30.0), Vec3::Y),
        Msaa::Off,
    ));

    commands.spawn((
        DirectionalLight {
            shadow_maps_enabled: true,
            ..Default::default()
        },
        Transform::from_xyz(4.0, 8.0, 3.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    let scene_handle =
        asset_server.load(GltfAssetLabel::Scene(0).from_asset("models/KawaiiMeka/KawaiiMeka.glb"));

    // Specify the JSON and EXR to load
    let remap_handle: Handle<RemapInfo> =
        asset_server.load("models/KawaiiMeka/KawaiiMeka-remap_info.json");
    let vat_handle: Handle<Image> = asset_server.load("models/KawaiiMeka/Collection_vat.exr");
    // Safely load the "Idle" clip as a sub-asset
    let idle_clip: Handle<VatAnimationClip> =
        asset_server.load("models/KawaiiMeka/KawaiiMeka-remap_info.json#Idle");

    for x in 0..NUM_WIDTH {
        for z in 0..NUM_DEPTH {
            commands.spawn((
                WorldAssetRoot(scene_handle.clone()),
                Transform::from_xyz(
                    x as f32 * SPACING - (NUM_WIDTH as f32 / 2.0) * SPACING,
                    0.0,
                    z as f32 * SPACING - (NUM_DEPTH as f32 / 2.0) * SPACING,
                ),
                // Complete complex render pipeline configuration by adding just one component!
                VatAnimator {
                    remap_info: remap_handle.clone(),
                    vat_texture: vat_handle.clone(),
                    current_clip: idle_clip.clone(),
                    speed: 1.0,
                    is_playing: true,
                    offset: 0.0,
                    start_time: 0.0,
                },
            ));
        }
    }
}

fn setup_camera_instructions(mut commands: Commands) {
    commands.spawn((
        Text::new(
            "[Camera Controls]\n\
            - W/A/S/D : Move Forward/Left/Backward/Right\n\
            - Q / E   : Move Up / Down (Vertical)\n\
            - Arrow Keys : Rotate Camera View\n\
            - Shift   : Boost Movement Speed",
        ),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(15.0),
            left: Val::Px(15.0),
            ..default()
        },
    ));
}

fn camera_movement(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mut query: Query<&mut Transform, With<Camera3d>>,
) {
    let Ok(mut transform) = query.single_mut() else {
        return;
    };

    let mut direction = Vec3::ZERO;
    let forward = transform.rotation * Vec3::NEG_Z;
    let right = transform.rotation * Vec3::X;
    let up = Vec3::Y;

    if keys.pressed(KeyCode::KeyW) {
        direction += forward;
    }
    if keys.pressed(KeyCode::KeyS) {
        direction -= forward;
    }
    if keys.pressed(KeyCode::KeyD) {
        direction += right;
    }
    if keys.pressed(KeyCode::KeyA) {
        direction -= right;
    }
    if keys.pressed(KeyCode::KeyE) {
        direction += up;
    }
    if keys.pressed(KeyCode::KeyQ) {
        direction -= up;
    }

    let speed = if keys.pressed(KeyCode::ShiftLeft) {
        50.0
    } else {
        25.0
    };

    if direction != Vec3::ZERO {
        transform.translation += direction.normalize() * speed * time.delta_secs();
    }

    let rotation_speed = 1.5;
    if keys.pressed(KeyCode::ArrowLeft) {
        transform.rotate_y(rotation_speed * time.delta_secs());
    }
    if keys.pressed(KeyCode::ArrowRight) {
        transform.rotate_y(-rotation_speed * time.delta_secs());
    }
    if keys.pressed(KeyCode::ArrowUp) {
        transform.rotate_local_x(rotation_speed * time.delta_secs());
    }
    if keys.pressed(KeyCode::ArrowDown) {
        transform.rotate_local_x(-rotation_speed * time.delta_secs());
    }
}
