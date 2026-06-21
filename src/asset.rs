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
        if self.frame_rate <= 0.0 {
            return 0.0;
        }
        self.start_frame as f32 / self.frame_rate
    }

    /// Returns the number of frames in the clip.
    /// Uses saturating subtraction to prevent underflow when end_frame < start_frame.
    pub fn frame_count(&self) -> u32 {
        self.end_frame.saturating_sub(self.start_frame)
    }

    /// Gets the duration of the clip in seconds.
    /// Returns `None` if the clip has zero frames or an invalid frame rate.
    pub fn duration(&self) -> Option<f32> {
        if self.frame_rate <= 0.0 {
            return None;
        }

        let frames = self.frame_count();
        if frames == 0 {
            return None;
        }

        Some(frames as f32 / self.frame_rate)
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

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================
    // VatAnimationClip 테스트
    // ========================================

    fn make_clip(start: u32, end: u32, rate: f32, looping: bool) -> VatAnimationClip {
        VatAnimationClip {
            start_frame: start,
            end_frame: end,
            frame_rate: rate,
            looping,
        }
    }

    #[test]
    fn test_duration_normal() {
        let clip = make_clip(0, 60, 30.0, true);
        let d = clip.duration().unwrap();
        assert!((d - 2.0).abs() < f32::EPSILON, "60 frames at 30fps = 2.0s, got {d}");
    }

    #[test]
    fn test_duration_with_offset_start() {
        let clip = make_clip(10, 40, 30.0, false);
        let d = clip.duration().unwrap();
        assert!((d - 1.0).abs() < f32::EPSILON, "30 frames at 30fps = 1.0s, got {d}");
    }

    #[test]
    fn test_duration_zero_frames_returns_none() {
        let clip = make_clip(10, 10, 30.0, true);
        assert!(clip.duration().is_none(), "Zero-frame clip should return None");
    }

    #[test]
    fn test_duration_zero_framerate_returns_none() {
        let clip = make_clip(0, 60, 0.0, true);
        assert!(clip.duration().is_none(), "Zero framerate should return None");
    }

    #[test]
    fn test_duration_negative_framerate_returns_none() {
        let clip = make_clip(0, 60, -1.0, true);
        assert!(clip.duration().is_none(), "Negative framerate should return None");
    }

    #[test]
    fn test_duration_end_before_start_returns_none() {
        // Medium #5: end_frame < start_frame must NOT panic
        let clip = make_clip(50, 10, 30.0, true);
        assert!(clip.duration().is_none(), "Inverted range should return None (0 frames)");
    }

    #[test]
    fn test_frame_count_normal() {
        let clip = make_clip(0, 60, 30.0, true);
        assert_eq!(clip.frame_count(), 60);
    }

    #[test]
    fn test_frame_count_with_offset() {
        let clip = make_clip(10, 40, 30.0, true);
        assert_eq!(clip.frame_count(), 30);
    }

    #[test]
    fn test_frame_count_zero() {
        let clip = make_clip(10, 10, 30.0, true);
        assert_eq!(clip.frame_count(), 0);
    }

    #[test]
    fn test_frame_count_inverted_no_panic() {
        // Medium #5: must NOT panic, should return 0 via saturating_sub
        let clip = make_clip(100, 50, 30.0, true);
        assert_eq!(clip.frame_count(), 0);
    }

    #[test]
    fn test_start_time_normal() {
        let clip = make_clip(30, 60, 30.0, true);
        let st = clip.start_time();
        assert!((st - 1.0).abs() < f32::EPSILON, "Frame 30 at 30fps = 1.0s, got {st}");
    }

    #[test]
    fn test_start_time_zero_framerate() {
        let clip = make_clip(30, 60, 0.0, true);
        assert_eq!(clip.start_time(), 0.0, "Zero framerate should return 0.0");
    }

    // ========================================
    // VatInstanceData 테스트
    // ========================================

    use crate::data::VatInstanceData;

    #[test]
    fn test_instance_data_default() {
        let d = VatInstanceData::default();
        assert_eq!(d.start_frame, 0);
        assert_eq!(d.frame_count, 1);
        assert_eq!(d.rate, 0.0);
        assert_eq!(d.offset, 0.0);
    }

    #[test]
    fn test_instance_data_size() {
        // GPU expects exactly 16 bytes (4 x 4-byte fields)
        assert_eq!(std::mem::size_of::<VatInstanceData>(), 16);
    }

    #[test]
    fn test_instance_data_alignment() {
        // #[repr(C)] guarantees no padding between same-sized fields
        assert_eq!(std::mem::align_of::<VatInstanceData>(), 4);
    }

    #[test]
    fn test_instance_data_custom_values() {
        let d = VatInstanceData {
            start_frame: 10,
            frame_count: 50,
            rate: 1.5,
            offset: -0.3,
        };
        assert_eq!(d.start_frame, 10);
        assert_eq!(d.frame_count, 50);
        assert!((d.rate - 1.5).abs() < f32::EPSILON);
        assert!((d.offset - (-0.3)).abs() < f32::EPSILON);
    }

    #[test]
    fn test_instance_data_zero_frame_count() {
        let d = VatInstanceData {
            start_frame: 0,
            frame_count: 0,
            rate: 1.0,
            offset: 0.0,
        };
        assert_eq!(d.frame_count, 0);
    }

    #[test]
    fn test_instance_data_max_values() {
        let d = VatInstanceData {
            start_frame: u32::MAX,
            frame_count: u32::MAX,
            rate: f32::MAX,
            offset: f32::MIN,
        };
        assert_eq!(d.start_frame, u32::MAX);
        assert_eq!(d.frame_count, u32::MAX);
    }

    // ========================================
    // OsRemap 테스트
    // ========================================

    #[test]
    fn test_os_remap_default_values() {
        let remap = OsRemap {
            min: [0.0, 0.0, 0.0],
            max: [1.0, 1.0, 1.0],
            frames: 60,
        };
        assert_eq!(remap.frames, 60);
        assert_eq!(remap.min, [0.0, 0.0, 0.0]);
        assert_eq!(remap.max, [1.0, 1.0, 1.0]);
    }

    #[test]
    fn test_os_remap_negative_bounds() {
        let remap = OsRemap {
            min: [-10.0, -5.0, -3.0],
            max: [10.0, 5.0, 3.0],
            frames: 120,
        };
        assert!(remap.min[0] < remap.max[0]);
        assert!(remap.min[1] < remap.max[1]);
        assert!(remap.min[2] < remap.max[2]);
    }

    // ========================================
    // JSON 디시리얼라이즈 테스트
    // ========================================

    #[test]
    fn test_remap_info_json_deserialization() {
        let json = r#"{
            "os-remap": {
                "Min": [-1.0, -2.0, -3.0],
                "Max": [1.0, 2.0, 3.0],
                "Frames": 100
            },
            "animations": {
                "Run": {
                    "startFrame": 0,
                    "endFrame": 30,
                    "framerate": 30.0,
                    "looping": true
                },
                "Idle": {
                    "startFrame": 30,
                    "endFrame": 90,
                    "framerate": 24.0,
                    "looping": false
                }
            }
        }"#;

        let info: RemapInfo = serde_json::from_str(json).expect("JSON deserialization failed");
        assert_eq!(info.os_remap.frames, 100);
        assert_eq!(info.os_remap.min, [-1.0, -2.0, -3.0]);
        assert_eq!(info.os_remap.max, [1.0, 2.0, 3.0]);
        assert_eq!(info.animations.len(), 2);

        let run = info.animations.get("Run").expect("Run clip missing");
        assert_eq!(run.start_frame, 0);
        assert_eq!(run.end_frame, 30);
        assert!((run.frame_rate - 30.0).abs() < f32::EPSILON);
        assert!(run.looping);

        let idle = info.animations.get("Idle").expect("Idle clip missing");
        assert_eq!(idle.start_frame, 30);
        assert_eq!(idle.end_frame, 90);
        assert!(!idle.looping);
    }

    #[test]
    fn test_clip_json_deserialization() {
        let json = r#"{
            "startFrame": 5,
            "endFrame": 25,
            "framerate": 12.0,
            "looping": true
        }"#;

        let clip: VatAnimationClip = serde_json::from_str(json).expect("Clip deserialization failed");
        assert_eq!(clip.start_frame, 5);
        assert_eq!(clip.end_frame, 25);
        assert_eq!(clip.frame_count(), 20);
        assert!((clip.frame_rate - 12.0).abs() < f32::EPSILON);
        assert!(clip.looping);

        let d = clip.duration().unwrap();
        let expected = 20.0 / 12.0;
        assert!((d - expected).abs() < 1e-6, "Expected {expected}, got {d}");
    }

    #[test]
    fn test_empty_animations_json() {
        let json = r#"{
            "os-remap": {
                "Min": [0.0, 0.0, 0.0],
                "Max": [1.0, 1.0, 1.0],
                "Frames": 10
            },
            "animations": {}
        }"#;

        let info: RemapInfo = serde_json::from_str(json).expect("Empty animations should be valid");
        assert!(info.animations.is_empty());
    }
}
