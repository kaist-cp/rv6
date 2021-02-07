//! rv6: post-modernization of Unix Version 6 with Rust and RISC-V

#![no_std]
//
// # Tries to deny all lints (`rustc -W help`).
#![deny(absolute_paths_not_starting_with_crate)]
#![deny(anonymous_parameters)]
#![deny(box_pointers)]
#![deny(deprecated_in_future)]
#![deny(elided_lifetimes_in_paths)]
#![deny(explicit_outlives_requirements)]
#![deny(keyword_idents)]
#![deny(macro_use_extern_crate)]
#![deny(missing_debug_implementations)]
#![deny(non_ascii_idents)]
#![deny(pointer_structural_match)]
#![deny(rust_2018_idioms)]
#![deny(trivial_numeric_casts)]
#![deny(unaligned_references)]
#![deny(unsafe_op_in_unsafe_fn)]
#![deny(unused_crate_dependencies)]
#![deny(unused_extern_crates)]
#![deny(unused_import_braces)]
#![deny(unused_qualifications)]
#![deny(unused_results)]
#![deny(variant_size_differences)]
#![deny(warnings)]
//
// # TODO: deny them one day.
//
// #![deny(single_use_lifetimes)]
// #![deny(unreachable_pub)]
// #![deny(unused_lifetimes)]
//
// # The following lints should not be denied.
//
// #![deny(unstable_features)]
// #![deny(invalid_html_tags)]
// #![deny(missing_doc_code_examples)]
// #![deny(missing_docs)]
// #![deny(rustdoc)]
#![allow(dead_code)] // TODO(https://github.com/kaist-cp/rv6/issues/120)
#![allow(incomplete_features)]
#![feature(llvm_asm)]
#![feature(const_fn_fn_ptr_basics)]
#![feature(const_wrapping_int_methods)]
#![feature(maybe_uninit_ref)]
#![feature(const_in_array_repeat_expressions)]
#![feature(array_value_iter)]
#![feature(const_fn)]
#![feature(const_fn_union)]
#![feature(const_mut_refs)]
#![feature(maybe_uninit_extra)]
#![feature(generic_associated_types)]
#![feature(unsafe_block_in_unsafe_fn)]
#![feature(variant_count)]

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
mod list;
mod memlayout;
mod page;
mod param;
mod pinned_kernel;
mod pipe;
mod plic;
mod poweroff;
mod proc;
mod riscv;
mod sleepablelock;
mod sleeplock;
mod spinlock;
mod start;
mod stat;
mod syscall;
mod sysfile;
mod sysproc;
mod trap;
mod uart;
mod utils;
mod virtio;
mod virtio_disk;
mod vm;
