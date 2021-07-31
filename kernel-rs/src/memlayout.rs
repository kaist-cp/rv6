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

pub trait MemLayout {
    /// qemu puts UART registers here in physical memory.
    const UART0: usize;

    /// virtio mmio interface
    const VIRTIO0: usize;

    /// the kernel expects there to be RAM
    /// for use by the kernel and user pages
    /// from physical address KERNBASE to PHYSTOP.
    const KERNBASE: usize;
    const PHYSTOP: usize = Self::KERNBASE.wrapping_add(128 * 1024 * 1024);

    const TRAMPOLINE: usize;
    const TRAPFRAME: usize;

    /// map kernel stacks beneath the trampoline,
    /// each surrounded by invalid guard pages.
    fn kstack(p: usize) -> usize;
}
