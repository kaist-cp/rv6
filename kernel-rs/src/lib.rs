//! rv6: post-modernization of Unix Version 6 with Rust and RISC-V.

#![no_std]
#![deny(warnings)]
#![warn(missing_docs, missing_debug_implementations, rust_2018_idioms)]

mod panic;
mod utils;

/// # Safety
///
/// TODO: FFI.
#[no_mangle]
pub unsafe extern "C" fn hello_rs() {}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
