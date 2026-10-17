use bevy::{
    asset::RenderAssetUsages,
    platform::collections::HashSet,
    prelude::*,
    render::{render_resource::TextureFormat, renderer::RenderDevice, settings::WgpuFeatures},
};

use crate::{asset::RemapInfo, data::VatAnimator, material::VatStandardMaterial};

/// Converts an `Rgba32Float` image with a CPU copy to `Rgba16Unorm` and marks it render-only.
///
/// EXR files load as 16 bytes per texel plus a main-world copy. OpenVAT data is normalized to
/// [0, 1], so 16-bit unorm keeps more precision than the half floats the file held at half the
/// size, and `RENDER_WORLD` lets Bevy drop the CPU copy after upload. Values outside [0, 1] are
/// clamped. Returns `false`, leaving the image untouched, for any other format or without data.
pub fn compact_vat_image(image: &mut Image) -> bool {
    if image.texture_descriptor.format != TextureFormat::Rgba32Float {
        return false;
    }
    let Some(data) = image.data.as_ref() else {
        return false;
    };
    let compact: Vec<u16> = bytemuck::cast_slice::<u8, f32>(data)
        .iter()
        .map(|v| (v.clamp(0.0, 1.0) * 65535.0).round() as u16)
        .collect();
    image.data = Some(bytemuck::cast_slice(&compact).to_vec());
    image.texture_descriptor.format = TextureFormat::Rgba16Unorm;
    image.asset_usage = RenderAssetUsages::RENDER_WORLD;
    true
}

/// Marks an `Rgba32Float` image render-only without converting it, for devices that cannot
/// create 16-bit normalized textures. The CPU copy is still released after upload.
pub fn release_vat_image(image: &mut Image) -> bool {
    if image.texture_descriptor.format != TextureFormat::Rgba32Float
        || image.asset_usage == RenderAssetUsages::RENDER_WORLD
    {
        return false;
    }
    image.asset_usage = RenderAssetUsages::RENDER_WORLD;
    true
}

