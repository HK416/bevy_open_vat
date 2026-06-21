use bevy::{
    asset::{AssetLoader, LoadContext, io::Reader},
    platform::collections::HashMap,
    prelude::*,
    reflect::TypePath,
    tasks::ConditionalSendFuture,
};
use serde::Deserialize;
use thiserror::Error;

/// Structure representing the position remapping data from the JSON file.
/// Used to decompress the normalized VAT texture values back into world space positions.
#[derive(Debug, Clone, Copy, Deserialize, Reflect)]
pub struct OsRemap {
    /// Minimum position offset bounds for VAT geometry.
    #[serde(rename = "Min")]
    pub min: [f32; 3],
    /// Maximum position offset bounds for VAT geometry.
    #[serde(rename = "Max")]
    pub max: [f32; 3],
    /// Total number of frames in the VAT.
    #[serde(rename = "Frames")]
    pub frames: u32,
}

/// Structure representing an animation clip defined in the JSON file.
#[derive(Debug, Clone, Copy, Deserialize, Reflect, Asset)]
pub struct VatAnimationClip {
    /// The starting frame index of the clip.
    #[serde(rename = "startFrame")]
    pub start_frame: u32,
    /// The ending frame index of the clip.
    #[serde(rename = "endFrame")]
    pub end_frame: u32,
    /// The framerate (frames per second) of the clip.
    #[serde(rename = "framerate")]
    pub frame_rate: f32,
    /// Whether the clip should loop automatically.
    pub looping: bool,
}

impl VatAnimationClip {
    /// Gets the start time of the clip in seconds.
    pub fn start_time(&self) -> f32 {
        self.start_frame as f32 / self.frame_rate
    }

    /// Gets the duration of the clip in seconds.
    pub fn duration(&self) -> Option<f32> {
        if self.frame_rate <= 0.0 {
            return None;
        }

        Some((self.end_frame - self.start_frame) as f32 / self.frame_rate)
    }
}

/// Main asset structure holding remapping info and animation clips.
/// This corresponds to the sidecar JSON file generated alongside the VAT texture.
#[derive(Debug, Clone, Asset, Deserialize, TypePath)]
pub struct RemapInfo {
    /// The coordinate remapping data.
    #[serde(rename = "os-remap")]
    pub os_remap: OsRemap,
    /// A collection of animation clips mapped by name.
    pub animations: HashMap<String, VatAnimationClip>,
}

/// Asset loader for `RemapInfo` files (JSON).
#[derive(Default, TypePath)]
pub(crate) struct RemapInfoAssetLoader;

/// Error types for RemapInfo loader.
#[derive(Debug, Error)]
pub enum RemapLoaderError {
    /// IO error during loading.
    #[error("Failed to load asset for the following reason:{0}")]
    Io(#[from] std::io::Error),
    /// JSON parsing error during loading.
    #[error("Failed to decode asset for the following reason:{0}")]
    Json(#[from] serde_json::Error),
}

impl AssetLoader for RemapInfoAssetLoader {
    type Asset = RemapInfo;
    type Settings = ();
    type Error = RemapLoaderError;

    fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &Self::Settings,
        load_context: &mut LoadContext,
    ) -> impl ConditionalSendFuture<Output = std::result::Result<Self::Asset, Self::Error>> {
        Box::pin(async move {
            // Read the raw bytes from the asset file.
            let mut bytes = Vec::new();
            reader.read_to_end(&mut bytes).await?;

            // Deserialize the JSON bytes into our serializable format.
            let remap_info = serde_json::from_slice::<RemapInfo>(&bytes)?;

            // Register each animation clip as a sub-asset
            for (name, clip) in &remap_info.animations {
                load_context.add_labeled_asset(name.clone(), clip.clone());
            }

            Ok(remap_info)
        })
    }

    fn extensions(&self) -> &[&str] {
        &["json"]
    }
}
