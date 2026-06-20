use bevy::{
    asset::RenderAssetUsages,
    mesh::VertexAttributeValues,
    platform::collections::HashMap,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use bevy_open_vat::prelude::*;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(OpenVatPlugin)
        .add_systems(Startup, (setup, setup_camera_instructions))
        .add_systems(Update, camera_movement)
        .run();
}

/// Sets up a scene with a procedurally generated VAT animation.
fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut remap_infos: ResMut<Assets<RemapInfo>>,
    mut clips: ResMut<Assets<VatAnimationClip>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    // Create a simple plane mesh
    let mut plane_mesh = Mesh::from(Plane3d::default().mesh().size(1.0, 1.0));

    // Assign unique UV1 coordinates to each vertex.
    // In VAT, UV1 is often used to index into the VAT texture's X-axis (vertex index).
    let vertex_count = 4;
    let frame_count = 60;

    let y_uv = 0.5 / (frame_count * 2) as f32;
    plane_mesh.insert_attribute(
        Mesh::ATTRIBUTE_UV_1,
        VertexAttributeValues::Float32x2(vec![
            [(0.0 + 0.5) / 4.0, y_uv],
            [(1.0 + 0.5) / 4.0, y_uv],
            [(2.0 + 0.5) / 4.0, y_uv],
            [(3.0 + 0.5) / 4.0, y_uv],
        ]),
    );

    let mesh_handle = meshes.add(plane_mesh);

    // Create a procedural texture that makes vertices "bounce"
    let vat_texture = create_bounce_texture(vertex_count, frame_count);
    let vat_texture_handle = images.add(vat_texture);

    // Manually create RemapInfo for the procedural animation.
    let remap_info = remap_infos.add(RemapInfo {
        os_remap: OsRemap {
            min: [0.0, 0.0, 0.0],
            max: [1.0, 1.0, 1.0],
            frames: frame_count,
        },
        animations: HashMap::new(), // Not used directly when manually constructing
    });

    // Manually create a VatAnimationClip since we aren't loading from a file
    let default_clip = clips.add(VatAnimationClip {
        start_frame: 0,
        end_frame: frame_count,
        frame_rate: 10.0,
        looping: true,
    });

    // We just create a normal StandardMaterial. The plugin will wrap it automatically!
    let material = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.2, 0.2),
        double_sided: true,
        cull_mode: None,
        alpha_mode: AlphaMode::Mask(0.0),
        ..Default::default()
    });

    // Spawn a grid of entities, each with its own animation speed/offset
    let grid_width = 10;
    let grid_depth = 10;
    let duplicates = 2;
    let spacing = 1.2;
    for x in 0..grid_width {
        for z in 0..grid_depth {
            for i in 0..duplicates {
                commands.spawn((
                    Mesh3d(mesh_handle.clone()),
                    MeshMaterial3d(material.clone()),
                    VatAnimator {
                        remap_info: remap_info.clone(),
                        vat_texture: vat_texture_handle.clone(),
                        current_clip: default_clip.clone(),
                        speed: x as f32 / grid_width as f32
                            + z as f32 / grid_depth as f32
                            + i as f32 / duplicates as f32,
                        ..Default::default()
                    },
                    Transform::from_xyz(
                        x as f32 * spacing - (grid_width as f32 / 2.0) * spacing,
                        0.0,
                        z as f32 * spacing - (grid_depth as f32 / 2.0) * spacing,
                    ),
                ));
            }
        }
    }

    // Spawn camera and light
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 10.0, 15.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    commands.spawn((
        DirectionalLight {
            shadow_maps_enabled: true,
            illuminance: 10000.0,
            ..Default::default()
        },
        Transform::from_xyz(4.0, 8.0, 4.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}

fn create_bounce_texture(vertex_count: u32, frame_count: u32) -> Image {
    let mut data = Vec::new();

    // Generate Position Data
    for f in 0..frame_count {
        for _v in 0..vertex_count {
            let t = (f as f32 / frame_count as f32) * std::f32::consts::TAU;
            let y_offset = t.sin() * 2.0;
            data.extend_from_slice(&0.0f32.to_le_bytes()); // X
            data.extend_from_slice(&0.0f32.to_le_bytes()); // Y
            data.extend_from_slice(&y_offset.to_le_bytes()); // Z
            data.extend_from_slice(&1.0f32.to_le_bytes()); // W (Padding/Extra)
        }
    }

    // Generate Normal Data
    // Normals are encoded as (n + 1.0) / 2.0 to map [-1, 1] → [0, 1]
    // Shader decodes with: n * 2.0 - 1.0
    // For a Z-up normal in Blender space (0, 0, 1): encoded = (0.5, 0.5, 1.0)
    for _f in 0..frame_count + 1 {
        for _v in 0..vertex_count {
            data.extend_from_slice(&0.5f32.to_le_bytes()); // X: (0+1)/2
            data.extend_from_slice(&0.5f32.to_le_bytes()); // Y: (0+1)/2
            data.extend_from_slice(&1.0f32.to_le_bytes()); // Z: (1+1)/2
            data.extend_from_slice(&0.0f32.to_le_bytes()); // W (Padding)
        }
    }

    Image::new(
        Extent3d {
            width: vertex_count,
            height: frame_count + frame_count + 1,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba32Float,
        RenderAssetUsages::RENDER_WORLD,
    )
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
