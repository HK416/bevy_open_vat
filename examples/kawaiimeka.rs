use bevy::{
    mesh::MeshTag, pbr::ExtendedMaterial, platform::collections::HashMap, prelude::*,
    render::storage::ShaderStorageBuffer,
};
use bevy_open_vat::{data::VatInstanceData, prelude::*};

const NUM_WIDTH: usize = 50;
const NUM_DEPTH: usize = 50;
const SPACING: f32 = 5.0;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(OpenVatPlugin)
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            insert_extended_materials.run_if(resource_exists::<AssetHandles>),
        )
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
            shadows_enabled: true,
            ..Default::default()
        },
        Transform::from_xyz(4.0, 8.0, 3.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    let scene_handle =
        asset_server.load(GltfAssetLabel::Scene(0).from_asset("models/KawaiiMeka/KawaiiMeka.glb"));
    let remap_handle: Handle<RemapInfo> =
        asset_server.load("models/KawaiiMeka/KawaiiMeka-remap_info.json");
    let vat_handle: Handle<Image> = asset_server.load("models/KawaiiMeka/Collection_vat.exr");

    for x in 0..NUM_WIDTH {
        for z in 0..NUM_DEPTH {
            commands.spawn((
                SceneRoot(scene_handle.clone()),
                Transform::from_xyz(
                    x as f32 * SPACING - (NUM_WIDTH as f32 / 2.0) * SPACING,
                    0.0,
                    z as f32 * SPACING - (NUM_DEPTH as f32 / 2.0) * SPACING,
                ),
            ));
        }
    }

    commands.insert_resource(AssetHandles {
        remap_handle,
        vat_handle,
    });
}

fn insert_extended_materials(
    mut commands: Commands,
    asset_handles: Res<AssetHandles>,
    images: Res<Assets<Image>>,
    remap_infos: Res<Assets<RemapInfo>>,
    std_materials: Res<Assets<StandardMaterial>>,
    mut vat_materials: ResMut<Assets<ExtendedMaterial<StandardMaterial, OpenVatExtension>>>,
    mut buffers: ResMut<Assets<ShaderStorageBuffer>>,
    mut entity_query: Query<(Entity, &MeshMaterial3d<StandardMaterial>)>,
) {
    let target_count = NUM_WIDTH * NUM_DEPTH;
    let entities: Vec<_> = entity_query.iter_mut().collect();

    if entities.len() < target_count {
        return;
    }

    let vat_texture = &asset_handles.vat_handle;
    let y_resolution = if let Some(image) = images.get(vat_texture) {
        image.texture_descriptor.size.height as f32
    } else {
        return;
    };

    let Some(remap_info) = remap_infos.get(&asset_handles.remap_handle) else {
        return;
    };

    let instance_data_vec: Vec<VatInstanceData> = Vec::with_capacity(entities.len());
    let buffer_handle = buffers.add(ShaderStorageBuffer::from(&instance_data_vec));

    let mut material_cache: HashMap<
        _,
        Handle<ExtendedMaterial<StandardMaterial, OpenVatExtension>>,
    > = HashMap::default();
    for (index, (entity, old_mat)) in entities.into_iter().enumerate() {
        let Some(std_material) = std_materials.get(&old_mat.0) else {
            continue;
        };

        match material_cache.get(&old_mat.0) {
            Some(material) => {
                commands.entity(entity).insert((
                    MeshMaterial3d(material.clone()),
                    VatAnimationController {
                        remap_info: asset_handles.remap_handle.clone(),
                        current_clip: "Idle".to_string(),
                        ..Default::default()
                    },
                    MeshTag(index as u32),
                ));
            }
            None => {
                let extended_material = ExtendedMaterial {
                    base: std_material.clone(),
                    extension: OpenVatExtension {
                        vat_texture: vat_texture.clone(),
                        min_pos: remap_info.os_remap.min.into(),
                        frame_count: remap_info.os_remap.frames,
                        max_pos: remap_info.os_remap.max.into(),
                        y_resolution,
                        instance: buffer_handle.clone(),
                    },
                };

                let material = vat_materials.add(extended_material);
                commands.entity(entity).insert((
                    MeshMaterial3d(material.clone()),
                    VatAnimationController {
                        remap_info: asset_handles.remap_handle.clone(),
                        current_clip: "Idle".to_string(),
                        ..Default::default()
                    },
                    MeshTag(index as u32),
                ));

                material_cache.insert(old_mat.0.clone(), material);
            }
        }
    }

    commands.remove_resource::<AssetHandles>();
}

#[derive(Resource)]
struct AssetHandles {
    remap_handle: Handle<RemapInfo>,
    vat_handle: Handle<Image>,
}
