# Bevy OpenVAT

[![Crates.io](https://img.shields.io/crates/v/bevy_open_vat.svg)](https://crates.io/crates/bevy_open_vat)
[![MIT/Apache 2.0](https://img.shields.io/badge/license-MIT%2FApache-blue.svg)](./LICENSE-MIT)
[![Bevy](https://img.shields.io/badge/Bevy-0.20-orange)](https://bevyengine.org/)

**Bevy OpenVAT** is a high-performance Vertex Animation Texture (VAT) plugin for the Bevy game engine. It enables rendering massive amounts of animated meshes with minimal CPU overhead by offloading animation processing to the vertex shader.

## Features

- **GPU-Powered Animation:** Decodes position and normal offsets directly in the vertex shader for maximum efficiency.
- **High Performance Instancing:** Uses `StorageBuffers` to render thousands of animated entities with few draw calls.
- **Smooth Interpolation:** Supports frame blending for fluid animation playback.
- **PBR Compatible:** Built on top of Bevy's `StandardMaterial`, fully supporting lighting, shadows, and depth pre-pass.
- **Flexible Control:** Per-entity play, pause, seek, and speed control without animation pops, plus one-shot clips with a `VatClipFinished` event.

## Installation

| Bevy Version | Crate Version |
| :--- | :--- |
| `0.20` | `0.20` |
| `0.19` | `0.19` |
| `0.18` | `0.18` |

Add the following to your `Cargo.toml`:

```toml
[dependencies]
bevy = "0.20"
bevy_open_vat = "0.20.0"
```

Bevy 0.20 requires Rust 1.97.1 or newer.

## Usage

1. Add the `OpenVatPlugin` to your Bevy app.
2. Load your JSON (`RemapInfo`), VAT texture (`Image`), and standard glTF models.
3. Add the `VatAnimator` component to the entity with the mesh and `StandardMaterial`. The plugin will automatically convert the material to a VAT-enabled material.

```rust,no_run
use bevy::prelude::*;
use bevy_open_vat::prelude::*;

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, OpenVatPlugin))
        // ... setup systems ...
        .run();
}

fn setup(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
) {
    // Load assets
    let remap_info: Handle<RemapInfo> = asset_server.load("model_remap.json");
    let vat_texture: Handle<Image> = asset_server.load("model_vat.exr");
    let clip: Handle<VatAnimationClip> = asset_server.load("model_remap.json#AnimationName");

    // Spawn entity with VAT animator
    commands.spawn((
        Mesh3d(asset_server.load("model.gltf#Mesh0/Primitive0")),
        MeshMaterial3d::<StandardMaterial>(asset_server.load("model.gltf#Material0")),
        Transform::default(),
        // Add VatAnimator to automatically convert the material
        VatAnimator::new(remap_info, vat_texture, clip),
    ));
}
```

### Playback control

Animation time advances on the GPU, so change playback through the `VatAnimator` methods.
They re-anchor at the current time, so the animation continues from where it is instead of jumping.

```rust
use bevy::prelude::*;
use bevy_open_vat::prelude::*;

fn control(time: Res<Time>, mut animators: Query<&mut VatAnimator>) {
    for mut animator in &mut animators {
        animator.pause(&time);                // hold the current frame
        animator.resume(&time);
        animator.set_speed(2.0, &time);       // negative values play backwards
        animator.seek(0.5, &time);            // seconds into the clip
        // animator.play(other_clip.clone(), &time); // switch clips and restart
    }
}
```

Use `VatAnimator::new(..).with_speed(1.5).with_elapsed(0.3)` to configure an animator at spawn,
for example to desynchronize a crowd. Clips with `"looping": false` hold their last frame and
trigger a `VatClipFinished` event on the entity when they end.

### Scenes and hierarchies

When `VatAnimator` is added to a parent such as a glTF scene root, the plugin copies it to every
descendant mesh and links them with `VatAnimatorOf`. Keep controlling the parent: its changes are
copied to the meshes automatically. To touch only the parents in a query, filter with
`Without<VatAnimatorOf>`.

### Rendering notes

* **Culling:** VAT meshes get a bounding box grown by the remap offset range, so frustum culling
  never drops a mesh whose animated pose leaves its rest-pose bounds.
* **Vertex compression:** Meshes using Bevy's vertex attribute compression (`Mesh::compress_*`)
  are supported, which can cut vertex bandwidth for large crowds.
* **Texture format:** The VAT texture is read with exact texel loads, so it works with
  non-filterable formats such as the `Rgba32Float` that EXR files load as.

### Web support

On the web, only **WebGPU** is supported. Per-instance animation data is read from a storage
buffer in the vertex shader, which WebGL2 does not have, so with Bevy's default WebGL2 backend
VAT materials fail to build and nothing is drawn.

Enable Bevy's `webgpu` feature in your app and run it in a browser with WebGPU support:

```toml
[dependencies]
bevy = { version = "0.20", features = ["webgpu"] }
```

Bevy also requests a `.meta` file for every asset on the web; the resulting 404s are harmless,
or you can skip the lookup with `AssetPlugin { meta_check: AssetMetaCheck::Never, ..default() }`.

## Asset Preparation

To use this plugin, you need exported VAT assets. Typically, this includes:

1.  **Geometry (.glb or .gltf):** A static mesh with UV2 coordinates mapped for VAT lookup.
2.  **VAT Texture (.exr):** High-precision texture encoding vertex position (and optionally normal) offsets.
3.  **Remap Info (.json):** Metadata containing animation frame counts, bounding box info, and clip names.

> **Note:** Ensure your VAT textures are exported with settings compatible with your shader (e.g., coordinate system handling).

### Preparation
1. Install the **OpenVAT** plugin for Blender.

### Conversion Workflow
![Asset Workflow Example](https://raw.githubusercontent.com/hk416/bevy_open_vat/main/assets/image.png)

1.  **Load Model:** Open Blender and load the model file containing your animations.
2.  **NLA Setup:** In the NLA (Non-Linear Animation) editor, combine the animations you want to extract into a single continuous sequence.
3.  **Open Panel:** Open the OpenVAT panel (Press `N`).
4.  **Configure:** Select the target model and configuration options.
    > **Important:** You must manually set the **frame range** at the bottom of the panel to match your NLA sequence.
5.  **Export:** 1.  Generate/Export the VAT texture using the plugin.
    2.  Select the **preview model** generated by OpenVAT in the viewport.
    3.  Export this preview model to **glTF** format.

### Export Notes

* **Materials:** Models extracted directly by the OpenVAT tool often have stripped material data. We recommend exporting the generated **preview model** as glTF to maintain material slots.
* **glTF Settings:** When exporting the glTF, ensure the following data is **EXCLUDED** (unchecked):
    * Animation Data
    * Skinning Data

## License

This project is licensed under either of

 * Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
 * MIT license ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.