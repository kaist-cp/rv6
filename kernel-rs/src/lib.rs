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
// #![deny(unstable_features)]
// #![deny(unused_lifetimes)]
//
// # The following lints should not be denied.
//
// #![deny(invalid_html_tags)]
// #![deny(missing_doc_code_examples)]
// #![deny(missing_docs)]
// #![deny(rustdoc)]
#![allow(incomplete_features)]
#![allow(clippy::upper_case_acronyms)]
#![feature(arbitrary_self_types)]
#![feature(asm)]
#![feature(arbitrary_self_types)]
#![feature(const_fn)]
#![feature(const_fn_fn_ptr_basics)]
#![feature(const_fn_union)]
#![feature(const_precise_live_drops)]
#![feature(const_trait_impl)]
#![feature(generic_associated_types)]
#![feature(maybe_uninit_extra)]
#![feature(maybe_uninit_ref)]
#![feature(variant_count)]

mod arch;
mod arena;
mod bio;
mod console;
mod exec;
mod file;
mod fs;
mod kalloc;
mod kernel;
mod lock;
mod page;
mod param;
mod pipe;
mod proc;
mod start;
mod syscall;
mod trap;
mod uart;
mod util;
mod virtio;
mod vm;
