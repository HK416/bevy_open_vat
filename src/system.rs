use bevy::{
    mesh::MeshTag, pbr::ExtendedMaterial, platform::collections::HashMap, prelude::*,
    render::storage::ShaderBuffer,
};

use crate::{
    asset::{RemapInfo, VatAnimationClip},
    data::{VatAnimator, VatInstanceData, VatMaterialReady},
    material::OpenVatExtension,
};

/// 1. Auto-Setup System
/// Automatically converts StandardMaterial to VAT ExtendedMaterial when assets are ready.
pub fn auto_setup_vat_materials(
    mut commands: Commands,
    query: Query<
        (
            Entity,
            &VatAnimator,
            Option<&MeshMaterial3d<StandardMaterial>>,
        ),
        Without<VatMaterialReady>,
    >,
    children_query: Query<&Children>,
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
                let mut base_mat = std_materials
                    .get(&mat_handle.0)
                    .cloned()
                    .unwrap_or_default();

                // Workaround: In Opaque mode, prepass does not bind MaterialExtension,
                // causing the VAT vertex shader to not work.
                // Force the binding in prepass by setting it to Mask(0.0).
                // (This doesn't affect the rendering result since the actual discard threshold is 0)
                if matches!(base_mat.alpha_mode, AlphaMode::Opaque) {
                    base_mat.alpha_mode = AlphaMode::Mask(0.0);
                }

                let buffer = buffers.add(ShaderBuffer::default());
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
                .insert((MeshMaterial3d(extended_mat), VatMaterialReady, MeshTag(0)));
        }

        // Apply recursively to children (useful for SceneRoot)
        if let Ok(children) = children_query.get(entity) {
            for child in children.iter() {
                commands.entity(child).insert(animator.clone());
            }
            // Mark root as ready so we don't process it again
            commands.entity(entity).insert(VatMaterialReady);
        }
    }

    // Clean up dead materials from the cache if their original assets have been dropped
    material_cache.retain(|(mat_id, img_id, remap_id), _| {
        std_materials.contains(*mat_id) && images.contains(*img_id) && remap_infos.contains(*remap_id)
    });
}

/// 2. Update Instance Data System
/// Computes current animation data and passes it to the GPU
pub fn update_instance_data(
    mut controller_query: Query<(
        &VatAnimator,
        &mut MeshTag,
        &MeshMaterial3d<ExtendedMaterial<StandardMaterial, OpenVatExtension>>,
    )>,
    clips: Res<Assets<VatAnimationClip>>,
    mut materials: ResMut<Assets<ExtendedMaterial<StandardMaterial, OpenVatExtension>>>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
    mut last_counts: Local<
        HashMap<AssetId<ExtendedMaterial<StandardMaterial, OpenVatExtension>>, usize>,
    >,
) {
    let mut material_batches: HashMap<
        AssetId<ExtendedMaterial<StandardMaterial, OpenVatExtension>>,
        Vec<VatInstanceData>,
    > = HashMap::new();

    // Phase 1: Advance time and build GPU data grouped by material
    for (animator, mut mesh_tag, mat_handle) in controller_query.iter_mut() {
        let mat_id = mat_handle.0.id();
        let batch = material_batches.entry(mat_id).or_default();

        let index = batch.len() as u32;

        // Sync the MeshTag synchronously! No 1-frame delay.
        if mesh_tag.0 != index {
            mesh_tag.0 = index;
        }

        let Some(clip) = clips.get(&animator.current_clip) else {
            batch.push(VatInstanceData::default());
            continue;
        };

        let duration = clip.duration().unwrap_or(1.0);
        let speed = if animator.is_playing {
            animator.speed
        } else {
            0.0
        };
        let rate = speed / duration;

        // Since GPU computes time using globals.time, we don't need to manually advance time on CPU.
        // start_time is treated as a static offset.
        let offset = -(animator.start_time * rate) + animator.offset;

        batch.push(VatInstanceData {
            start_frame: clip.start_frame,
            frame_count: clip.end_frame - clip.start_frame,
            rate,
            offset,
        });
    }

    // Phase 2: Update buffers
    for (mat_id, data) in material_batches {
        let new_len = data.len();

        if let Some(mat) = materials.get(mat_id) {
            if let Some(mut buffer) = buffers.get_mut(&mat.extension.instance) {
                buffer.set_data(data);
            }
        }

        // 0.19 AssetMut fix: if buffer size changed, force BindGroup rebuild by touching the material.
        let last_len = last_counts.entry(mat_id).or_insert(0);
        if *last_len != new_len {
            *last_len = new_len;
            if let Some(mut mat_mut) = materials.get_mut(mat_id) {
                let _ = &mut *mat_mut; // Explicit DerefMut triggers AssetEvent::Modified
            }
        }
    }

    // Phase 3: Clean up dead materials from `last_counts` to prevent memory leaks
    last_counts.retain(|id, _| materials.contains(*id));
}
