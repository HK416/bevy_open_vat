pub mod data;
pub mod material;
pub mod plugin;
pub mod system;

pub mod prelude {
    pub use crate::data::{VatAnimLoopMode, VatAnimationClip, VatAnimationController};
    pub use crate::material::OpenVatExtension;
    pub use crate::plugin::OpenVatPlugin;
}
