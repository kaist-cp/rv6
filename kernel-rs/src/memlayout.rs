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

use crate::addr::{MAXVA, PGSIZE};
use crate::arch::{interface::MemLayout, TargetArch};

/// User memory layout.
/// Address zero first:
///   text
///   original data and bss
///   fixed-size stack
///   expandable heap
///   ...
///   TRAPFRAME (p->trapframe, used by the trampoline)
///   TRAMPOLINE (the same page as in the kernel)
pub const TRAPFRAME: usize = TRAMPOLINE.wrapping_sub(PGSIZE);

/// map the trampoline page to the highest address,
/// in both user and kernel space.
pub const TRAMPOLINE: usize = MAXVA.wrapping_sub(PGSIZE);

/// map kernel stacks beneath the MAXVA,
/// each surrounded by invalid guard pages.
pub fn kstack(p: usize) -> usize {
    TRAMPOLINE - ((p + 1) * 2 * PGSIZE)
}

pub const PHYSTOP: usize = TargetArch::KERNBASE.wrapping_add(128 * 1024 * 1024);
