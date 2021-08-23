//! Architecture-dependent code.

use super::interface::Arch;

pub mod addr;
pub mod asm;
pub mod intr;
pub mod memlayout;
pub mod poweroff;
pub mod proc;
pub mod start;
pub mod timer;
pub mod trap;
pub mod uart;
pub mod vm;

pub struct Armv8;

impl Arch for Armv8 {
    type Uart = uart::Uart;

    unsafe fn start() {
        unsafe {
            start::start();
        }
    }
}
