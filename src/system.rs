use bevy::{
    asset::asset_changed::AssetChanged,
    camera::{
        primitives::{Aabb, MeshAabb},
        visibility::NoAutoAabb,
    },
    ecs::entity::EntityHashMap,
    mesh::MeshTag,
    pbr::ExtendedMaterial,
    platform::collections::{HashMap, HashSet},
    prelude::*,
    render::storage::ShaderBuffer,
};

use crate::{
    asset::{RemapInfo, VatAnimationClip},
    data::{VatAnimator, VatAnimatorOf, VatClipFinished, VatInstanceData, VatMaterialReady},
    material::{OpenVatExtension, VatStandardMaterial},
};

/// Links meshes to the `VatAnimator` that drives them.
///
/// A new `StandardMaterial` mesh is linked to its nearest ancestor animator, and a new animator
/// is linked to the `StandardMaterial` meshes below it. Either side may arrive first: a glTF
/// scene spawns its meshes a few frames after the root, and an animator may be added to an
/// existing hierarchy. Meshes only ever get a `VatAnimatorOf` link, never a copy of the animator.
pub fn link_vat_meshes(
    mut commands: Commands,
    new_meshes: Query<
        Entity,
        (
            Added<MeshMaterial3d<StandardMaterial>>,
            Without<VatAnimator>,
        ),
    >,
    new_animators: Query<Entity, Added<VatAnimator>>,
    parents: Query<&ChildOf>,
    children: Query<&Children>,
    animators: Query<(), With<VatAnimator>>,
    std_meshes: Query<(), (With<MeshMaterial3d<StandardMaterial>>, Without<VatAnimator>)>,
) {
    let nearest_animator = |mesh: Entity| {
        parents
            .iter_ancestors(mesh)
            .find(|ancestor| animators.contains(*ancestor))
    };
    for mesh in &new_meshes {
        if let Some(source) = nearest_animator(mesh) {
            commands.entity(mesh).insert(VatAnimatorOf(source));
        }
    }
    for source in &new_animators {
        for mesh in children.iter_descendants(source) {
            // Skip meshes driven by a nested animator closer to them.
            if std_meshes.contains(mesh) && nearest_animator(mesh) == Some(source) {
                commands.entity(mesh).insert(VatAnimatorOf(source));
            }
        }
    }
}

type MaterialKey = (
    AssetId<StandardMaterial>,
    AssetId<Image>,
    AssetId<RemapInfo>,
);

