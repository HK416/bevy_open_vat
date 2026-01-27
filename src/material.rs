use bevy::{
    pbr::MaterialExtension,
    prelude::*,
    render::{render_resource::AsBindGroup, storage::ShaderStorageBuffer},
    shader::ShaderRef,
};

const SHADER_ASSET_PATH: &str = "shaders/openvat_pbr.wgsl";

#[derive(Debug, Clone, Asset, AsBindGroup, Reflect)]
pub struct OpenVatExtension {
    #[texture(100, visibility(vertex))]
    #[sampler(101, visibility(vertex))]
    pub vat_texture: Handle<Image>,

    #[uniform(102, visibility(vertex))]
    pub min_pos: Vec3,
    #[uniform(102, visibility(vertex))]
    pub frame_count: u32,
    #[uniform(102, visibility(vertex))]
    pub max_pos: Vec3,
    #[uniform(102, visibility(vertex))]
    pub y_resolution: f32,

    #[storage(103, visibility(vertex), read_only)]
    pub instance: Handle<ShaderStorageBuffer>,
}

impl MaterialExtension for OpenVatExtension {
    fn vertex_shader() -> ShaderRef {
        SHADER_ASSET_PATH.into()
    }
}
