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

        controller.start_time += dt * controller.speed;

        let duration = clip.duration().unwrap_or(1.0);

        match clip.looping {
            false => {
                if controller.start_time >= duration {
                    controller.start_time = duration;
                    controller.is_playing = false;
                } else if controller.start_time < 0.0 {
                    controller.start_time = 0.0;
                    controller.is_playing = false;
                }
            }
            true => {
                if controller.start_time >= duration {
                    controller.start_time %= duration;
                } else if controller.start_time < 0.0 {
                    controller.start_time = duration + (controller.start_time % duration);
                }
            }
        }
    }
}

/// Synchronizes the CPU-side animation state with the GPU via a storage buffer.
/// Optimized: Only rebuilds buffer when entities are added/removed or components change.
pub fn update_instance_data(
    mut commands: Commands,
    changed_query: Query<Entity, Changed<VatAnimationController>>,
    controller_query: Query<(Entity, &VatAnimationController, Option<&MeshTag>)>,
    mut materials: ResMut<Assets<ExtendedMaterial<StandardMaterial, OpenVatExtension>>>,
    mat_query: Query<&MeshMaterial3d<ExtendedMaterial<StandardMaterial, OpenVatExtension>>>,
    remap_infos: Res<Assets<RemapInfo>>,
    mut buffers: ResMut<Assets<ShaderStorageBuffer>>,
    mut remap_events: MessageReader<AssetEvent<RemapInfo>>,
    mut last_count: Local<usize>,
) {
    let current_count = controller_query.iter().len();
    let any_changed = !changed_query.is_empty();
    let asset_changed = !remap_events.is_empty();
    remap_events.clear();

    // Skip update if nothing changed (Performance Optimization)
    if !any_changed && *last_count == current_count && !asset_changed {
        return;
    }
    *last_count = current_count;

    let mut gpu_data_vec: Vec<VatInstanceData> = Vec::with_capacity(current_count);

    for (index, (entity, controller, existing_tag)) in controller_query.iter().enumerate() {
        let target_tag_val = index as u32;

        let needs_tag_update = match existing_tag {
            Some(tag) => tag.0 != target_tag_val,
            None => true,
        };

        if needs_tag_update {
            commands.entity(entity).insert(MeshTag(target_tag_val));
        }

        let Some(remap_info) = remap_infos.get(&controller.remap_info) else {
            // Fill dummy data to keep index alignment if asset not ready
            gpu_data_vec.push(VatInstanceData::default());
            commands.entity(entity).insert(MeshTag(index as u32));
            continue;
        };
        let Some(clip) = remap_info.animations.get(&controller.current_clip) else {
            gpu_data_vec.push(VatInstanceData::default());
            commands.entity(entity).insert(MeshTag(index as u32));
            continue;
        };

        let duration = clip.duration().unwrap_or(1.0);
        let speed = if controller.is_playing {
            controller.speed
        } else {
            0.0
        };
        let rate = speed / duration;
        let offset = -(controller.start_time * rate) + controller.offset;

        gpu_data_vec.push(VatInstanceData {
            start_frame: clip.start_frame,
            frame_count: clip.end_frame - clip.start_frame,
            rate,
            offset,
        });
    }

    if gpu_data_vec.is_empty() {
        return;
    }

    // Batch update all buffers
    for mat_handle in mat_query.iter() {
        if let Some(mat) = materials.get_mut(&mat_handle.0) {
            if let Some(buffer) = buffers.get_mut(&mat.extension.instance) {
                buffer.set_data(gpu_data_vec.clone());
            }
        }
    }
}
