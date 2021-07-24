//! Architecture-dependent code.

#[path = "../addr.rs"]
pub mod addr;
pub mod asm;
pub mod intr;
pub mod memlayout;
pub mod poweroff;
pub mod proc;