/// Converts the `StandardMaterial` of meshes driven by a `VatAnimator` (their own, or the one
/// they are linked to with `VatAnimatorOf`) into a VAT material.
///
/// One VAT material is shared per (material, texture, remap) combination; the cache holds only
/// asset IDs, so a material no entity uses anymore is dropped and recreated on demand.
pub fn auto_setup_vat_materials(
    mut commands: Commands,
    query: Query<
        (
            Entity,
            Option<&VatAnimator>,
            Option<&VatAnimatorOf>,
            &MeshMaterial3d<StandardMaterial>,
        ),
        (
            Without<VatMaterialReady>,
            Or<(With<VatAnimator>, With<VatAnimatorOf>)>,
        ),
    >,
    sources: Query<&VatAnimator>,
    remap_infos: Res<Assets<RemapInfo>>,
    std_materials: Res<Assets<StandardMaterial>>,
    mut vat_materials: ResMut<Assets<VatStandardMaterial>>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
    mut material_cache: Local<HashMap<MaterialKey, AssetId<VatStandardMaterial>>>,
) {
    for (entity, own, link, mat_handle) in &query {
        let Some(animator) = own.or_else(|| link.and_then(|link| sources.get(link.0).ok())) else {
            continue;
        };
        let Some(remap) = remap_infos.get(&animator.remap_info) else {
            continue;
        };
        let os_remap = &remap.os_remap;

        let cache_key = (
            mat_handle.0.id(),
            animator.vat_texture.id(),
            animator.remap_info.id(),
        );
        let cached = material_cache
            .get(&cache_key)
            .and_then(|id| vat_materials.get_strong_handle(*id));
        let extended_mat = match cached {
            Some(mat) => mat,
            None => {
                // Wait for the material to load; a glTF material usually arrives after the
                // small remap JSON, and converting a missing material would bake the default
                // white material into the cache.
                let Some(base_mat) = std_materials.get(&mat_handle.0).cloned() else {
                    continue;
                };

                let buffer = buffers.add(ShaderBuffer::from(vec![VatInstanceData::default()]));
                let mat = vat_materials.add(ExtendedMaterial {
                    base: base_mat,
                    extension: OpenVatExtension {
                        vat_texture: animator.vat_texture.clone(),
                        min_pos: Vec3::from_array(os_remap.min),
                        max_pos: Vec3::from_array(os_remap.max),
                        frame_count: os_remap.frames,
                        instance: buffer,
                    },
                });
                material_cache.insert(cache_key, mat.id());
                mat
            }
        };

        commands
            .entity(entity)
            .remove::<MeshMaterial3d<StandardMaterial>>()
            .insert((
                MeshMaterial3d(extended_mat),
                VatMaterialReady,
                MeshTag::new(0),
            ));
    }

    // Drop cache entries whose source assets are gone, or whose VAT material no entity holds.
    material_cache.retain(|(mat_id, _, remap_id), vat_id| {
        std_materials.contains(*mat_id)
            && remap_infos.contains(*remap_id)
            && vat_materials.contains(*vat_id)
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
        Option<&VatAnimator>,
        Option<&VatAnimatorOf>,
        &mut MeshTag,
        &MeshMaterial3d<VatStandardMaterial>,
    )>,
    sources: Query<&VatAnimator>,
    changed: Query<
        (),
        Or<(
            Changed<VatAnimator>,
            Changed<MeshMaterial3d<VatStandardMaterial>>,
        )>,
    >,
    mut removed_animators: RemovedComponents<VatAnimator>,
    mut removed_materials: RemovedComponents<MeshMaterial3d<VatStandardMaterial>>,
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
    // Asset events for clips are only flushed after this system runs, so watch the collection
    // itself: a clip that finished loading this frame is picked up this frame.
    if changed.is_empty() && !removed && !clips.is_changed() && !time_wrapped {
        return;
    }

    let mut sorted_entities: Vec<_> = controller_query.iter_mut().collect();
    sorted_entities.sort_by_key(|(entity, ..)| *entity);

    let mut material_batches: HashMap<AssetId<VatStandardMaterial>, Vec<VatInstanceData>> =
        HashMap::new();

    // Build GPU data grouped by material in a deterministic order
    for (_, own, link, mut mesh_tag, mat_handle) in sorted_entities.into_iter() {
        let batch = material_batches.entry(mat_handle.0.id()).or_default();

        let index = batch.len() as u32;
        if mesh_tag.value != index {
            mesh_tag.value = index;
        }

        let animator = own.or_else(|| link.and_then(|link| sources.get(link.0).ok()));
        batch.push(
            animator
                .and_then(|animator| {
                    clips
                        .get(animator.current_clip())
                        .map(|clip| animator.instance_data(clip, wrap_base))
                })
                .unwrap_or_default(),
        );
    }

    // Sync every VAT material's buffer; materials with no active instances get a single default entry.
    // Buffers are only written when their contents change.
    // Bevy rebuilds the material bind group by itself when a buffer is reallocated.
    let idle = [VatInstanceData::default()];
    for (mat_id, mat) in materials.iter() {
        let data = material_batches
            .get(&mat_id)
            .map_or(&idle[..], Vec::as_slice);
        let handle = &mat.extension.instance;
        if !buffers
            .get(handle)
            .is_some_and(|buffer| needs_write(buffer, data))
        {
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
    query: Query<
        (
            Entity,
            &Mesh3d,
            Option<&VatAnimator>,
            Option<&VatAnimatorOf>,
        ),
        (
            With<VatMaterialReady>,
            // `NoAutoAabb` stops Bevy from refreshing the bounds, so refresh them here when the
            // mesh is swapped or its asset changes.
            Or<(Without<NoAutoAabb>, Changed<Mesh3d>, AssetChanged<Mesh3d>)>,
        ),
    >,
    sources: Query<&VatAnimator>,
    mut warned_meshes: Local<HashSet<AssetId<Mesh>>>,
) {
    for (entity, mesh, own, link) in &query {
        let Some(animator) = own.or_else(|| link.and_then(|link| sources.get(link.0).ok())) else {
            continue;
        };
        let Some(mesh_asset) = meshes.get(mesh) else {
            continue;
        };
        // Once per mesh asset, not per entity or per frame: a crowd shares one mesh, and this
        // entity is revisited every frame until its remap loads.
        if !mesh_asset.contains_attribute(Mesh::ATTRIBUTE_UV_1) && warned_meshes.insert(mesh.id()) {
            warn!(
                "VAT mesh {:?} on {entity} has no UV1 (Mesh::ATTRIBUTE_UV_1); the VAT shader cannot compile without it",
                mesh.id()
            );
        }
        let Some(rest) = mesh_asset.get_aabb() else {
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
    if clips.is_changed() {
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

    /// A render-less app running the plugin's systems. `Shader` and `ShaderBuffer` assets are
    /// normally registered by the render plugins the plugin's `build` relies on.
    fn test_app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Shader>()
            .init_asset::<Mesh>()
            .init_asset::<Image>()
            .init_asset::<StandardMaterial>()
            .init_asset::<ShaderBuffer>()
            .add_plugins(crate::plugin::OpenVatPlugin);
        app
    }

    fn remap_handle(app: &mut App) -> Handle<RemapInfo> {
        app.world_mut()
            .resource_mut::<Assets<RemapInfo>>()
            .add(RemapInfo {
                os_remap: crate::asset::OsRemap {
                    min: [-1.0; 3],
                    max: [1.0; 3],
                    frames: 4,
                },
                animations: default(),
            })
    }

    /// An animator over the test remap, a dummy texture and a loaded 2-second clip.
    fn animator(app: &mut App) -> VatAnimator {
        let remap = remap_handle(app);
        let image = app
            .world_mut()
            .resource_mut::<Assets<Image>>()
            .add(Image::default());
        let clip = app
            .world_mut()
            .resource_mut::<Assets<VatAnimationClip>>()
            .add(VatAnimationClip {
                start_frame: 0,
                end_frame: 20,
                frame_rate: 10.0,
                looping: true,
            });
        VatAnimator::new(remap, image, clip)
    }

    fn std_material(app: &mut App) -> Handle<StandardMaterial> {
        app.world_mut()
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial::default())
    }

    fn spawn_std_mesh(app: &mut App, parent: Option<Entity>) -> Entity {
        let material = std_material(app);
        let mut entity = app.world_mut().spawn(MeshMaterial3d(material));
        if let Some(parent) = parent {
            entity.insert(ChildOf(parent));
        }
        entity.id()
    }

    fn has<C: Component>(app: &App, entity: Entity) -> bool {
        app.world().get::<C>(entity).is_some()
    }

    /// The GPU instance data at `mesh`'s slot in its material's buffer.
    fn instance_at(app: &App, mesh: Entity) -> VatInstanceData {
        let world = app.world();
        let material = world
            .get::<MeshMaterial3d<VatStandardMaterial>>(mesh)
            .expect("mesh has a VAT material");
        let tag = world
            .get::<MeshTag>(mesh)
            .expect("mesh has a MeshTag")
            .value;
        let buffer = world
            .resource::<Assets<VatStandardMaterial>>()
            .get(&material.0)
            .unwrap()
            .extension
            .instance
            .clone();
        world
            .resource::<Assets<ShaderBuffer>>()
            .get(&buffer)
            .unwrap()
            .cast_slice::<VatInstanceData>()
            .unwrap()[tag as usize]
    }

    fn assert_linked_to(app: &App, mesh: Entity, source: Entity) {
        assert_eq!(
            app.world().get::<VatAnimatorOf>(mesh).map(|l| l.0),
            Some(source),
            "mesh must point at its source animator"
        );
        assert!(has::<MeshMaterial3d<VatStandardMaterial>>(app, mesh));
        assert!(!has::<MeshMaterial3d<StandardMaterial>>(app, mesh));
        assert!(
            !has::<VatAnimator>(app, mesh),
            "mesh must not get a copy of the animator"
        );
    }

    #[test]
    fn test_scene_meshes_link_to_root_animator() {
        let mut app = test_app();
        let animator = animator(&mut app);
        let root = app.world_mut().spawn(animator).id();
        let container = app.world_mut().spawn(ChildOf(root)).id();
        let mesh = spawn_std_mesh(&mut app, Some(container));

        app.update();
        app.update();
        assert_linked_to(&app, mesh, root);
        assert!(!has::<VatAnimator>(&app, container));
    }

    #[test]
    fn test_mesh_added_later_is_linked() {
        let mut app = test_app();
        let animator = animator(&mut app);
        let root = app.world_mut().spawn(animator).id();
        // Some unrelated child first (a blob shadow, a collider...), then the mesh arrives
        // later, as a glTF scene does.
        app.world_mut().spawn(ChildOf(root));
        app.update();
        let mesh = spawn_std_mesh(&mut app, Some(root));

        app.update();
        app.update();
        assert_linked_to(&app, mesh, root);
    }

    #[test]
    fn test_animator_added_later_links_existing_meshes() {
        let mut app = test_app();
        let root = app.world_mut().spawn_empty().id();
        let mesh = spawn_std_mesh(&mut app, Some(root));
        app.update();
        assert!(has::<MeshMaterial3d<StandardMaterial>>(&app, mesh));

        let animator = animator(&mut app);
        app.world_mut().entity_mut(root).insert(animator);
        app.update();
        app.update();
        assert_linked_to(&app, mesh, root);
    }

    #[test]
    fn test_unrelated_mesh_is_untouched() {
        let mut app = test_app();
        let animator = animator(&mut app);
        let root = app.world_mut().spawn(animator).id();
        let material = std_material(&mut app);
        app.world_mut()
            .spawn((MeshMaterial3d(material.clone()), ChildOf(root)));
        let bystander = app.world_mut().spawn(MeshMaterial3d(material)).id();

        app.update();
        app.update();
        assert!(has::<MeshMaterial3d<StandardMaterial>>(&app, bystander));
        assert!(!has::<VatAnimatorOf>(&app, bystander));
    }

    #[test]
    fn test_source_change_reaches_linked_mesh() {
        let mut app = test_app();
        let animator = animator(&mut app);
        let root = app.world_mut().spawn(animator).id();
        let mesh = spawn_std_mesh(&mut app, Some(root));
        app.update();
        app.update();
        assert_linked_to(&app, mesh, root);

        let time = *app.world().resource::<Time>();
        app.world_mut()
            .get_mut::<VatAnimator>(root)
            .unwrap()
            .set_speed(2.0, &time);
        app.update();
        // 20 frames at 10 fps is a 2 s clip; rate is speed / duration.
        assert_eq!(instance_at(&app, mesh).rate, 1.0);
    }

    #[test]
    fn test_waits_for_standard_material() {
        let mut app = test_app();
        let remap = remap_handle(&mut app);
        let image = app
            .world_mut()
            .resource_mut::<Assets<Image>>()
            .add(Image::default());
        let pending = app
            .world_mut()
            .resource_mut::<Assets<StandardMaterial>>()
            .reserve_handle();
        let entity = app
            .world_mut()
            .spawn((
                VatAnimator::new(remap, image, Handle::default()),
                MeshMaterial3d(pending.clone()),
            ))
            .id();

        app.update();
        assert!(
            app.world()
                .get::<MeshMaterial3d<StandardMaterial>>(entity)
                .is_some(),
            "must keep the original material until it is loaded"
        );
        assert!(
            app.world()
                .get::<MeshMaterial3d<VatStandardMaterial>>(entity)
                .is_none()
        );

        let red = Color::srgb(1.0, 0.0, 0.0);
        app.world_mut()
            .resource_mut::<Assets<StandardMaterial>>()
            .insert(
                &pending,
                StandardMaterial {
                    base_color: red,
                    ..default()
                },
            )
            .unwrap();
        app.update();
        let vat = app
            .world()
            .get::<MeshMaterial3d<VatStandardMaterial>>(entity)
            .expect("converted once the material is loaded");
        let materials = app.world().resource::<Assets<VatStandardMaterial>>();
        assert_eq!(materials.get(&vat.0).unwrap().base.base_color, red);
    }
}
