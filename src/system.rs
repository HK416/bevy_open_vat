use bevy::{
    mesh::MeshTag, pbr::ExtendedMaterial, prelude::*, render::storage::ShaderStorageBuffer,
};

use crate::{
    data::{VatAnimLoopMode, VatAnimationController, VatInstanceData},
    material::OpenVatExtension,
};

/// Updates the `VatAnimationController` components, advancing their timers based on delta time and playback speed.
/// Handles looping logic (Once vs Loop).
pub fn update_anim_controller(time: Res<Time>, mut query: Query<&mut VatAnimationController>) {
    let dt = time.delta_secs();

    for mut controller in query.iter_mut() {
        if !controller.is_playing {
            continue;
        }

        controller.timer += dt * controller.speed;

        let duration = if controller.current_clip.sampling_fps > 0.0 {
            controller.current_clip.frame_count as f32 / controller.current_clip.sampling_fps
        } else {
            1.0
        };

        match controller.mode {
            VatAnimLoopMode::Once => {
                if controller.timer >= duration {
                    controller.timer = duration;
                    controller.is_playing = false;
                } else if controller.timer < 0.0 {
                    controller.timer = 0.0;
                    controller.is_playing = false;
                }
            }
            VatAnimLoopMode::Loop => {
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
    mut buffers: ResMut<Assets<ShaderStorageBuffer>>,
) {
    let mut gpu_data_vec: Vec<VatInstanceData> = Vec::with_capacity(controller_query.iter().len());

    // Collect data for all active controllers
    for (index, (entity, controller)) in controller_query.iter().enumerate() {
        gpu_data_vec.push(VatInstanceData {
            timer: controller.timer * controller.current_clip.sampling_fps,
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
