use bevy::{pbr::ExtendedMaterial, prelude::*};

use crate::material::OpenVatExtension;
use crate::system::{update_anim_controller, update_instance_data};

pub struct OpenVatPlugin;

impl Plugin for OpenVatPlugin {
    fn build(&self, app: &mut App) {
        type Plugin = MaterialPlugin<ExtendedMaterial<StandardMaterial, OpenVatExtension>>;
        app.add_plugins(Plugin::default()).add_systems(
            Update,
            (update_anim_controller, update_instance_data).chain(),
        );
    }
}
