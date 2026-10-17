use bevy::{
    asset::asset_changed::AssetChanged,
    camera::{
        primitives::{Aabb, MeshAabb},
        visibility::NoAutoAabb,
    },
    ecs::{
        entity::{EntityHashMap, EntityHashSet},
        relationship::RelationshipTarget,
    },
    mesh::MeshTag,
    pbr::ExtendedMaterial,
    platform::collections::{HashMap, HashSet},
    prelude::*,
    render::storage::ShaderBuffer,
};

use crate::{
    asset::{RemapInfo, VatAnimationClip},
    data::{
        VatAnimatedMeshes, VatAnimator, VatAnimatorOf, VatClipFinished, VatInstanceData,
        VatMaterialReady,
    },
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

/// Writes the GPU instance data of every VAT mesh whose playback state changed.
///
/// Each mesh owns a stable slot (its `MeshTag`) in its material's instance buffer, handed out
/// by a per-material free list so despawning one mesh never moves the others. Only dirty slots
/// are rewritten: a mesh is dirty when its material or link changed, its animator (own or
/// linked) changed or went away, its clip (re)loaded, or the shader clock wrapped.
pub fn update_instance_data(
    mut meshes: Query<(
        Entity,
        &mut MeshTag,
        &MeshMaterial3d<VatStandardMaterial>,
        Option<&VatAnimator>,
        Option<&VatAnimatorOf>,
    )>,
    changed_meshes: Query<
        Entity,
        Or<(
            Changed<MeshMaterial3d<VatStandardMaterial>>,
            Changed<VatAnimatorOf>,
        )>,
    >,
    changed_sources: Query<(Entity, Option<&VatAnimatedMeshes>), Changed<VatAnimator>>,
    sources: Query<(Entity, &VatAnimator, Option<&VatAnimatedMeshes>)>,
    animated: Query<&VatAnimatedMeshes>,
    mut removed_meshes: RemovedComponents<MeshMaterial3d<VatStandardMaterial>>,
    mut removed_links: RemovedComponents<VatAnimatorOf>,
    mut removed_sources: RemovedComponents<VatAnimator>,
    mut clip_events: MessageReader<AssetEvent<VatAnimationClip>>,
    time: Res<Time>,
    mut last_wrap_base: Local<f64>,
    clips: Res<Assets<VatAnimationClip>>,
    materials: Res<Assets<VatStandardMaterial>>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
    mut slots: Local<VatSlots>,
) {
    let slots = &mut *slots;
    let mut dirty = EntityHashSet::default();
    // Slot writes grouped by material: (slot, data).
    let mut writes: HashMap<AssetId<VatStandardMaterial>, Vec<(u32, VatInstanceData)>> =
        HashMap::new();

    // Gone meshes free their slot and leave it idle.
    let mut any_removed = false;
    for entity in removed_meshes.read() {
        any_removed = true;
        if let Some((mat_id, slot)) = slots.by_entity.remove(&entity) {
            if let Some(pool) = slots.by_material.get_mut(&mat_id) {
                pool.free(slot);
            }
            writes
                .entry(mat_id)
                .or_default()
                .push((slot, VatInstanceData::default()));
        }
    }
    if any_removed {
        // Pools of materials that were dropped since the last removal go with them.
        slots
            .by_material
            .retain(|mat_id, _| materials.contains(*mat_id));
    }
    // A removed animator leaves its own mesh and its linked meshes idle. A despawned source
    // also drops the links on its meshes, which `removed_links` catches.
    for source in removed_sources.read() {
        dirty.insert(source);
        if let Ok(linked) = animated.get(source) {
            dirty.extend(linked.iter());
        }
    }
    dirty.extend(removed_links.read());

    // New or re-materialised meshes get a slot in their material's buffer.
    for entity in &changed_meshes {
        let Ok((_, mut tag, mat_handle, ..)) = meshes.get_mut(entity) else {
            continue;
        };
        let mat_id = mat_handle.0.id();
        let slot = match slots.by_entity.get(&entity).copied() {
            Some((old_mat, slot)) if old_mat == mat_id => slot,
            old => {
                if let Some((old_mat, slot)) = old {
                    if let Some(pool) = slots.by_material.get_mut(&old_mat) {
                        pool.free(slot);
                    }
                    writes
                        .entry(old_mat)
                        .or_default()
                        .push((slot, VatInstanceData::default()));
                }
                let slot = slots.by_material.entry(mat_id).or_default().alloc();
                slots.by_entity.insert(entity, (mat_id, slot));
                slot
            }
        };
        if tag.value != slot {
            tag.value = slot;
        }
        dirty.insert(entity);
    }

    // A changed animator dirties its own mesh and every mesh linked to it.
    for (source, linked) in &changed_sources {
        dirty.insert(source);
        dirty.extend(linked.into_iter().flat_map(VatAnimatedMeshes::iter));
    }

    // A clip that (re)loaded dirties the sources playing it.
    let loaded: HashSet<_> = clip_events.read().filter_map(loaded_clip).collect();
    if !loaded.is_empty() {
        for (source, animator, linked) in &sources {
            if loaded.contains(&animator.current_clip().id()) {
                dirty.insert(source);
                dirty.extend(linked.into_iter().flat_map(VatAnimatedMeshes::iter));
            }
        }
    }

    // `globals.time` wraps; the CPU folds the wrap into every offset, so rewrite everything.
    let wrap_base = time.elapsed_secs_f64() - time.elapsed_secs_wrapped_f64();
    if wrap_base != *last_wrap_base {
        *last_wrap_base = wrap_base;
        dirty.extend(meshes.iter().map(|(entity, ..)| entity));
    }

    for entity in dirty {
        // Sources that are not meshes have nothing to write.
        let Ok((_, tag, mat_handle, own, link)) = meshes.get(entity) else {
            continue;
        };
        let animator = own.or_else(|| {
            link.and_then(|link| sources.get(link.0).ok().map(|(_, animator, _)| animator))
        });
        let data = animator
            .and_then(|animator| {
                clips
                    .get(animator.current_clip())
                    .map(|clip| animator.instance_data(clip, wrap_base))
            })
            .unwrap_or_default();
        writes
            .entry(mat_handle.0.id())
            .or_default()
            .push((tag.value, data));
    }

    // Bevy re-uploads a buffer whose asset changed; same size is a plain write, a bigger one
    // is reallocated and the material's bind group rebuilt.
    for (mat_id, entries) in writes {
        let Some(len) = slots.by_material.get(&mat_id).map(|pool| pool.len as usize) else {
            continue;
        };
        let Some(mut buffer) = materials
            .get(mat_id)
            .and_then(|mat| buffers.get_mut(&mat.extension.instance))
        else {
            continue;
        };
        let have = buffer.cast_slice::<VatInstanceData>().map_or(0, <[_]>::len);
        if have < len {
            // Buffers only grow, by powers of two; they are not shrunk when a crowd thins out.
            let grow = len.next_power_of_two() - have;
            buffer.extend_from_slice(&vec![VatInstanceData::default(); grow]);
        }
        let Some(slice) = buffer.cast_slice_mut::<VatInstanceData>() else {
            continue;
        };
        for (slot, data) in entries {
            slice[slot as usize] = data;
        }
    }
}

/// The id of a clip that just became usable, if `event` says so.
fn loaded_clip(event: &AssetEvent<VatAnimationClip>) -> Option<AssetId<VatAnimationClip>> {
    match event {
        AssetEvent::Added { id }
        | AssetEvent::Modified { id }
        | AssetEvent::LoadedWithDependencies { id } => Some(*id),
        _ => None,
    }
}

/// Instance slots handed out per VAT material, kept across frames by `update_instance_data`.
#[derive(Default)]
pub struct VatSlots {
    by_material: HashMap<AssetId<VatStandardMaterial>, SlotPool>,
    by_entity: EntityHashMap<(AssetId<VatStandardMaterial>, u32)>,
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
    mut clip_events: MessageReader<AssetEvent<VatAnimationClip>>,
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
    changed.iter().for_each(&mut schedule);
    let loaded: HashSet<_> = clip_events.read().filter_map(loaded_clip).collect();
    if !loaded.is_empty() {
        all.iter()
            .filter(|(_, animator)| loaded.contains(&animator.current_clip().id()))
            .for_each(&mut schedule);
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

/// Free-list allocator for instance slots in one material's buffer.
#[derive(Default)]
struct SlotPool {
    free: Vec<u32>,
    len: u32,
}

impl SlotPool {
    fn alloc(&mut self) -> u32 {
        self.free.pop().unwrap_or_else(|| {
            self.len += 1;
            self.len - 1
        })
    }

    fn free(&mut self, slot: u32) {
        self.free.push(slot);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// Spawns an entity that is both the animator and the mesh.
    fn spawn_vat(
        app: &mut App,
        animator: &VatAnimator,
        material: &Handle<StandardMaterial>,
    ) -> Entity {
        app.world_mut()
            .spawn((animator.clone(), MeshMaterial3d(material.clone())))
            .id()
    }

    fn tag_of(app: &App, entity: Entity) -> u32 {
        app.world().get::<MeshTag>(entity).unwrap().value
    }

    /// The whole instance buffer of `mesh`'s material.
    fn buffer_of(app: &App, mesh: Entity) -> Vec<VatInstanceData> {
        let world = app.world();
        let material = world
            .get::<MeshMaterial3d<VatStandardMaterial>>(mesh)
            .unwrap();
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
            .unwrap()
            .to_vec()
    }

    #[test]
    fn test_slot_pool_reuses_freed_slots() {
        let mut p = SlotPool::default();
        assert_eq!((p.alloc(), p.alloc(), p.alloc()), (0, 1, 2));
        p.free(1);
        assert_eq!(p.alloc(), 1);
        assert_eq!(p.alloc(), 3);
        assert_eq!(p.len, 4);
    }

    #[test]
    fn test_slots_survive_despawn() {
        let mut app = test_app();
        let animator = animator(&mut app);
        let material = std_material(&mut app);
        let [a, b, c] = [(); 3].map(|_| spawn_vat(&mut app, &animator, &material));
        app.update();
        app.update();
        let mut tags = [a, b, c].map(|e| tag_of(&app, e));
        tags.sort();
        assert_eq!(tags, [0, 1, 2]);
        let (tag_a, tag_b, tag_c) = (tag_of(&app, a), tag_of(&app, b), tag_of(&app, c));

        app.world_mut().despawn(b);
        app.update();
        assert_eq!(
            (tag_of(&app, a), tag_of(&app, c)),
            (tag_a, tag_c),
            "slots must not shift"
        );
        assert_eq!(
            buffer_of(&app, a)[tag_b as usize],
            VatInstanceData::default()
        );

        let d = spawn_vat(&mut app, &animator, &material);
        app.update();
        app.update();
        assert_eq!(tag_of(&app, d), tag_b, "freed slot is reused");
    }

    #[test]
    fn test_only_changed_instance_is_rewritten() {
        let mut app = test_app();
        let animator = animator(&mut app);
        let material = std_material(&mut app);
        let a = spawn_vat(&mut app, &animator, &material);
        let b = spawn_vat(&mut app, &animator, &material);
        app.update();
        app.update();
        app.update();
        let (tag_a, tag_b) = (tag_of(&app, a), tag_of(&app, b));

        // Poison B's slot behind the plugin's back; a dirty-only writer leaves it alone.
        let sentinel = VatInstanceData {
            start_frame: 12345,
            ..default()
        };
        let buffer = app
            .world()
            .resource::<Assets<VatStandardMaterial>>()
            .get(
                &app.world()
                    .get::<MeshMaterial3d<VatStandardMaterial>>(a)
                    .unwrap()
                    .0,
            )
            .unwrap()
            .extension
            .instance
            .clone();
        app.world_mut()
            .resource_mut::<Assets<ShaderBuffer>>()
            .get_mut(&buffer)
            .unwrap()
            .cast_slice_mut::<VatInstanceData>()
            .unwrap()[tag_b as usize] = sentinel;

        let time = *app.world().resource::<Time>();
        app.world_mut()
            .get_mut::<VatAnimator>(a)
            .unwrap()
            .set_speed(2.0, &time);
        app.update();
        let after = buffer_of(&app, a);
        assert_eq!(after[tag_a as usize].rate, 1.0);
        assert_eq!(after[tag_b as usize], sentinel);
    }

    #[test]
    fn test_clip_load_updates_its_users() {
        let mut app = test_app();
        let mut animator_a = animator(&mut app);
        let material = std_material(&mut app);
        let pending = app
            .world_mut()
            .resource_mut::<Assets<VatAnimationClip>>()
            .reserve_handle();
        let animator_b = animator_a.clone();
        animator_a.play(pending.clone(), app.world().resource::<Time>());
        let a = spawn_vat(&mut app, &animator_a, &material);
        let b = spawn_vat(&mut app, &animator_b, &material);
        app.update();
        app.update();
        assert_eq!(instance_at(&app, a), VatInstanceData::default());
        assert_eq!(instance_at(&app, b).frame_count, 20);

        app.world_mut()
            .resource_mut::<Assets<VatAnimationClip>>()
            .insert(
                &pending,
                VatAnimationClip {
                    start_frame: 5,
                    end_frame: 25,
                    frame_rate: 10.0,
                    looping: true,
                },
            )
            .unwrap();
        app.update();
        app.update();
        assert_eq!(instance_at(&app, a).start_frame, 5);
        assert_eq!(instance_at(&app, a).frame_count, 20);
    }

    #[test]
    fn test_idle_frame_does_not_touch_buffers() {
        #[derive(Resource, Default)]
        struct Touched(bool);
        let mut app = test_app();
        app.init_resource::<Touched>().add_systems(
            Last,
            |buffers: Res<Assets<ShaderBuffer>>, mut touched: ResMut<Touched>| {
                touched.0 = buffers.is_changed();
            },
        );
        let animator = animator(&mut app);
        let material = std_material(&mut app);
        spawn_vat(&mut app, &animator, &material);
        app.update();
        app.update();
        app.update();
        assert!(
            !app.world().resource::<Touched>().0,
            "idle frame must not write"
        );
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
