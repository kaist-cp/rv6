//! Utilities.

// Dead code is allowed in this file because not all components are used in the kernel.
#![allow(dead_code)]

pub mod branded;
pub mod etrace;
pub mod intrusive_list;
pub mod pinned_array;
pub mod static_arc;
pub mod strong_pin;

pub fn spin_loop() -> ! {
    loop {
        ::core::hint::spin_loop();
    }
}
