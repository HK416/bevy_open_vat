use bevy::{
    asset::RenderAssetUsages,
    mesh::VertexAttributeValues,
    pbr::ExtendedMaterial,
    prelude::*,
    render::{
        render_resource::{Extent3d, TextureDimension, TextureFormat},
        storage::ShaderStorageBuffer,
    },
};
use bevy_open_vat::prelude::*;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(OpenVatPlugin)
        .add_systems(Startup, setup)
        .run();
}

/// Sets up a scene with a procedurally generated VAT animation.
fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut buffers: ResMut<Assets<ShaderStorageBuffer>>,
    mut vat_materials: ResMut<Assets<ExtendedMaterial<StandardMaterial, OpenVatExtension>>>,
    mut images: ResMut<Assets<Image>>,
) {
    // Create a simple plane mesh
    let mut plane_mesh = Mesh::from(Plane3d::default().mesh().size(1.0, 1.0));

    // Assign unique UV1 coordinates to each vertex.
    // In VAT, UV1 is often used to index into the VAT texture's X-axis (vertex index).
    plane_mesh.insert_attribute(
        Mesh::ATTRIBUTE_UV_1,
        VertexAttributeValues::Float32x2(vec![
            [0.0 / 4.0, 0.0],
            [1.0 / 4.0, 0.0],
            [2.0 / 4.0, 0.0],
            [3.0 / 4.0, 0.0],
        ]),
    );

    let mesh_handle = meshes.add(plane_mesh);
    let vertex_count = 4;
    let frame_count = 60;

    // Create a procedural texture that makes vertices "bounce"
    let vat_texture = create_bounce_texture(vertex_count, frame_count);
    let vat_texture_handle = images.add(vat_texture);

    // Initialize the VAT material extension
    let material = vat_materials.add(ExtendedMaterial {
        base: StandardMaterial {
            base_color: Color::srgb(1.0, 0.2, 0.2),
            double_sided: true,
            cull_mode: None,
            ..Default::default()
        },
        extension: OpenVatExtension {
            vat_texture: vat_texture_handle,
            min_pos: Vec3::ZERO,
            frame_count,
            max_pos: Vec3::ONE,
            y_resolution: (frame_count * 2) as f32, // Position + Normal rows
            instance: buffers.add(ShaderStorageBuffer::default()),
        },
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
                    VatAnimationController {
                        mode: VatAnimLoopMode::Loop,
                        current_clip: VatAnimationClip {
                            frame_count,
                            sampling_fps: 1.0,
                        },
                        // Varying speed based on position to demonstrate independent control
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
            shadows_enabled: true,
            illuminance: 10000.0,
            ..Default::default()
        },
        Transform::from_xyz(4.0, 8.0, 4.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}

/// Generates a procedural VAT texture where vertices move in a sinusoidal "bounce".
///
/// Texture Layout:
/// - Rows 0..frame_count: Position offsets (XYZ)
/// - Rows frame_count..frame_count*2: Normal vectors
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

    // Generate Normal Data (pointing straight up for simplicity)
    for _f in 0..frame_count + 1 {
        for _v in 0..vertex_count {
            data.extend_from_slice(&0.0f32.to_le_bytes());
            data.extend_from_slice(&1.0f32.to_le_bytes());
            data.extend_from_slice(&0.0f32.to_le_bytes());
            data.extend_from_slice(&0.0f32.to_le_bytes());
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
