//! Architecture-dependent code.

use super::interface::*;

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

pub struct RiscV;

impl Arch for RiscV {
    type Uart = uart::Uart;

    unsafe fn start() {
        unsafe {
            start::start();
        }
    }
}
