use bevy::{
    pbr::{ExtendedMaterial, MaterialExtension},
    prelude::*,
    render::{render_resource::AsBindGroup, storage::ShaderStorageBuffer},
    shader::ShaderRef,
};

use crate::plugin::{OPENVAT_PREPASS_SHADER_HANDLE, OPENVAT_SHADER_HANDLE};

pub type VatStandardMaterial = ExtendedMaterial<StandardMaterial, OpenVatExtension>;

/// A material extension that adds Vertex Animation Texture (VAT) support to StandardMaterial.
#[derive(Debug, Default, Clone, Asset, AsBindGroup, Reflect)]
pub struct OpenVatExtension {
    /// The VAT texture containing position and normal offsets.
    #[texture(100, visibility(vertex))]
    #[sampler(101, visibility(vertex))]
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
    /// The Y resolution of the texture (used for UV calculation).
    #[uniform(102, visibility(vertex))]
    pub y_resolution: f32,

    /// Buffer storing per-instance animation data (e.g., current time).
    #[storage(103, visibility(vertex), read_only)]
    pub instance: Handle<ShaderStorageBuffer>,
}

impl MaterialExtension for OpenVatExtension {
    fn vertex_shader() -> ShaderRef {
        OPENVAT_SHADER_HANDLE.into()
    }

    fn prepass_vertex_shader() -> ShaderRef {
        OPENVAT_PREPASS_SHADER_HANDLE.into()
    }
}
