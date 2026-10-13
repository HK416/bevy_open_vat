use bevy::{
    mesh::MeshTag, pbr::ExtendedMaterial, platform::collections::HashMap, prelude::*,
    render::storage::ShaderBuffer,
};

use crate::{
    asset::{RemapInfo, VatAnimationClip},
    data::{VatAnimator, VatInstanceData, VatMaterialReady, VatPropagated},
    material::OpenVatExtension,
};

/// Automatically sets up VAT materials for entities with a `VatAnimator`.
///
/// This system detects entities that have a `VatAnimator` but lack a `VatMaterialReady`
/// marker. It converts their existing `StandardMaterial` into an `ExtendedMaterial`
/// configured for vertex animation, and recursively propagates the `VatAnimator` to
/// their descendants.
pub fn auto_setup_vat_materials(
    mut commands: Commands,
    query: Query<
        (
            Entity,
            &VatAnimator,
            Option<&MeshMaterial3d<StandardMaterial>>,
        ),
        (Without<VatMaterialReady>, Without<VatPropagated>),
    >,
    children_query: Query<&Children>,
    has_std_material: Query<(), With<MeshMaterial3d<StandardMaterial>>>,
    images: Res<Assets<Image>>,
    remap_infos: Res<Assets<RemapInfo>>,
    std_materials: Res<Assets<StandardMaterial>>,
    mut vat_materials: ResMut<Assets<ExtendedMaterial<StandardMaterial, OpenVatExtension>>>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
    mut material_cache: Local<
        HashMap<
            (
                AssetId<StandardMaterial>,
                AssetId<Image>,
                AssetId<RemapInfo>,
            ),
            Handle<ExtendedMaterial<StandardMaterial, OpenVatExtension>>,
        >,
    >,
) {
    for (entity, animator, std_material) in query.iter() {
        let Some(image) = images.get(&animator.vat_texture) else {
            continue;
        };
        let Some(remap) = remap_infos.get(&animator.remap_info) else {
            continue;
        };

        let y_resolution = image.texture_descriptor.size.height as f32;
        let os_remap = &remap.os_remap;

        if let Some(mat_handle) = std_material {
            let cache_key = (
                mat_handle.0.id(),
                animator.vat_texture.id(),
                animator.remap_info.id(),
            );

            let extended_mat = if let Some(cached_mat) = material_cache.get(&cache_key) {
                cached_mat.clone()
            } else {
                let base_mat = std_materials
                    .get(&mat_handle.0)
                    .cloned()
                    .unwrap_or_default();

                let buffer = buffers.add(ShaderBuffer::from(vec![VatInstanceData::default()]));
                let mat = vat_materials.add(ExtendedMaterial {
                    base: base_mat,
                    extension: OpenVatExtension {
                        vat_texture: animator.vat_texture.clone(),
                        min_pos: Vec3::from_array(os_remap.min),
                        max_pos: Vec3::from_array(os_remap.max),
                        frame_count: os_remap.frames,
                        y_resolution,
                        instance: buffer,
                    },
                });
                material_cache.insert(cache_key, mat.clone());
                mat
            };

            commands
                .entity(entity)
                .remove::<MeshMaterial3d<StandardMaterial>>()
                .insert((MeshMaterial3d(extended_mat), VatMaterialReady, MeshTag::new(0)));
        }

        propagate_to_descendants(
            entity,
            animator,
            &mut commands,
            &children_query,
            &has_std_material,
        );
    }

    // Clean up dead materials from the cache if their original assets have been dropped
    material_cache.retain(|(mat_id, img_id, remap_id), _| {
        std_materials.contains(*mat_id) && images.contains(*img_id) && remap_infos.contains(*remap_id)
    });
}

