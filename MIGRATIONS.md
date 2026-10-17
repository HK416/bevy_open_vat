# Migration Guide

## 0.19 → 0.20

Targets Bevy 0.20 (Rust 1.97.1 or newer). The `exr` feature, enabled by default, turns on Bevy's
EXR loader; disable it if your VAT textures use another format.

### `VatAnimator` fields are private

Playback time advances on the GPU, so the plugin needs to re-anchor the animator whenever its
state changes. Construct it and drive it through methods instead of writing fields:

```rust
// 0.19
VatAnimator {
    remap_info,
    vat_texture,
    current_clip: clip,
    is_playing: true,
    speed: 1.0,
    ..default()
}

// 0.20
VatAnimator::new(remap_info, vat_texture, clip)
    .with_speed(1.0)        // optional
    .with_elapsed(0.3)      // optional: start partway into the clip
    // .paused()            // optional
```

| 0.19 field write | 0.20 |
| :-- | :-- |
| `animator.current_clip = clip` | `animator.play(clip, &time)` |
| `animator.is_playing = false` / `true` | `animator.pause(&time)` / `animator.resume(&time)` |
| `animator.speed = s` | `animator.set_speed(s, &time)` |
| `animator.start_time = ..`, `animator.offset = ..` | `animator.seek(seconds, &time)` |

Read state with `speed()`, `is_playing()`, `elapsed(&time)` and `current_clip()`. The methods
take `&Time` (`Res<Time>`) so the clip continues from its current pose instead of jumping.

### Hierarchies: one animator per scene, linked meshes

Add `VatAnimator` to the entity you spawn (for example the glTF `SceneRoot`). Every descendant
mesh with a `StandardMaterial` is converted and gets a `VatAnimatorOf` component pointing at that
ancestor; the ancestor gets the matching `VatAnimatedMeshes` relationship target. Meshes no
longer carry an animator of their own, so `Query<&mut VatAnimator>` only sees the roots. Meshes
spawned under the root later are linked when they appear.

### Clip end event

Clips with `"looping": false` hold their last frame and trigger `VatClipFinished { entity, clip }`
on the entity that owns the animator. Observe it with `commands.add_observer` or an entity
observer; previously non-looping clips wrapped like looping ones.

### Textures are compacted

A VAT texture loaded as `Rgba32Float` (every EXR) is converted to `Rgba16Unorm` on devices with
the `TEXTURE_FORMAT_16BIT_NORM` wgpu feature (native platforms), and its CPU copy is released
after upload on every platform. Consequences:

* `Assets<Image>::get(..).data` is `None` once the texture is on the GPU; read the data before
  the animator that uses it is spawned if you need it.
* Procedural `Rgba32Float` textures are compacted too. Store values in [0, 1] and put the scale
  in `OsRemap { min, max }`, as `examples/simple_test.rs` does.
* Textures in any other format (KTX2, `Rgba16Float`, ...) are used unchanged.

### Material snapshot

The VAT material is built once from the `StandardMaterial` and `RemapInfo` an entity has when
it is converted. Later changes to those assets are not propagated; insert a new
`MeshMaterial3d<StandardMaterial>` on the entity to rebuild it.

### Other changes

* VAT meshes receive `NoAutoAabb` and an `Aabb` grown by the remap offset range, so frustum
  culling no longer drops meshes whose animation leaves the rest-pose bounds.
* The loader warns about clips whose frame range or frame rate is invalid, and the plugin warns
  once per mesh that has no `Mesh::ATTRIBUTE_UV_1`.
* `VatAnimationClip::frame_count()`, `RemapInfo::offset_bounds()` and
  `VatAnimationClip::validate()` are new.
* Web builds need the `webgpu` backend; WebGL2 cannot read the per-instance storage buffer.

## 0.18 → 0.19

Targets Bevy 0.19.

### Automatic material setup

The plugin converts a `StandardMaterial` for you. Spawn the mesh with its normal material and a
`VatAnimator`; do not build `ExtendedMaterial<StandardMaterial, OpenVatExtension>` or the instance
`ShaderStorageBuffer` by hand anymore.

```rust
// 0.18
let material = vat_materials.add(ExtendedMaterial {
    base: StandardMaterial::default(),
    extension: OpenVatExtension { vat_texture, min_pos, max_pos, frame_count, .. },
});
commands.spawn((
    Mesh3d(mesh),
    MeshMaterial3d(material),
    VatAnimationController { remap_info, current_clip: "Run".to_string(), ..default() },
));

// 0.19
commands.spawn((
    Mesh3d(mesh),
    MeshMaterial3d(standard_material),
    VatAnimator { remap_info, vat_texture, current_clip: clip, is_playing: true, speed: 1.0, ..default() },
));
```

Converted entities get the `VatMaterialReady` marker.

### `VatAnimationController` → `VatAnimator`

The component was renamed. The VAT texture handle moved from `OpenVatExtension::vat_texture`
to `VatAnimator::vat_texture`.

### Clips are assets

`current_clip` is a `Handle<VatAnimationClip>` instead of a clip name. Load a clip as a labeled
sub-asset of the remap file:

```rust
let clip: Handle<VatAnimationClip> = asset_server.load("model-remap_info.json#Run");
```

### Systems

`update_anim_controller` was replaced by `auto_setup_vat_materials`. If you ordered systems
relative to it, use the new name.
