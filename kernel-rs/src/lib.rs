//! rv6: post-modernization of Unix Version 6 with Rust and RISC-V.

#![no_std]
#![deny(warnings)]
#![warn(missing_docs, missing_debug_implementations, rust_2018_idioms)]
#![allow(
    non_camel_case_types,
    elided_lifetimes_in_paths,
    unused_assignments,
    unused_mut,
    dead_code,
    unused_unsafe,
    non_upper_case_globals
)]
#![feature(asm)]
#![feature(llvm_asm)]
#![feature(extern_types)]
#![feature(c_variadic)]
#![feature(core_intrinsics)]
#![feature(ptr_wrapping_offset_from)]

// TODO(@jeehoonkang): we define `libc` module here because the `libc` crate doesn't work for the
// `riscv64gc-unknown-none-elfhf` target.
//
// Types are adopted from:
// https://github.com/rust-lang/libc/blob/master/src/unix/linux_like/linux/gnu/b64/riscv64/mod.rs
mod libc {
    pub type c_void = core::ffi::c_void;
    pub type c_char = u8;
    pub type c_uchar = u8;
    pub type c_short = i16;
    pub type c_ushort = u16;
    pub type c_int = i32;
    pub type c_uint = u32;
    pub type c_long = i64;
    pub type c_ulong = u64;
    pub type intptr_t = isize;
}

mod bio;
mod console;
mod exec;
mod file;
mod fs;
mod kalloc;
mod kernel_main;
mod log;
mod panic;
mod pipe;
mod plic;
mod printf;
mod proc;
mod sleeplock;
mod spinlock;
mod start;
mod string;
mod syscall;
mod sysfile;
mod sysproc;
mod trap;
mod uart;
mod utils;
mod virtio_disk;
mod vm;
mod stat;

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
