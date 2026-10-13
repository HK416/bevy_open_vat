use bevy::{
    pbr::{ExtendedMaterial, MaterialExtension},
    prelude::*,
    render::{render_resource::AsBindGroup, storage::ShaderBuffer},
    shader::ShaderRef,
};

use crate::plugin::{OPENVAT_PREPASS_SHADER_HANDLE, OPENVAT_SHADER_HANDLE};

/// Type alias for the standard material extended with OpenVAT support.
pub type VatStandardMaterial = ExtendedMaterial<StandardMaterial, OpenVatExtension>;

/// A material extension that adds Vertex Animation Texture (VAT) support to StandardMaterial.
#[derive(Debug, Default, Clone, Asset, AsBindGroup, Reflect)]
pub struct OpenVatExtension {
    /// The VAT texture containing position and normal offsets.
    /// Read with `textureLoad`, so it needs no sampler and may use non-filterable formats
    /// such as the `Rgba32Float` that EXR files load as.
    #[texture(100, visibility(vertex), filterable = false)]
    pub vat_texture: Handle<Image>,

    /// Minimum position bound for decoding the texture data.
    #[uniform(102, visibility(vertex))]
    pub min_pos: Vec3,
    /// Total number of frames in the texture.
    #[uniform(102, visibility(vertex))]
    pub frame_count: u32,
    /// Maximum position bound for decoding the texture data.
    #[uniform(102, visibility(vertex))]
    pub max_pos: Vec3,

    /// Buffer storing per-instance animation data (e.g., current time).
    #[storage(103, visibility(vertex), read_only)]
    pub instance: Handle<ShaderBuffer>,
}

impl MaterialExtension for OpenVatExtension {
    fn vertex_shader() -> ShaderRef {
        OPENVAT_SHADER_HANDLE.into()
    }

    fn prepass_vertex_shader() -> ShaderRef {
        OPENVAT_PREPASS_SHADER_HANDLE.into()
    }
}
