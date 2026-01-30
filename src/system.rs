use bevy::{
    mesh::MeshTag, pbr::ExtendedMaterial, prelude::*, render::storage::ShaderStorageBuffer,
};

use crate::{
    asset::RemapInfo,
    data::{VatAnimationController, VatInstanceData},
    material::OpenVatExtension,
};

/// Updates the `VatAnimationController` components, advancing their timers based on delta time and playback speed.
/// Handles looping logic (Once vs Loop).
pub fn update_anim_controller(
    time: Res<Time>,
    remap_infos: Res<Assets<RemapInfo>>,
    mut query: Query<&mut VatAnimationController>,
) {
    let dt = time.delta_secs();

    for mut controller in query.iter_mut() {
        if !controller.is_playing {
            continue;
        }

        let Some(remap_info) = remap_infos.get(&controller.remap_info) else {
            warn!("RemapInfo asset not found for VatAnimationController");
            continue;
        };
        let Some(clip) = remap_info.animations.get(&controller.current_clip) else {
            warn!(
                "Animation clip '{}' not found in RemapInfo",
                controller.current_clip
            );
            continue;
        };

        controller.timer += dt * controller.speed;

        let duration = clip.duration().unwrap_or(1.0);

        match clip.looping {
            false => {
                if controller.timer >= duration {
                    controller.timer = duration;
                    controller.is_playing = false;
                } else if controller.timer < 0.0 {
                    controller.timer = 0.0;
                    controller.is_playing = false;
                }
            }
            true => {
                if controller.timer >= duration {
                    controller.timer %= duration;
                } else if controller.timer < 0.0 {
                    controller.timer = duration + (controller.timer % duration);
                }
            }
        }
    }
}

/// Synchronizes the CPU-side animation state with the GPU via a storage buffer.
/// Assigns `MeshTag`s (instance indices) to entities and uploads `VatInstanceData`.
pub fn update_instance_data(
    mut commands: Commands,
    controller_query: Query<(Entity, &VatAnimationController)>,
    mut materials: ResMut<Assets<ExtendedMaterial<StandardMaterial, OpenVatExtension>>>,
    mat_query: Query<&MeshMaterial3d<ExtendedMaterial<StandardMaterial, OpenVatExtension>>>,
    remap_infos: Res<Assets<RemapInfo>>,
    mut buffers: ResMut<Assets<ShaderStorageBuffer>>,
) {
    let mut gpu_data_vec: Vec<VatInstanceData> = Vec::with_capacity(controller_query.iter().len());

    // Collect data for all active controllers
    for (index, (entity, controller)) in controller_query.iter().enumerate() {
        let Some(remap_info) = remap_infos.get(&controller.remap_info) else {
            warn!("RemapInfo asset not found for entity {:?}", entity);
            continue;
        };
        let Some(clip) = remap_info.animations.get(&controller.current_clip) else {
            warn!(
                "Animation clip '{}' not found in RemapInfo for entity {:?}",
                controller.current_clip, entity
            );
            continue;
        };

        gpu_data_vec.push(VatInstanceData {
            timer: clip.start_frame as f32 + controller.timer * clip.frame_rate,
        });

        // Assign an index to the entity so the shader knows which instance data to read
        commands.entity(entity).insert(MeshTag(index as u32));
    }

    if gpu_data_vec.is_empty() {
        return;
    }

    // Update the storage buffer for all materials using this extension
    for mat_handle in mat_query.iter() {
        if let Some(mat) = materials.get_mut(&mat_handle.0) {
            if let Some(buffer) = buffers.get_mut(&mat.extension.instance) {
                buffer.set_data(gpu_data_vec.clone());
            }
        }
    }
}
