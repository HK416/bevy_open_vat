use bevy::{
    asset::RenderAssetUsages,
    pbr::ExtendedMaterial,
    prelude::*,
    render::{
        render_resource::{Extent3d, TextureDimension, TextureFormat},
        storage::ShaderBuffer,
    },
};
use bevy_open_vat::{data::VatAnimator, material::OpenVatExtension, plugin::OpenVatPlugin};

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(OpenVatPlugin)
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (spawn_and_despawn_system, debug_asset_counts_system),
        )
        .run();
}

#[derive(Resource)]
struct LeakTestAssets {
    mesh: Handle<Mesh>,
    vat_texture: Handle<Image>,
    remap_info: Handle<bevy_open_vat::asset::RemapInfo>,
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut remap_infos: ResMut<Assets<bevy_open_vat::asset::RemapInfo>>,
) {
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 5.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        DirectionalLight::default(),
        Transform::from_xyz(5.0, 5.0, 5.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    let mut mesh = Cuboid::new(1.0, 1.0, 1.0).mesh().build();
    let vertex_count = mesh.count_vertices();
    let frames = 64;
    let height = frames * 2;

    // One texture column per vertex: 0.5 texels encode a zero position offset in the upper
    // half, and in the lower half a normal that decodes to (0, 0, 1) in Blender space (up).
    // Not sRGB: VAT texels are data, and 128 must read back as 0.5.
    let mut data = vec![128u8; vertex_count * height * 4];
    for texel in data[vertex_count * frames * 4..].as_chunks_mut::<4>().0 {
        texel[2] = 255;
    }
    let vat_texture = images.add(Image::new(
        Extent3d {
            width: vertex_count as u32,
            height: height as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    ));

    // UV1 points each vertex at its own column in frame 0.
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_UV_1,
        (0..vertex_count)
            .map(|i| [(i as f32 + 0.5) / vertex_count as f32, 0.5 / height as f32])
            .collect::<Vec<_>>(),
    );

    let remap_info = remap_infos.add(bevy_open_vat::asset::RemapInfo {
        os_remap: bevy_open_vat::asset::OsRemap {
            min: [-1.0, -1.0, -1.0],
            max: [1.0, 1.0, 1.0],
            frames: frames as u32,
        },
        animations: bevy::platform::collections::HashMap::new(),
    });

    commands.insert_resource(LeakTestAssets {
        mesh: meshes.add(mesh),
        vat_texture,
        remap_info,
    });
}

#[derive(Component)]
struct DespawnTimer(Timer);

fn spawn_and_despawn_system(
    mut commands: Commands,
    time: Res<Time>,
    test_assets: Res<LeakTestAssets>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut query: Query<(Entity, &mut DespawnTimer)>,
) {
    // Spawn a new VAT entity every frame with a unique material
    let unique_material = materials.add(StandardMaterial {
        base_color: Color::srgb(time.elapsed_secs().fract(), 0.5, 0.5),
        ..default()
    });

    commands.spawn((
        Mesh3d(test_assets.mesh.clone()),
        MeshMaterial3d(unique_material),
        Transform::from_xyz(0.0, 0.0, 0.0),
        VatAnimator::new(
            test_assets.remap_info.clone(),
            test_assets.vat_texture.clone(),
            Handle::default(),
        ),
        DespawnTimer(Timer::from_seconds(0.5, TimerMode::Once)),
    ));

    for (entity, mut timer) in query.iter_mut() {
        if timer.0.tick(time.delta()).just_finished() {
            commands.entity(entity).despawn();
        }
    }
}

fn debug_asset_counts_system(
    time: Res<Time>,
    mut timer: Local<f32>,
    extended_mats: Res<Assets<ExtendedMaterial<StandardMaterial, OpenVatExtension>>>,
    shader_buffers: Res<Assets<ShaderBuffer>>,
) {
    *timer += time.delta_secs();
    if *timer >= 1.0 {
        *timer = 0.0;
        println!(
            "ExtendedMaterials count: {}, ShaderBuffers count: {}",
            extended_mats.len(),
            shader_buffers.len()
        );
        // If there is no leak, the counts should not grow infinitely.
        // They should remain small (e.g., around the number of currently spawned unique materials, which is 1 here).
    }
}
