//! Utilities.

// Dead code is allowed in this file because not all components are used in the kernel.
#![allow(dead_code)]

pub mod branded;
pub mod etrace;
pub mod intrusive_list;
pub mod list;
pub mod pinned_array;
pub mod rc_cell;
pub mod shared_mut;

pub fn spin_loop() -> ! {
    loop {
        ::core::hint::spin_loop();
    }
}
