//! rv6: post-modernization of Unix Version 6 with Rust and RISC-V.

#![no_std]
#![deny(warnings)]
#![warn(missing_docs, missing_debug_implementations, rust_2018_idioms)]
// Required for unused features in xv6 (see https://github.com/kaist-cp/rv6/issues/120 for details).
#![allow(dead_code)]
#![feature(llvm_asm)]
#![feature(c_variadic)]
#![feature(const_wrapping_int_methods)]
#![feature(maybe_uninit_ref)]
#![feature(const_in_array_repeat_expressions)]
#![feature(array_value_iter)]
#![feature(slice_ptr_range)]
#![feature(maybe_uninit_extra)]
#![feature(min_const_generics)]
#![feature(once_cell)]

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
mod pool;
mod poweroff;
mod printf;
mod proc;
mod riscv;
mod sleepablelock;
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
