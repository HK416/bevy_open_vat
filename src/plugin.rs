use bevy::{pbr::ExtendedMaterial, prelude::*};

use crate::asset::{RemapInfo, RemapInfoAssetLoader};
use crate::material::OpenVatExtension;
use crate::system::{update_anim_controller, update_instance_data};

/// Plugin that sets up the VAT material extension and animation update systems.
pub struct OpenVatPlugin;

impl Plugin for OpenVatPlugin {
    fn build(&self, app: &mut App) {
        type Plugin = MaterialPlugin<ExtendedMaterial<StandardMaterial, OpenVatExtension>>;
        app.init_asset::<RemapInfo>()
            .register_asset_loader(RemapInfoAssetLoader)
            .add_plugins(Plugin::default())
            .add_systems(
                Update,
                (update_anim_controller, update_instance_data).chain(),
            );
    }
}
