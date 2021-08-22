mod gicv2;
mod gicv3;

#[cfg(feature = "gicv2")]
pub use gicv2::*;
#[cfg(feature = "gicv3")]
pub use gicv3::*;

use crate::arch::interface::InterruptManager;
use crate::arch::ArmV8;

impl InterruptManager for ArmV8 {
    unsafe fn intr_init() {
        unsafe {
            intr_init();
        }
    }

    unsafe fn intr_init_core() {
        unsafe {
            intr_init_core();
        }
    }
}
