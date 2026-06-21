use bevy::{prelude::*, render::render_resource::ShaderType};

use crate::asset::{RemapInfo, VatAnimationClip};

/// Internal marker component indicating the entity's material has been swapped
/// and the shader buffer is ready.
#[derive(Component)]
pub struct VatMaterialReady;

/// Marker component to indicate that VatAnimator has been propagated to children.
/// Unlike VatMaterialReady, this is for entities without StandardMaterial (container nodes).
#[derive(Component)]
pub struct VatPropagated;

/// Component to control the playback of a VAT animation on an entity.
/// Add this to an entity to automatically convert its material to a VAT material.
#[derive(Debug, Clone, Component, Reflect)]
#[require(Transform, Visibility)]
pub struct VatAnimator {
    /// Handle to the remapping info asset.
    pub remap_info: Handle<RemapInfo>,
    /// Handle to the VAT image texture.
    pub vat_texture: Handle<Image>,
    /// Handle to the current animation clip to play.
    pub current_clip: Handle<VatAnimationClip>,
    /// Reference time for animation start (Global time).
    pub start_time: f32,
    /// Accumulated time offset (used for handling pause/resume/looping).
    pub offset: f32,
    /// Playback speed multiplier (1.0 is normal speed).
    pub speed: f32,
    /// Whether the animation is currently playing.
    pub is_playing: bool,
}

impl Default for VatAnimator {
    fn default() -> Self {
        Self {
            remap_info: Default::default(),
            vat_texture: Default::default(),
            current_clip: Default::default(),
            start_time: 0.0,
            offset: 0.0,
            speed: 1.0,
            is_playing: true,
        }
    }
}

/// Data sent to the GPU for each instance, containing the current animation state.
#[repr(C)]
#[derive(Debug, Clone, Copy, Reflect, ShaderType)]
pub struct VatInstanceData {
    /// The starting frame of the animation clip.
    pub start_frame: u32,
    /// The total number of frames in the clip.
    pub frame_count: u32,
    /// The playback rate (speed / duration).
    pub rate: f32,
    /// The time offset for the animation.
    pub offset: f32,
}

impl Default for VatInstanceData {
    fn default() -> Self {
        Self {
            start_frame: 0,
            frame_count: 1,
            rate: 0.0,
            offset: 0.0,
        }
    }
}
