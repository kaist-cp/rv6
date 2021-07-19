//! ARM instructions.

// Dead code is allowed in this file because not all components are used in the kernel.
#![allow(dead_code)]

/// Enable device interrupts.
#[inline]
pub unsafe fn intr_on() {
    unimplemented!()
}

/// Disable device interrupts.
#[inline]
pub fn intr_off() {
    unimplemented!()
}

/// Are device interrupts enabled?
#[inline]
pub fn intr_get() -> bool {
    unimplemented!()
}
