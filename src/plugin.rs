use bevy::asset::{load_internal_asset, uuid_handle};
use bevy::{pbr::ExtendedMaterial, prelude::*};

use crate::asset::{RemapInfo, RemapInfoAssetLoader};
use crate::material::OpenVatExtension;
use crate::system::update_instance_data;

pub const OPENVAT_SHADER_HANDLE: Handle<Shader> =
    uuid_handle!("d4909c30-b350-4ae2-b003-b03b7adcb66d");
pub const OPENVAT_PREPASS_SHADER_HANDLE: Handle<Shader> =
    uuid_handle!("460afec2-b2d7-4ba3-9858-026997e63d4d");

/// Plugin that sets up the VAT material extension and animation update systems.
pub struct OpenVatPlugin;

impl Plugin for OpenVatPlugin {
    fn build(&self, app: &mut App) {
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
            .register_asset_loader(RemapInfoAssetLoader)
            .add_plugins(Plugin::default())
            .add_systems(PostUpdate, update_instance_data);
    }
}
