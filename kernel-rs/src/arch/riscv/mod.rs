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

pub type TargetArch = RiscV;

pub struct RiscV;

impl Arch for RiscV {
    type Uart = uart::RiscVUart;

    fn cpu_id() -> usize {
        asm::cpu_id()
    }
}
