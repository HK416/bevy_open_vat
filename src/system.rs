use bevy::{
    camera::{
        primitives::{Aabb, MeshAabb},
        visibility::NoAutoAabb,
    },
    ecs::entity::EntityHashMap,
    mesh::MeshTag,
    pbr::ExtendedMaterial,
    platform::collections::HashMap,
    prelude::*,
    render::storage::ShaderBuffer,
};

use crate::{
    asset::{RemapInfo, VatAnimationClip},
    data::{VatAnimator, VatClipFinished, VatInstanceData, VatMaterialReady, VatPropagated},
    material::{OpenVatExtension, VatStandardMaterial},
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
/// Animation time advances on the GPU, so this only does work when an animator, its material,
/// or a clip changed, an instance was removed, or the shader's wrapped time rolled over.
/// It batches the instance data by material, updates the corresponding `ShaderBuffer`,
/// and assigns deterministic `MeshTag` indices to ensure stable rendering.
#[allow(clippy::too_many_arguments)]
pub fn update_instance_data(
    mut controller_query: Query<(
        Entity,
        &VatAnimator,
        &mut MeshTag,
        &MeshMaterial3d<VatStandardMaterial>,
    )>,
    changed: Query<
        (),
        (
            With<MeshTag>,
            Or<(
                Changed<VatAnimator>,
                Changed<MeshMaterial3d<VatStandardMaterial>>,
            )>,
        ),
    >,
    mut removed_animators: RemovedComponents<VatAnimator>,
    mut removed_materials: RemovedComponents<MeshMaterial3d<VatStandardMaterial>>,
    mut clip_events: MessageReader<AssetEvent<VatAnimationClip>>,
    time: Res<Time>,
    mut last_wrap_base: Local<f64>,
    clips: Res<Assets<VatAnimationClip>>,
    materials: Res<Assets<VatStandardMaterial>>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
) {
    let wrap_base = time.elapsed_secs_f64() - time.elapsed_secs_wrapped_f64();
    let time_wrapped = wrap_base != *last_wrap_base;
    *last_wrap_base = wrap_base;
    // Drain every reader each frame, even when another check already marked us dirty.
    let removed = removed_animators.read().count() + removed_materials.read().count() > 0;
    let clips_changed = clip_events.read().count() > 0;
    if changed.is_empty() && !removed && !clips_changed && !time_wrapped {
        return;
    }

    let mut sorted_entities: Vec<_> = controller_query.iter_mut().collect();
    sorted_entities.sort_by_key(|(entity, _, _, _)| *entity);

    let mut material_batches: HashMap<AssetId<VatStandardMaterial>, Vec<VatInstanceData>> =
        HashMap::new();

    // Build GPU data grouped by material in a deterministic order
    for (_, animator, mut mesh_tag, mat_handle) in sorted_entities.into_iter() {
        let batch = material_batches.entry(mat_handle.0.id()).or_default();

        let index = batch.len() as u32;
        if mesh_tag.value != index {
            mesh_tag.value = index;
        }

        batch.push(
            clips
                .get(animator.current_clip())
                .map(|clip| animator.instance_data(clip, wrap_base))
                .unwrap_or_default(),
        );
    }

    // Sync every VAT material's buffer; materials with no active instances get a single default entry.
    // Buffers are only written when their contents change.
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

/// Gives VAT meshes a bounding box that covers every animated pose.
///
/// Bevy computes the AABB from the rest pose, so frustum culling could drop meshes whose
/// animation moves vertices outside it. The rest bounds are grown by the VAT offset range,
/// and `NoAutoAabb` keeps Bevy from recomputing them.
pub fn update_vat_aabbs(
    mut commands: Commands,
    meshes: Res<Assets<Mesh>>,
    remap_infos: Res<Assets<RemapInfo>>,
    query: Query<(Entity, &Mesh3d, &VatAnimator), (With<VatMaterialReady>, Without<NoAutoAabb>)>,
) {
    for (entity, mesh, animator) in &query {
        let Some(rest) = meshes.get(mesh).and_then(|mesh| mesh.get_aabb()) else {
            continue;
        };
        let Some(remap) = remap_infos.get(&animator.remap_info) else {
            continue;
        };
        let (offset_min, offset_max) = remap.os_remap.offset_bounds();
        let aabb = Aabb::from_min_max(
            Vec3::from(rest.min()) + offset_min,
            Vec3::from(rest.max()) + offset_max,
        );
        commands.entity(entity).insert((aabb, NoAutoAabb));
    }
}

/// Triggers [`VatClipFinished`] when a playing non-looping clip reaches its end.
///
/// Finish times are only recomputed for animators that changed (or when clips load),
/// so idle frames just check the pending deadlines.
pub fn track_clip_finish(
    mut commands: Commands,
    time: Res<Time>,
    clips: Res<Assets<VatAnimationClip>>,
    mut clip_events: MessageReader<AssetEvent<VatAnimationClip>>,
    all: Query<(Entity, &VatAnimator)>,
    changed: Query<(Entity, &VatAnimator), Changed<VatAnimator>>,
    mut removed: RemovedComponents<VatAnimator>,
    mut pending: Local<EntityHashMap<(f64, Handle<VatAnimationClip>)>>,
) {
    for entity in removed.read() {
        pending.remove(&entity);
    }

    let mut schedule = |(entity, animator): (Entity, &VatAnimator)| {
        let clip = animator.current_clip();
        match clips.get(clip).and_then(|c| animator.finish_time(c)) {
            Some(at) => pending.insert(entity, (at, clip.clone())),
            None => pending.remove(&entity),
        };
    };
    if clip_events.read().count() > 0 {
        all.iter().for_each(&mut schedule);
    } else {
        changed.iter().for_each(&mut schedule);
    }

    let now = time.elapsed_secs_f64();
    pending.retain(|&entity, (at, clip)| {
        if *at > now {
            return true;
        }
        commands.trigger(VatClipFinished {
            entity,
            clip: clip.clone(),
        });
        false
    });
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
