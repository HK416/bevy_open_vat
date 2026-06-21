#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

/// Asset types for VAT, including animations and remapping info.
pub mod asset;
/// Core data components and structs for VAT playback.
pub mod data;
/// Material extension for rendering VAT meshes.
pub mod material;
/// Bevy plugin integration for OpenVAT.
pub mod plugin;
/// Systems for auto-setup and updating VAT animations.
pub mod system;

/// Prelude module containing commonly used types and components.
pub mod prelude {
    pub use crate::asset::{OsRemap, RemapInfo, VatAnimationClip};
    pub use crate::data::VatAnimator;
    pub use crate::material::OpenVatExtension;
    pub use crate::plugin::OpenVatPlugin;
}
