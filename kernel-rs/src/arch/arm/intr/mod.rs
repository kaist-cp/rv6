mod gicv2;
mod gicv3;

#[cfg(feature = "gicv2")]
pub use gicv2::*;
#[cfg(feature = "gicv3")]
pub use gicv3::*;
