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

use crate::addr::{MAXVA, PGSIZE};
use crate::memlayout::MemLayout;

pub struct MemLayoutImpl;

impl MemLayout for MemLayoutImpl {
    /// the kernel expects there to be RAM
    /// for use by the kernel and user pages
    /// from physical address 0x80000000 to PHYSTOP.
    const KERNBASE: usize = 0x40000000;
    /// map the trampoline page to the highest address,
    /// in both user and kernel space.
    const TRAMPOLINE: usize = MAXVA.wrapping_sub(PGSIZE);
    const TRAPFRAME: usize = Self::TRAMPOLINE.wrapping_sub(PGSIZE);
    /// qemu puts UART registers here in physical memory.
    const UART0: usize = 0x09000000;
    /// virtio mmio interface
    const VIRTIO0: usize = 0x0a000000;

    /// map kernel stacks beneath the MAXVA,
    /// each surrounded by invalid guard pages.
    /// TODO: change this?
    fn kstack(p: usize) -> usize {
        Self::TRAMPOLINE - ((p + 1) * 2 * PGSIZE)
    }
}

// TODO: Find counterpart of this in ARM, seems that it doesn't exist.
/// SiFive Test Finisher. (virt device only)
// pub const FINISHER: usize = 0x100000;

/// qemu puts Arm generic Interrupt controller (GIC) here.
pub const GIC: usize = 0x08000000;

pub const TIMER0_IRQ: usize = 27;
pub const UART0_IRQ: usize = 33;
pub const VIRTIO0_IRQ: usize = 48;

// TODO: change this to its counterpart in ARM
// core local interruptor (CLINT), which contains the timer.
// pub const CLINT: usize = 0x2000000;
// pub const fn clint_mtimecmp(coreid: usize) -> usize {
//     CLINT
//         .wrapping_add(0x4000)
//         .wrapping_add(coreid.wrapping_mul(8))
// }
// cycles since boot.
// pub const CLINT_MTIME: usize = CLINT.wrapping_add(0xbff8);

// TODO: implement trampoline in ARM
