//! rv6: post-modernization of Unix Version 6 with Rust and RISC-V.

#![no_std]
#![deny(warnings)]
#![warn(missing_docs, missing_debug_implementations, rust_2018_idioms)]
// Required for unused features in xv6 (see https://github.com/kaist-cp/rv6/issues/120 for details).
#![allow(dead_code)]
#![feature(llvm_asm)]
#![feature(c_variadic)]
#![feature(ptr_offset_from)]
#![feature(const_wrapping_int_methods)]
#![feature(maybe_uninit_ref)]
#![feature(const_in_array_repeat_expressions)]
#![feature(array_value_iter)]
#![feature(slice_ptr_range)]
#![feature(maybe_uninit_extra)]

// TODO(@jeehoonkang): we define `libc` module here because the `libc` crate doesn't work for the
// `riscv64gc-unknown-none-elfhf` target.
//
// Types are adopted from:
// https://github.com/rust-lang/libc/blob/master/src/unix/linux_like/linux/gnu/b64/riscv64/mod.rs
mod libc {
    pub type CVoid = core::ffi::c_void;
}

mod abort;
mod bio;
mod buf;
mod console;
mod elf;
mod etrace;
mod exec;
mod fcntl;
mod file;
mod fs;
mod kalloc;
mod kernel_main;
mod log;
mod memlayout;
mod page;
mod param;
mod pipe;
mod plic;
mod printf;
mod proc;
mod pool;
mod riscv;
mod sleeplock;
mod spinlock;
mod start;
mod stat;
mod string;
mod syscall;
mod sysfile;
mod sysproc;
mod trap;
mod uart;
mod utils;
mod virtio;
mod virtio_disk;
mod vm;

#[macro_use]
extern crate bitflags;