/// Updates the animation state for all active VAT instances and syncs data to the GPU.
///
/// This system calculates the current frame, rate, and offset for each `VatAnimator`,
/// batches the instance data by material, and updates the corresponding `ShaderBuffer`.
/// It also assigns deterministic `MeshTag` indices to ensure stable rendering.
pub fn update_instance_data(
    mut controller_query: Query<(
        Entity,
        &VatAnimator,
        &mut MeshTag,
        &MeshMaterial3d<ExtendedMaterial<StandardMaterial, OpenVatExtension>>,
    )>,
    clips: Res<Assets<VatAnimationClip>>,
    materials: Res<Assets<ExtendedMaterial<StandardMaterial, OpenVatExtension>>>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
) {
    let mut sorted_entities: Vec<_> = controller_query.iter_mut().collect();
    sorted_entities.sort_by_key(|(entity, _, _, _)| *entity);

    let mut material_batches: HashMap<
        AssetId<ExtendedMaterial<StandardMaterial, OpenVatExtension>>,
        Vec<VatInstanceData>,
    > = HashMap::new();

    // Build GPU data grouped by material in a deterministic order
    for (_, animator, mut mesh_tag, mat_handle) in sorted_entities.into_iter() {
        let mat_id = mat_handle.0.id();
        let batch = material_batches.entry(mat_id).or_default();

        let index = batch.len() as u32;

        if mesh_tag.value != index {
            mesh_tag.value = index;
        }

        let Some(clip) = clips.get(&animator.current_clip) else {
            batch.push(VatInstanceData::default());
            continue;
        };

        let duration = clip.duration().unwrap_or(1.0);
        let duration = if duration <= 0.0 { 1.0 } else { duration };
        let speed = if animator.is_playing {
            animator.speed
        } else {
            0.0
        };
        let rate = speed / duration;

        let offset = -(animator.start_time * rate) + animator.offset;

        batch.push(VatInstanceData {
            start_frame: clip.start_frame,
            frame_count: clip.frame_count(),
            rate,
            offset,
        });
    }

    // Sync every VAT material's buffer; materials with no active instances get a single default entry.
    // Animation time advances on the GPU, so buffers are only written when their contents change.
    // Bevy rebuilds the material bind group by itself when a buffer is reallocated.
    let idle = [VatInstanceData::default()];
    for (mat_id, mat) in materials.iter() {
        let data = material_batches.get(&mat_id).map_or(&idle[..], Vec::as_slice);
        let handle = &mat.extension.instance;
        if !buffers.get(handle).is_some_and(|buffer| needs_write(buffer, data)) {
            continue;
        }
        if let Some(mut buffer) = buffers.get_mut(handle) {
            buffer.clear();
            buffer.extend_from_slice(data);
        }
    }
}

/// Returns `true` if `buffer` does not already hold exactly `data`.
fn needs_write(buffer: &ShaderBuffer, data: &[VatInstanceData]) -> bool {
    buffer.cast_slice::<VatInstanceData>() != Some(data)
}

/// Propagates the `VatAnimator` to all descendant entities using a breadth-first search.
///
/// Entities that contain a `MeshMaterial3d<StandardMaterial>` receive the `VatAnimator`
/// component. Container entities receive the `VatPropagated` marker to prevent redundant processing.
fn propagate_to_descendants(
    root: Entity,
    animator: &VatAnimator,
    commands: &mut Commands,
    children_query: &Query<&Children>,
    has_std_material: &Query<(), With<MeshMaterial3d<StandardMaterial>>>,
) {
    let Ok(children) = children_query.get(root) else {
        return;
    };

    commands.entity(root).insert(VatPropagated);

    let mut queue: Vec<Entity> = children.iter().collect();

    while let Some(child) = queue.pop() {
        if has_std_material.get(child).is_ok() {
            commands.entity(child).insert(animator.clone());
        } else {
            commands.entity(child).insert(VatPropagated);
        }

        if let Ok(grandchildren) = children_query.get(child) {
            queue.extend(grandchildren.iter());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instance(rate: f32) -> VatInstanceData {
        VatInstanceData {
            rate,
            ..Default::default()
        }
    }

    #[test]
    fn test_needs_write() {
        let data = vec![instance(1.0), instance(2.0)];
        let buffer = ShaderBuffer::from(data.clone());

        assert!(!needs_write(&buffer, &data));
        assert!(needs_write(&buffer, &[instance(1.0), instance(3.0)]));
        assert!(needs_write(&buffer, &data[..1]));
        assert!(needs_write(&ShaderBuffer::default(), &data));
    }
}
