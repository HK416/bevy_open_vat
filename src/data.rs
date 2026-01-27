use bevy::{prelude::*, render::render_resource::ShaderType};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Reflect)]
pub enum VatAnimLoopMode {
    #[default]
    Once,
    Loop,
}

#[derive(Debug, Clone, Copy, Reflect)]
pub struct VatAnimationClip {
    pub frame_count: u32,
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

#[derive(Debug, Clone, Copy, Component, Reflect)]
pub struct VatAnimationController {
    pub current_clip: VatAnimationClip,
    pub mode: VatAnimLoopMode,
    pub timer: f32,
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
