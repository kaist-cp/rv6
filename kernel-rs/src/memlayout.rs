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
use crate::riscv::{MAXVA, PGSIZE};

/// qemu puts UART registers here in physical memory.
pub const UART0: usize = 0x10000000;
pub const UART0_IRQ: usize = 10;

/// virtio mmio interface
pub const VIRTIO0: usize = 0x10001000;
pub const VIRTIO0_IRQ: i32 = 1;

/// local interrupt controller, which contains the timer.
pub const CLINT: usize = 0x2000000;
pub const fn clint_mtimecmp(hartid: usize) -> usize {
    CLINT.wrapping_add(0x4000).wrapping_add(hartid.wrapping_mul(8))
}

/// cycles since boot.
pub const CLINT_MTIME: usize = CLINT.wrapping_add(0xbff8);

/// qemu puts programmable interrupt controller here.
pub const PLIC: usize = 0xc000000;
pub const PLIC_PENDING: usize = PLIC.wrapping_add(0x1000);
pub const fn plic_senable(hart: i32) -> usize {
    PLIC.wrapping_add(0x2080).wrapping_add((hart as usize).wrapping_mul(0x100))
}
pub const fn plic_spriority(hart: i32) -> i64 {
    PLIC as i64 + 0x201000 + (hart * 0x2000) as i64
}
pub const fn plic_sclaim(hart: i32) -> i64 {
    PLIC as i64 + 0x201004 + (hart * 0x2000) as i64
}

/// the kernel expects there to be RAM
/// for use by the kernel and user pages
/// from physical address 0x80000000 to PHYSTOP.
pub const KERNBASE: i64 = 0x80000000;
pub const PHYSTOP: i64 = KERNBASE + (128 * 1024 * 1024);

/// map the trampoline page to the highest address,
/// in both user and kernel space.
pub const TRAMPOLINE: i64 = MAXVA - PGSIZE as i64;

/// map kernel stacks beneath the trampoline,
/// each surrounded by invalid guard pages.
pub const fn kstack(p: i32) -> i64 {
    TRAMPOLINE - ((p + 1) * 2 * PGSIZE as i32) as i64
}

/// User memory layout.
/// Address zero first:
///   text
///   original data and bss
///   fixed-size stack
///   expandable heap
///   ...
///   TRAPFRAME (p->tf, used by the trampoline)
///   TRAMPOLINE (the same page as in the kernel)
pub const TRAPFRAME: i64 = TRAMPOLINE - PGSIZE as i64;
