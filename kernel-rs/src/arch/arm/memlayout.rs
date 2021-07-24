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

use crate::arch::addr::{MAXVA, PGSIZE};

// TODO: Find counterpart of this in ARM, seems that it doesn't exist.
/// SiFive Test Finisher. (virt device only)
// pub const FINISHER: usize = 0x100000;

/// qemu puts UART registers here in physical memory.
pub const UART0: usize = 0x09000000;
pub const UART0_IRQ: usize = 10;

/// virtio mmio interface
pub const VIRTIO0: usize = 0x0a000000;
pub const VIRTIO0_IRQ: usize = 1;

// TODO: change this to its counterpart in ARM
/// core local interruptor (CLINT), which contains the timer.
pub const CLINT: usize = 0x2000000;
pub const fn clint_mtimecmp(hartid: usize) -> usize {
    CLINT
        .wrapping_add(0x4000)
        .wrapping_add(hartid.wrapping_mul(8))
}

/// cycles since boot.
pub const CLINT_MTIME: usize = CLINT.wrapping_add(0xbff8);

/// qemu puts Arm generic Interrupt controller (GIC) here.
pub const GIC: usize = 0x08000000;

/// the kernel expects there to be RAM
/// for use by the kernel and user pages
/// from physical address 0x40000000 to PHYSTOP.
pub const KERNBASE: usize = 0x40000000;
pub const PHYSTOP: usize = KERNBASE.wrapping_add(128 * 1024 * 1024);

// TODO: implement trampoline in ARM
/// map the trampoline page to the highest address,
/// in both user and kernel space.
// pub const TRAMPOLINE: usize = MAXVA.wrapping_sub(PGSIZE);

/// map kernel stacks beneath the trampoline,
/// each surrounded by invalid guard pages.
pub const fn kstack(p: usize) -> usize {
    MAXVA - ((p + 1) * 2 * PGSIZE)
}

/// User memory layout.
/// Address zero first:
///   text
///   original data and bss
///   fixed-size stack
///   expandable heap
///   ...
///   TRAPFRAME (p->trapframe, used by the trampoline)
///   TRAMPOLINE (the same page as in the kernel)
pub const TRAPFRAME: usize = MAXVA.wrapping_sub(PGSIZE);
