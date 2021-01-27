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

// TODO(rv6): We must apply #[deny(unsafe_op_in_unsafe_fn)] to every module.
mod arena;
#[deny(unsafe_op_in_unsafe_fn)]
mod bio;
mod console;
#[deny(unsafe_op_in_unsafe_fn)]
mod etrace;
#[deny(unsafe_op_in_unsafe_fn)]
mod exec;
#[deny(unsafe_op_in_unsafe_fn)]
mod fcntl;
mod file;
mod fs;
#[deny(unsafe_op_in_unsafe_fn)]
mod kalloc;
mod kernel;
#[deny(unsafe_op_in_unsafe_fn)]
mod list;
#[deny(unsafe_op_in_unsafe_fn)]
mod memlayout;
mod page;
#[deny(unsafe_op_in_unsafe_fn)]
mod param;
mod pipe;
mod plic;
#[deny(unsafe_op_in_unsafe_fn)]
mod poweroff;
mod proc;
mod riscv;
mod sleepablelock;
mod sleeplock;
mod spinlock;
mod start;
#[deny(unsafe_op_in_unsafe_fn)]
mod stat;
mod syscall;
mod sysfile;
mod sysproc;
mod trap;
#[deny(unsafe_op_in_unsafe_fn)]
mod uart;
#[deny(unsafe_op_in_unsafe_fn)]
mod utils;
mod virtio;
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
