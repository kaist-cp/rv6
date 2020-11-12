//! rv6: post-modernization of Unix Version 6 with Rust and RISC-V.

#![no_std]
#![deny(warnings)]
#![warn(missing_docs, missing_debug_implementations, rust_2018_idioms)]
// Required for unused features in xv6 (see https://github.com/kaist-cp/rv6/issues/120 for details).
#![allow(dead_code)]
#![allow(incomplete_features)]
#![feature(llvm_asm)]
#![feature(const_fn_fn_ptr_basics)]
#![feature(const_wrapping_int_methods)]
#![feature(maybe_uninit_ref)]
#![feature(const_in_array_repeat_expressions)]
#![feature(array_value_iter)]
#![feature(const_fn)]
#![feature(maybe_uninit_extra)]
#![feature(min_const_generics)]
#![feature(generic_associated_types)]

mod arena;
mod bio;
mod console;
mod etrace;
mod exec;
mod fcntl;
mod file;
mod fs;
mod kalloc;
mod kernel;
mod log;
mod memlayout;
mod page;
mod param;
mod pipe;
mod plic;
mod poweroff;
mod printer;
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
#[macro_use]
extern crate array_const_fn_init;
