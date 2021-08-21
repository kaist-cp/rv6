//! Physical memory layout
//!
//! aarch64 -machine virt is set up like this,
//! based on qemu's hw/arm/virt.c:
//!
//! 00000000 -- boot ROM, provided by qemu, space up to 0x8000000 is reserved.
//! 08000000 -- GIC
//! 09000000 -- uart0
//! 0a000000 -- virtio disk
//! 40010000 -- boot ROM jumps here in machine mode
//!             -kernel loads the kernel here
//! unused RAM after 40000000.
//! the kernel uses physical memory thus:
//! 40010000 -- entry.S, then kernel text and data
//! end -- start of kernel page allocation area
//! PHYSTOP -- end RAM used by the kernel

// Dead code is allowed in this file because not all components are used in the kernel.
#![allow(dead_code)]

use crate::arch::interface::MemLayout;
use crate::arch::ArmV8;

impl MemLayout for ArmV8 {
    /// the kernel expects there to be RAM
    /// for use by the kernel and user pages
    /// from physical address 0x80000000 to PHYSTOP.
    const KERNBASE: usize = 0x40000000;
    /// qemu puts UART registers here in physical memory.
    const UART0: usize = 0x09000000;
    const UART0_IRQ: usize = 33;
    /// virtio mmio interface
    const VIRTIO0: usize = 0x0a000000;
    const VIRTIO0_IRQ: usize = 48;
}

// TODO: Find counterpart of this in ARM, seems that it doesn't exist.
/// SiFive Test Finisher. (virt device only)
// pub const FINISHER: usize = 0x100000;

/// qemu puts Arm generic Interrupt controller (GIC) here.
pub const GIC: usize = 0x08000000;

pub const TIMER0_IRQ: usize = 27;