/// Compacts the VAT texture of every animator once both the texture and the animator exist.
///
/// `Rgba16Unorm` needs `TEXTURE_FORMAT_16BIT_NORM`, a native-only wgpu feature; without it
/// (WebGPU, some mobile drivers) the texture keeps its format and only the CPU copy is released.
/// Textures already in another format (KTX2, procedurally built) are left alone. VAT materials
/// bound to a converted texture are marked modified so their bind groups pick up the new one.
pub fn compact_vat_textures(
    mut events: MessageReader<AssetEvent<Image>>,
    new_animators: Query<&VatAnimator, Added<VatAnimator>>,
    animators: Query<&VatAnimator>,
    remaps: Res<Assets<RemapInfo>>,
    device: Option<Res<RenderDevice>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<VatStandardMaterial>>,
) {
    let unorm16 = device.as_ref().is_none_or(|device| {
        device
            .features()
            .contains(WgpuFeatures::TEXTURE_FORMAT_16BIT_NORM)
    });
    // `Modified` covers hot reloads; our own conversion also emits it, but the format check
    // below skips an image that is already compact.
    let added: HashSet<AssetId<Image>> = events
        .read()
        .filter_map(|event| match event {
            AssetEvent::Added { id } | AssetEvent::Modified { id } => Some(*id),
            _ => None,
        })
        .collect();
    let mut candidates: HashSet<AssetId<Image>> =
        new_animators.iter().map(|a| a.vat_texture.id()).collect();
    if !added.is_empty() {
        candidates.extend(
            animators
                .iter()
                .map(|a| a.vat_texture.id())
                .filter(|id| added.contains(id)),
        );
    }
    for id in candidates {
        let Some(image) = images.get(id) else {
            continue;
        };
        if image.texture_descriptor.format != TextureFormat::Rgba32Float {
            continue;
        }
        let frames = animators
            .iter()
            .filter(|a| a.vat_texture.id() == id)
            .find_map(|a| remaps.get(&a.remap_info))
            .map(|remap| remap.os_remap.frames);
        // OpenVAT pads the texture to a power-of-two height; normals start at `height / 2`,
        // so only a texture too short for its frames is a problem.
        if let Some(frames) = frames
            && image.height() < 2 * frames
        {
            warn!(
                "VAT texture {id:?} height {} has fewer than 2 × {frames} rows",
                image.height()
            );
        }
        let Some(mut image) = images.get_mut(id) else {
            continue;
        };
        let changed = if unorm16 {
            compact_vat_image(&mut image)
        } else {
            release_vat_image(&mut image)
        };
        if !changed {
            continue;
        }
        let users: Vec<_> = materials
            .iter()
            .filter(|(_, mat)| mat.extension.vat_texture.id() == id)
            .map(|(mat_id, _)| mat_id)
            .collect();
        for mat_id in users {
            // `AssetMut` only queues `Modified` once the asset is borrowed mutably.
            if let Some(material) = materials.get_mut(mat_id) {
                material.into_inner();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::render::render_resource::{Extent3d, TextureDimension};

    /// A 1×N `Rgba32Float` image holding `values`, with a CPU copy.
    fn float_image(values: &[f32]) -> Image {
        Image::new(
            Extent3d {
                width: 1,
                height: (values.len() / 4) as u32,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            bytemuck::cast_slice(values).to_vec(),
            TextureFormat::Rgba32Float,
            RenderAssetUsages::default(),
        )
    }

    fn texels(img: &Image) -> &[u16] {
        bytemuck::cast_slice(img.data.as_ref().unwrap())
    }

    #[test]
    fn test_compact_converts_to_rgba16unorm() {
        let mut img = float_image(&[0.0, 0.5, 1.0, 0.25, 0.75, 0.0, 1.0, 1.0]);
        assert!(compact_vat_image(&mut img));
        assert_eq!(img.texture_descriptor.format, TextureFormat::Rgba16Unorm);
        assert_eq!(img.asset_usage, RenderAssetUsages::RENDER_WORLD);
        assert_eq!(img.height(), 2);
        assert_eq!(
            texels(&img),
            &[0, 32768, 65535, 16384, 49151, 0, 65535, 65535]
        );
    }

    #[test]
    fn test_compact_clamps_out_of_range() {
        let mut img = float_image(&[-1.0, 2.0, 1.0001, 0.0]);
        assert!(compact_vat_image(&mut img));
        assert_eq!(texels(&img), &[0, 65535, 65535, 0]);
    }

    #[test]
    fn test_reloaded_texture_is_compacted_again_and_materials_rebound() {
        use crate::material::VatStandardMaterial;
        use crate::system::tests::{remap_handle, std_material, test_app};
        let mut app = test_app();
        let remap = remap_handle(&mut app);
        let texture = app
            .world_mut()
            .resource_mut::<Assets<Image>>()
            .add(float_image(&[0.5; 4]));
        let material = std_material(&mut app);
        app.world_mut().spawn((
            VatAnimator::new(remap, texture.clone(), Handle::default()),
            MeshMaterial3d(material),
        ));
        app.update();
        app.update();
        let format = |app: &App| {
            app.world()
                .resource::<Assets<Image>>()
                .get(&texture)
                .unwrap()
                .texture_descriptor
                .format
        };
        assert_eq!(format(&app), TextureFormat::Rgba16Unorm);
        assert_eq!(
            app.world().resource::<Assets<VatStandardMaterial>>().len(),
            1
        );

        // A hot-reloaded EXR arrives as a modified `Rgba32Float` image.
        app.world_mut()
            .resource_mut::<Assets<Image>>()
            .insert(&texture, float_image(&[0.5; 4]))
            .unwrap();
        app.update();
        app.update();
        assert_eq!(format(&app), TextureFormat::Rgba16Unorm);
        // The material must be re-prepared, or its bind group keeps the old GPU texture.
        let material_modified = app
            .world()
            .resource::<Messages<AssetEvent<VatStandardMaterial>>>()
            .iter_current_update_messages()
            .any(|event| matches!(event, AssetEvent::Modified { .. }));
        assert!(material_modified, "VAT material must be marked modified");
    }

    #[test]
    fn test_release_keeps_format_but_drops_cpu_copy() {
        let mut img = float_image(&[0.5; 4]);
        assert!(release_vat_image(&mut img));
        assert_eq!(img.texture_descriptor.format, TextureFormat::Rgba32Float);
        assert_eq!(img.asset_usage, RenderAssetUsages::RENDER_WORLD);
        assert!(!release_vat_image(&mut img), "already render-only");
    }

    #[test]
    fn test_compact_ignores_other_formats() {
        let mut img = Image::default();
        assert!(!compact_vat_image(&mut img));
        assert_eq!(img.texture_descriptor.format, TextureFormat::Rgba8UnormSrgb);
    }
}
