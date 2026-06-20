use bevy::{
    pbr::ExtendedMaterial,
    prelude::*,
    render::storage::ShaderBuffer,
};
use bevy_open_vat::{
    data::VatAnimator, material::OpenVatExtension, plugin::OpenVatPlugin,
};

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(OpenVatPlugin)
        .add_systems(Startup, setup)
        .add_systems(Update, (spawn_and_despawn_system, debug_asset_counts_system))
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

    let mut dummy_image = Image::default();
    dummy_image.texture_descriptor.size.height = 64;
    dummy_image.texture_descriptor.size.width = 64;
    dummy_image.data = vec![0u8; 64 * 64 * 4].into();
    let vat_texture = images.add(dummy_image);

    let remap_info = remap_infos.add(bevy_open_vat::asset::RemapInfo {
        os_remap: bevy_open_vat::asset::OsRemap {
            min: [-1.0, -1.0, -1.0],
            max: [1.0, 1.0, 1.0],
            frames: 64,
        },
        animations: bevy::platform::collections::HashMap::new(),
    });

    let mut mesh = Cuboid::new(1.0, 1.0, 1.0).mesh().build();
    if let Some(uvs) = mesh.attribute(Mesh::ATTRIBUTE_UV_0) {
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, uvs.clone());
    }

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
        VatAnimator {
            vat_texture: test_assets.vat_texture.clone(),
            remap_info: test_assets.remap_info.clone(),
            ..default()
        },
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
