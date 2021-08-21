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

pub type TargetArch = ArmV8;

pub struct ArmV8;

impl Arch for ArmV8 {}
