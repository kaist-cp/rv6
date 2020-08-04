use crate::riscv::{MAXVA, PGSIZE};
/// Physical memory layout
/// qemu -machine virt is set up like this,
/// based on qemu's hw/riscv/virt.c:
///
/// 00001000 -- boot ROM, provided by qemu
/// 02000000 -- CLINT
/// 0C000000 -- PLIC
/// 10000000 -- uart0
/// 10001000 -- virtio disk
/// 80000000 -- boot ROM jumps here in machine mode
///             -kernel loads the kernel here
/// unused RAM after 80000000.
/// the kernel uses physical memory thus:
/// 80000000 -- entry.S, then kernel text and data
/// end -- start of kernel page allocation area
/// PHYSTOP -- end RAM used by the kernel

/// qemu puts UART registers here in physical memory.
pub const UART0: usize = 0x10000000;
pub const UART0_IRQ: i32 = 10;

/// virtio mmio interface
pub const VIRTIO0: i32 = 0x10001000;
pub const VIRTIO0_IRQ: i32 = 1;

/// local interrupt controller, which contains the timer.
pub const CLINT: i64 = 0x2000000;
pub const fn clint_mtimecmp(hartid: usize) -> usize {
    (CLINT + 0x4000 as i64 + (8 * hartid) as i64) as usize
}
pub const CLINT_MTIME: i64 = CLINT + 0xbff8 as i32 as i64;

/// qemu puts programmable interrupt controller here.
pub const PLIC: i64 = 0xc000000;
pub const PLIC_PENDING: i64 = PLIC + 0x1000;
pub const fn plic_senable(hart: i32) -> i64 {
    PLIC + 0x2080 as i32 as i64 + (hart * 0x100 as i32) as i64
}
pub const fn plic_spriority(hart: i32) -> i64 {
    PLIC + 0x201000 as i32 as i64 + (hart * 0x2000) as i64
}
pub const fn plic_sclaim(hart: i32) -> i64 {
    PLIC + 0x201004 as i64 + (hart * 0x2000) as i64
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
    TRAMPOLINE - ((p + 1 as i32) * 2 as i32 * PGSIZE) as i64
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
