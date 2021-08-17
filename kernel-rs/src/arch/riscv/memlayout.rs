//! Physical memory layout
//!
//! qemu -machine virt is set up like this,
//! based on qemu's hw/riscv/virt.c:
//!
//! 00001000 -- boot ROM, provided by qemu
//! 02000000 -- CLINT
//! 0C000000 -- PLIC
//! 10000000 -- uart0
//! 10001000 -- virtio disk
//! 80000000 -- boot ROM jumps here in machine mode
//!             -kernel loads the kernel here
//! unused RAM after 80000000.
//! the kernel uses physical memory thus:
//! 80000000 -- entry.S, then kernel text and data
//! end -- start of kernel page allocation area
//! PHYSTOP -- end RAM used by the kernel

// Dead code is allowed in this file because not all components are used in the kernel.
#![allow(dead_code)]

use crate::memlayout::{DeviceMappingInfo, IrqNumbers};

pub type MemLayout = RiscVVirtMemLayout;

pub struct RiscVVirtMemLayout;

impl DeviceMappingInfo for RiscVVirtMemLayout {
    /// the kernel expects there to be RAM
    /// for use by the kernel and user pages
    /// from physical address 0x80000000 to PHYSTOP.
    const KERNBASE: usize = 0x80000000;
    /// qemu puts UART registers here in physical memory.
    const UART0: usize = 0x10000000;
    /// virtio mmio interface
    const VIRTIO0: usize = 0x10001000;
}

impl IrqNumbers for RiscVVirtMemLayout {
    const UART0_IRQ: usize = 10;
    const VIRTIO0_IRQ: usize = 1;
}

/// SiFive Test Finisher. (virt device only)
pub const FINISHER: usize = 0x100000;

/// core local interruptor (CLINT), which contains the timer.
pub const CLINT: usize = 0x2000000;
pub const fn clint_mtimecmp(hartid: usize) -> usize {
    CLINT
        .wrapping_add(0x4000)
        .wrapping_add(hartid.wrapping_mul(8))
}

/// cycles since boot.
pub const CLINT_MTIME: usize = CLINT.wrapping_add(0xbff8);

/// qemu puts platform-level interrupt controller (PLIC) here.
pub const PLIC: usize = 0xc000000;

pub const PLIC_PENDING: usize = PLIC.wrapping_add(0x1000);

pub const fn plic_senable(hart: usize) -> usize {
    PLIC.wrapping_add(0x2080)
        .wrapping_add((hart).wrapping_mul(0x100))
}
pub const fn plic_spriority(hart: usize) -> usize {
    PLIC.wrapping_add(0x201000)
        .wrapping_add((hart).wrapping_mul(0x2000))
}
pub const fn plic_sclaim(hart: usize) -> usize {
    PLIC.wrapping_add(0x201004)
        .wrapping_add((hart).wrapping_mul(0x2000))
}
