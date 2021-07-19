//! Architecture-dependent code.

pub mod addr;
pub mod poweroff;
#[cfg(target_arch="aarch64")]
#[path = "arm/arm.rs"]
pub mod arm;
#[cfg(target_arch="aarch64")]
#[path = "arm/memlayout.rs"]
pub mod memlayout;

#[cfg(target_arch="riscv64")]
#[path = "riscv/memlayout.rs"]
pub mod memlayout;
#[cfg(target_arch="riscv64")]
#[path = "riscv/riscv.rs"]
pub mod riscv;
#[cfg(target_arch="riscv64")]
#[path = "riscv/plic.rs"]
pub mod plic;
