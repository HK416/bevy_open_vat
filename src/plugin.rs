use bevy::asset::{load_internal_asset, uuid_handle};
use bevy::{pbr::ExtendedMaterial, prelude::*};

use crate::asset::{RemapInfo, RemapInfoAssetLoader};
use crate::material::OpenVatExtension;
use crate::system::{auto_setup_vat_materials, update_instance_data};

/// Handle for the main OpenVAT PBR shader.
pub const OPENVAT_SHADER_HANDLE: Handle<Shader> =
    uuid_handle!("d4909c30-b350-4ae2-b003-b03b7adcb66d");
/// Handle for the OpenVAT prepass shader.
pub const OPENVAT_PREPASS_SHADER_HANDLE: Handle<Shader> =
    uuid_handle!("460afec2-b2d7-4ba3-9858-026997e63d4d");
/// Handle for the common OpenVAT shader utilities.
pub const OPENVAT_COMMON_SHADER_HANDLE: Handle<Shader> =
    uuid_handle!("a1b2c3d4-e5f6-7890-abcd-ef1234567890");

/// Main plugin for OpenVAT, setting up assets, materials, and systems.
pub struct OpenVatPlugin;

impl Plugin for OpenVatPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(
            app,
            OPENVAT_COMMON_SHADER_HANDLE,
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/assets/shaders/openvat_common.wgsl"
            ),
            Shader::from_wgsl
        );

        load_internal_asset!(
            app,
            OPENVAT_SHADER_HANDLE,
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/assets/shaders/openvat_pbr.wgsl"
            ),
            Shader::from_wgsl
        );

        load_internal_asset!(
            app,
            OPENVAT_PREPASS_SHADER_HANDLE,
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/assets/shaders/openvat_prepass.wgsl"
            ),
            Shader::from_wgsl
        );

        type Plugin = MaterialPlugin<ExtendedMaterial<StandardMaterial, OpenVatExtension>>;
        app.init_asset::<RemapInfo>()
            .init_asset::<crate::asset::VatAnimationClip>()
            .register_asset_loader(RemapInfoAssetLoader)
            .add_plugins(Plugin::default())
            .add_systems(Update, auto_setup_vat_materials)
            .add_systems(PostUpdate, update_instance_data);
    }
}
