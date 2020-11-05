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

/// SiFive Test Finisher. (virt device only)
pub const FINISHER: usize = 0x100000;

/// qemu puts UART registers here in physical memory.
pub const UART0: usize = 0x10000000;
pub const UART0_IRQ: usize = 10;

/// virtio mmio interface
pub const VIRTIO0: usize = 0x10001000;
pub const VIRTIO0_IRQ: usize = 1;

/// local interrupt controller, which contains the timer.
pub const CLINT: usize = 0x2000000;
pub const fn clint_mtimecmp(hartid: usize) -> usize {
    CLINT
        .wrapping_add(0x4000)
        .wrapping_add(hartid.wrapping_mul(8))
}

/// cycles since boot.
pub const CLINT_MTIME: usize = CLINT.wrapping_add(0xbff8);

/// qemu puts programmable interrupt controller here.
pub const PLIC: usize = 0xc000000;
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

/// the kernel expects there to be RAM
/// for use by the kernel and user pages
/// from physical address 0x80000000 to PHYSTOP.
pub const KERNBASE: usize = 0x80000000;
pub const PHYSTOP: usize = KERNBASE.wrapping_add(128 * 1024 * 1024);

/// map the trampoline page to the highest address,
/// in both user and kernel space.
pub const TRAMPOLINE: usize = MAXVA.wrapping_sub(PGSIZE);

/// map kernel stacks beneath the trampoline,
/// each surrounded by invalid guard pages.
pub const fn kstack(p: usize) -> usize {
    TRAMPOLINE - ((p + 1) * 2 * PGSIZE)
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
pub const TRAPFRAME: usize = TRAMPOLINE.wrapping_sub(PGSIZE);
