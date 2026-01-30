use bevy::{prelude::*, render::render_resource::ShaderType};

use crate::asset::RemapInfo;

/// Component to control the playback of a VAT animation on an entity.
#[derive(Debug, Clone, Component, Reflect)]
pub struct VatAnimationController {
    pub remap_info: Handle<RemapInfo>,
    pub current_clip: String,
    /// Current playback time in seconds.
    pub timer: f32,
    /// Playback speed multiplier (1.0 is normal speed).
    pub speed: f32,
    pub is_playing: bool,
}

impl Default for VatAnimationController {
    fn default() -> Self {
        Self {
            remap_info: Handle::default(),
            current_clip: String::default(),
            timer: 0.0,
            speed: 1.0,
            is_playing: true,
        }
    }
}

/// Data sent to the GPU for each instance, containing the current animation state.
#[repr(C)]
#[derive(Debug, Clone, Copy, Reflect, ShaderType)]
pub struct VatInstanceData {
    pub timer: f32,
}

impl Default for VatInstanceData {
    fn default() -> Self {
        Self { timer: 0.0 }
    }
}
