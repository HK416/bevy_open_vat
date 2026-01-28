use bevy::{pbr::ExtendedMaterial, prelude::*, render::storage::ShaderStorageBuffer};
use bevy_common_assets::json::JsonAssetPlugin;
use bevy_open_vat::{data::VatInstanceData, prelude::*};
use serde::Deserialize;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(OpenVatPlugin)
        .add_plugins(JsonAssetPlugin::<RemapInfo>::new(&["json"]))
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
        Transform::from_xyz(6.0, 6.0, 6.0).looking_at(Vec3::new(0.0, 1.0, 0.0), Vec3::Y),
    ));

    commands.spawn((
        DirectionalLight {
            shadows_enabled: true,
            ..Default::default()
        },
        Transform::from_xyz(4.0, 8.0, 3.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    commands.spawn(SceneRoot(asset_server.load(
        GltfAssetLabel::Scene(0).from_asset("models/KawaiiMeka/KawaiiMeka.glb"),
    )));

    let remap_handle: Handle<RemapInfo> =
        asset_server.load("models/KawaiiMeka/KawaiiMeka-remap_info.json");
    let vat_handle: Handle<Image> = asset_server.load("models/KawaiiMeka/Collection_vat.exr");

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
    if entity_query.is_empty() {
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

    for (entity, mesh_material) in entity_query.iter_mut() {
        if let Some(std_material) = std_materials.get(&mesh_material.0) {
            let mut buffer = ShaderStorageBuffer::default();
            buffer.set_data(vec![VatInstanceData::default()]);

            let material = vat_materials.add(ExtendedMaterial {
                base: std_material.clone(),
                extension: OpenVatExtension {
                    vat_texture: vat_texture.clone(),
                    min_pos: remap_info.os_remap.min.into(),
                    frame_count: remap_info.os_remap.frames,
                    max_pos: remap_info.os_remap.max.into(),
                    y_resolution,
                    instance: buffers.add(buffer),
                },
            });

            commands.entity(entity).insert((
                MeshMaterial3d(material),
                VatAnimationController {
                    mode: VatAnimLoopMode::Loop,
                    current_clip: VatAnimationClip {
                        frame_count: remap_info.os_remap.frames,
                        sampling_fps: 24.0,
                    },
                    speed: 1.0,
                    ..Default::default()
                },
            ));
        }
    }

    commands.remove_resource::<AssetHandles>();
}

#[derive(Resource)]
struct AssetHandles {
    remap_handle: Handle<RemapInfo>,
    vat_handle: Handle<Image>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct OsRemap {
    #[serde(rename = "Min")]
    pub min: [f32; 3],
    #[serde(rename = "Max")]
    pub max: [f32; 3],
    #[serde(rename = "Frames")]
    pub frames: u32,
}

#[derive(Debug, Deserialize, Asset, TypePath, Clone)]
pub struct RemapInfo {
    #[serde(rename = "os-remap")]
    pub os_remap: OsRemap,
}
