use bevy::{prelude::*, render::render_resource::ShaderType};

/// Defines how the animation should behave when it reaches the end.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Reflect)]
pub enum VatAnimLoopMode {
    #[default]
    Once,
    Loop,
}

/// Stores metadata about a specific VAT animation clip.
#[derive(Debug, Clone, Copy, Reflect)]
pub struct VatAnimationClip {
    /// Total number of frames in the animation texture.
    pub frame_count: u32,
    /// The frame rate at which the animation was sampled/baked.
    pub sampling_fps: f32,
}

impl Default for VatAnimationClip {
    fn default() -> Self {
        Self {
            frame_count: 0,
            sampling_fps: 30.0,
        }
    }
}

/// Component to control the playback of a VAT animation on an entity.
#[derive(Debug, Clone, Copy, Component, Reflect)]
pub struct VatAnimationController {
    pub current_clip: VatAnimationClip,
    pub mode: VatAnimLoopMode,
    /// Current playback time in seconds.
    pub timer: f32,
    /// Playback speed multiplier (1.0 is normal speed).
    pub speed: f32,
    pub is_playing: bool,
}

impl Default for VatAnimationController {
    fn default() -> Self {
        Self {
            current_clip: VatAnimationClip::default(),
            mode: VatAnimLoopMode::default(),
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
