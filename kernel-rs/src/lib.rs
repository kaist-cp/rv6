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
#![feature(const_fn_union)]
#![feature(maybe_uninit_extra)]
#![feature(min_const_generics)]
#![feature(generic_associated_types)]
#![feature(unsafe_block_in_unsafe_fn)]

// TODO(https://github.com/kaist-cp/rv6/issues/335)
// We must apply #[deny(unsafe_op_in_unsafe_fn)] to every module.
#[deny(unsafe_op_in_unsafe_fn)]
mod arena;
#[deny(unsafe_op_in_unsafe_fn)]
mod bio;
#[deny(unsafe_op_in_unsafe_fn)]
mod console;
#[deny(unsafe_op_in_unsafe_fn)]
mod etrace;
#[deny(unsafe_op_in_unsafe_fn)]
mod exec;
#[deny(unsafe_op_in_unsafe_fn)]
mod fcntl;
#[deny(unsafe_op_in_unsafe_fn)]
mod file;

#[deny(unsafe_op_in_unsafe_fn)]
mod fs;
#[deny(unsafe_op_in_unsafe_fn)]
mod kalloc;
#[deny(unsafe_op_in_unsafe_fn)]
mod kernel;
#[deny(unsafe_op_in_unsafe_fn)]
mod list;
#[deny(unsafe_op_in_unsafe_fn)]
mod memlayout;
#[deny(unsafe_op_in_unsafe_fn)]
mod page;
#[deny(unsafe_op_in_unsafe_fn)]
mod param;
#[deny(unsafe_op_in_unsafe_fn)]
mod pipe;
#[deny(unsafe_op_in_unsafe_fn)]
mod plic;
#[deny(unsafe_op_in_unsafe_fn)]
mod poweroff;
#[deny(unsafe_op_in_unsafe_fn)]
mod proc;
mod riscv;
mod sleepablelock;
mod sleeplock;
mod spinlock;
mod start;
#[deny(unsafe_op_in_unsafe_fn)]
mod stat;
mod syscall;

#[deny(unsafe_op_in_unsafe_fn)]
mod sysfile;

#[deny(unsafe_op_in_unsafe_fn)]
mod sysproc;

#[deny(unsafe_op_in_unsafe_fn)]
mod trap;
#[deny(unsafe_op_in_unsafe_fn)]
mod uart;
#[deny(unsafe_op_in_unsafe_fn)]
mod utils;
#[deny(unsafe_op_in_unsafe_fn)]
mod virtio;
#[deny(unsafe_op_in_unsafe_fn)]
mod virtio_disk;
#[deny(unsafe_op_in_unsafe_fn)]
mod vm;

#[macro_use]
extern crate bitflags;
#[macro_use]
extern crate array_macro;
#[macro_use]
extern crate static_assertions;
#[macro_use]
extern crate itertools;
